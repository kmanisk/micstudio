//! Master Audio Processing Pipeline chaining all real-time DSP stages.
//! Architecture:
//!   Hardware Mic -> [Cleanup: HPF -> RNNoise/Blend -> Gate -> 9-Band EQ -> Compressor]
//!                -> [Voice FX: Pitch -> Vocoder/RingMod -> Bandpass -> Bitcrusher -> Stutter -> Post-EQ]
//!                -> [Safety: True Peak Limiter] -> Output

use crate::dsp::denoise::NeuralDenoise;
use crate::dsp::effects::{BandpassEffect, Bitcrusher, Compressor, Limiter, StutterGate};
use crate::dsp::eq::ParametricEq;
use crate::dsp::pitch::PitchShifter;
use crate::dsp::vocoder::Vocoder;
use crate::presets::Preset;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct AudioTelemetry {
    pub input_rms_db: f32,
    pub output_rms_db: f32,
    pub peak_level: f32,
    pub vad_probability: f32,
    pub xruns: u64,
    pub hw_adc_clipped: bool,
    pub digital_limiting: bool,
}

pub struct DspPipeline {
    denoise: NeuralDenoise,
    pitch: PitchShifter,
    vocoder: Vocoder,
    bandpass: BandpassEffect,
    bitcrusher: Bitcrusher,
    stutter: StutterGate,
    eq: ParametricEq,
    compressor: Compressor,
    limiter: Limiter,

    // Bypass & diagnostic flags
    cleanup_enabled: bool,
    fx_enabled: bool,

    // Telemetry accumulators
    in_sum_sq: f32,
    out_sum_sq: f32,
    peak_hold: f32,
    telemetry_samples: usize,
    latest_telemetry: AudioTelemetry,
    sample_rate: f32,
}

