//! Master Audio Processing Pipeline chaining all real-time DSP stages.

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
            in_sum_sq: 0.0,
            out_sum_sq: 0.0,
            peak_hold: 0.0,
            telemetry_samples: 0,
            latest_telemetry: AudioTelemetry::default(),
            sample_rate,
        }
    }

    pub fn apply_preset(&mut self, p: &Preset) {
        // Denoise
        self.denoise.set_enabled(p.noise_suppression);
        self.denoise.set_aggressiveness(p.noise_gate_aggro);

        // Pitch shift
        self.pitch.set_semitones(p.pitch_semitones);

        // Vocoder & Ring Mod
        self.vocoder.set_params(p.vocoder_mix, p.vocoder_carrier_hz, p.vocoder_detune, p.matrix_ring_mod);

        // Bandpass
        self.bandpass.set_params(p.bandpass_hpf, p.bandpass_lpf, self.sample_rate);

        // Bitcrusher
        self.bitcrusher.set_params(p.bitcrush_bits, p.bitcrush_downsample);

        // Stutter
        self.stutter.set_params(p.stutter_rate, p.stutter_duty);

        // EQ
        self.eq.set_enabled(p.eq_enabled);
        self.eq.set_gains(p.eq_gains);
        self.eq.set_post_gain(p.eq_post_gain);

        // Dynamics
        self.compressor.set_enabled(p.compressor_enabled);
        self.limiter.set_enabled(p.limiter_enabled);
    }

    #[inline(always)]
    pub fn process_sample(&mut self, in_sample: f32) -> f32 {
        self.in_sum_sq += in_sample * in_sample;

        // Stage 1: Denoise + Soft Gate
        let mut s = self.denoise.process_sample(in_sample);

        // Stage 2: Pitch Shift
        s = self.pitch.process(s);

        // Stage 3: Vocoder & Matrix Ring Mod
        s = self.vocoder.process(s);

        // Stage 4: Resonant Bandpass
        s = self.bandpass.process(s);

        // Stage 5: Lo-Fi Bitcrusher
        s = self.bitcrusher.process(s);

        // Stage 6: Stutter Gate
        s = self.stutter.process(s);

        // Stage 7: 9-Band Studio EQ
        s = self.eq.process(s);

        // Stage 8: Studio Compressor
        s = self.compressor.process(s);

        // Stage 9: Peak Limiter
        s = self.limiter.process(s);

        // Clamp to valid range
        let out_sample = s.clamp(-1.0, 1.0);

        self.out_sum_sq += out_sample * out_sample;
        let abs_out = out_sample.abs();
        if abs_out > self.peak_hold {
            self.peak_hold = abs_out;
        }

        self.telemetry_samples += 1;
        // Update telemetry every ~1000 samples (~20ms at 48kHz)
        if self.telemetry_samples >= 1000 {
            let in_rms = (self.in_sum_sq / self.telemetry_samples as f32).sqrt();
            let out_rms = (self.out_sum_sq / self.telemetry_samples as f32).sqrt();

            self.latest_telemetry.input_rms_db = if in_rms > 1e-5 { 20.0 * in_rms.log10() } else { -96.0 };
            self.latest_telemetry.output_rms_db = if out_rms > 1e-5 { 20.0 * out_rms.log10() } else { -96.0 };
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
