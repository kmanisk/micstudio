//! Neural Voice Denoising with RNNoise and Soft Hysteresis Gate.
//! Frame-based 480-sample processing with 70 Hz sub-rumble pre-filter.

use crate::dsp::biquad::{Biquad, FilterType};
use nnnoiseless::DenoiseState;

pub const RNN_FRAME_SIZE: usize = 480;

pub struct NeuralDenoise {
    denoiser: Box<DenoiseState<'static>>,
    hpf_70: Biquad,
    in_buffer: [f32; RNN_FRAME_SIZE],
    out_buffer: [f32; RNN_FRAME_SIZE],
    in_idx: usize,
    out_idx: usize,
    out_avail: usize,
    
    // Gate state machine
    vad_ema: f32,
    hangover_samples: usize,
    gate_gain: f32,
    
    // Controls
    enabled: bool,
    aggressiveness: f32, // 0.0 to 1.0 (Dusky default 0.70 to 0.96)
    vad_probability: f32,
}

impl NeuralDenoise {
    pub fn new(sample_rate: f32) -> Self {
        let mut hpf = Biquad::new();
        hpf.configure(FilterType::HighPass, 70.0, 0.707, 0.0, sample_rate);
        Self {
            denoiser: DenoiseState::new(),
            hpf_70: hpf,
            in_buffer: [0.0; RNN_FRAME_SIZE],
            out_buffer: [0.0; RNN_FRAME_SIZE],
            in_idx: 0,
            out_idx: 0,
            out_avail: 0,
            vad_ema: 0.0,
            hangover_samples: 0,
            gate_gain: 1.0,
            enabled: true,
            aggressiveness: 0.70,
            vad_probability: 0.0,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.gate_gain = 1.0;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_aggressiveness(&mut self, aggro: f32) {
        self.aggressiveness = aggro.clamp(0.0, 1.0);
    }

    pub fn vad_probability(&self) -> f32 {
        self.vad_probability
    }

    pub fn process_sample(&mut self, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }

        // 1. Rumble HPF
        let filtered = self.hpf_70.process(sample);
        self.in_buffer[self.in_idx] = filtered;
        self.in_idx += 1;

        // When 480 samples are collected, process frame
        if self.in_idx >= RNN_FRAME_SIZE {
            self.in_idx = 0;
            self.process_frame();
        }

        // Return processed sample from output buffer
        if self.out_avail > 0 {
            let out_sample = self.out_buffer[self.out_idx];
            self.out_idx += 1;
            self.out_avail -= 1;
            out_sample
        } else {
            // Passthrough during initial buffer fill
            filtered
        }
    }

    fn process_frame(&mut self) {
        let mut frame_out = [0.0f32; RNN_FRAME_SIZE];
        
        // RNNoise expects float scaled to approx 16-bit range (-32768 to 32768)
        let mut scaled_in = [0.0f32; RNN_FRAME_SIZE];
        for (i, &s) in self.in_buffer.iter().enumerate() {
            scaled_in[i] = s * 32768.0;
        }

        let vad = self.denoiser.process_frame(&mut frame_out, &scaled_in);
        self.vad_probability = vad;

        // Asymmetric EMA on VAD
        if vad > self.vad_ema {
            self.vad_ema = 0.6 * vad + 0.4 * self.vad_ema;
        } else {
            self.vad_ema = 0.05 * vad + 0.95 * self.vad_ema;
        }

        // Soft gate hysteresis
        let open_threshold = 0.40;
        let close_threshold = 0.15;
        let hangover_max = 12000; // 250 ms at 48 kHz

        if self.vad_ema >= open_threshold {
            self.hangover_samples = hangover_max;
        } else if self.hangover_samples > 0 {
            self.hangover_samples = self.hangover_samples.saturating_sub(RNN_FRAME_SIZE);
        }

        // Calculate target attenuation
        let is_speech = self.vad_ema >= close_threshold || self.hangover_samples > 0;
        let target_gain = if is_speech {
            1.0
        } else {
            // Attenuation down to -24 dB based on aggressiveness
            (1.0 - 0.94 * self.aggressiveness).max(0.04)
        };

        // Smooth gate gain across frame
        for i in 0..RNN_FRAME_SIZE {
            // 5ms smoothing
            self.gate_gain = 0.98 * self.gate_gain + 0.02 * target_gain;
            let raw_sample = frame_out[i] / 32768.0;
            self.out_buffer[i] = raw_sample * self.gate_gain;
        }

        self.out_idx = 0;
        self.out_avail = RNN_FRAME_SIZE;
    }

    pub fn reset(&mut self) {
        self.in_idx = 0;
        self.out_idx = 0;
        self.out_avail = 0;
        self.vad_ema = 0.0;
        self.hangover_samples = 0;
        self.gate_gain = 1.0;
        self.hpf_70.reset();
    }
}