impl DspPipeline {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            denoise: NeuralDenoise::new(sample_rate),
            pitch: PitchShifter::new(),
            vocoder: Vocoder::new(sample_rate),
            bandpass: BandpassEffect::new(sample_rate),
            bitcrusher: Bitcrusher::new(),
            stutter: StutterGate::new(sample_rate),
            eq: ParametricEq::new(sample_rate),
            compressor: Compressor::new(sample_rate),
            limiter: Limiter::new(),
            cleanup_enabled: true,
            fx_enabled: true,
            in_sum_sq: 0.0,
            out_sum_sq: 0.0,
            peak_hold: 0.0,
            telemetry_samples: 0,
            latest_telemetry: AudioTelemetry::default(),
            sample_rate,
        }
    }

    pub fn apply_preset(&mut self, p: &Preset) {
        // Noise suppression is ALWAYS active as stage 1 for every preset (Windows / studio standard)
        self.denoise.set_enabled(true);
        self.denoise.set_aggressiveness(p.noise_gate_aggro);

        // Determine whether creative voice transformation FX should run (bypassed for Natural Clean)
        let has_fx = p.pitch_semitones.abs() > 0.01
            || p.vocoder_mix > 0.01
            || p.matrix_ring_mod > 0.01
            || p.bandpass_hpf > 20.0
            || p.bitcrush_bits < 16
            || p.bitcrush_downsample > 1
            || p.stutter_rate > 0.01;

        self.fx_enabled = has_fx;

        // Pitch Shift
        self.pitch.set_semitones(p.pitch_semitones);
        self.pitch.reset();

        // Vocoder & Ring Mod
        self.vocoder.set_params(
            p.vocoder_mix,
            p.vocoder_carrier_hz,
            p.vocoder_detune,
            p.matrix_ring_mod,
        );
        self.vocoder.reset();

        // Bandpass (Walkie Talkie)
        self.bandpass
            .set_params(p.bandpass_hpf, p.bandpass_lpf, self.sample_rate);
        self.bandpass.reset();

        // Bitcrusher
        self.bitcrusher
            .set_params(p.bitcrush_bits, p.bitcrush_downsample);
        self.bitcrusher.reset();

        // Stutter (Cyber Chopper)
        self.stutter.set_params(p.stutter_rate, p.stutter_duty);
        self.stutter.reset();

        // EQ & Post Gain
        self.eq.set_enabled(p.eq_enabled);
        self.eq.set_gains(p.eq_gains);
        self.eq.set_post_gain(p.eq_post_gain);

        // Dynamics
        self.compressor.set_enabled(p.compressor_enabled);
        self.limiter.set_enabled(p.limiter_enabled);
    }

    // Diagnostic bypass controls
    pub fn set_cleanup_enabled(&mut self, enabled: bool) {
        self.cleanup_enabled = enabled;
    }

    pub fn set_fx_enabled(&mut self, enabled: bool) {
        self.fx_enabled = enabled;
    }

    pub fn set_hpf_enabled(&mut self, enabled: bool) {
        self.denoise.set_hpf_enabled(enabled);
    }

    pub fn set_denoise_enabled(&mut self, enabled: bool) {
        self.denoise.set_enabled(enabled);
    }

    pub fn set_denoise_blend(&mut self, blend: f32) {
        self.denoise.set_blend(blend);
    }

    pub fn set_gate_enabled(&mut self, enabled: bool) {
        self.denoise.set_gate_enabled(enabled);
    }

    pub fn set_limiter_enabled(&mut self, enabled: bool) {
        self.limiter.set_enabled(enabled);
    }

    #[inline(always)]
    pub fn process_sample(&mut self, in_sample: f32) -> f32 {
        self.in_sum_sq += in_sample * in_sample;

        let mut s = in_sample;

        // Stage 1: Microphone Cleanup (70 Hz HPF -> RNNoise 480-sample frame -> Hysteresis Gate)
        if self.cleanup_enabled {
            s = self.denoise.process_sample(s);
        }

        // Stage 2: 9-Band Studio EQ & Tone Shaping (Stage 2)
        s = self.eq.process(s);

        // Stage 3: Studio Vocal Compressor (Stage 3)
        s = self.compressor.process(s);

        // Stage 4: Voice Transformation FX (Stage 4, bypassed for Natural Clean)
        if self.fx_enabled {
            s = self.pitch.process(s);
            s = self.vocoder.process(s);
            s = self.bandpass.process(s);
            s = self.bitcrusher.process(s);
            s = self.stutter.process(s);
        }

        // Stage 5: Brickwall Safety Limiter (Stage 5, -1 dBFS peak guard)
        s = self.limiter.process(s);

        // Safety clamp strictly at -1 dBFS (0.891251)
        let out_sample = s.clamp(-0.891251, 0.891251);

        self.out_sum_sq += out_sample * out_sample;
        let abs_out = out_sample.abs();
        if abs_out > self.peak_hold {
            self.peak_hold = abs_out;
        }

        self.telemetry_samples += 1;
        if self.telemetry_samples >= 1000 {
            let in_rms = (self.in_sum_sq / self.telemetry_samples as f32).sqrt();
            let out_rms = (self.out_sum_sq / self.telemetry_samples as f32).sqrt();

            self.latest_telemetry.input_rms_db = if in_rms > 1e-5 {
                20.0 * in_rms.log10()
            } else {
                -96.0
            };
            self.latest_telemetry.output_rms_db = if out_rms > 1e-5 {
                20.0 * out_rms.log10()
            } else {
                -96.0
            };
            self.latest_telemetry.peak_level = self.peak_hold;
            self.latest_telemetry.vad_probability = self.denoise.vad_probability();

            self.in_sum_sq = 0.0;
            self.out_sum_sq = 0.0;
            self.peak_hold = 0.0;
            self.telemetry_samples = 0;
        }

        out_sample
    }

    pub fn get_telemetry(&self) -> AudioTelemetry {
        self.latest_telemetry
    }

    pub fn reset(&mut self) {
        self.denoise.reset();
        self.pitch.reset();
        self.vocoder.reset();
        self.bandpass.reset();
        self.bitcrusher.reset();
        self.stutter.reset();
        self.eq.reset();
        self.compressor.reset();
        self.limiter.reset();
        self.in_sum_sq = 0.0;
        self.out_sum_sq = 0.0;
        self.peak_hold = 0.0;
        self.telemetry_samples = 0;
    }
}
