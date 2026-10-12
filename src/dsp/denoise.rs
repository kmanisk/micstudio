//! Neural Voice Denoising with RNNoise and Soft Hysteresis Gate.
//! Frame-based 480-sample processing with 70 Hz sub-rumble pre-filter
//! and phase-aligned dry/wet blend.

use crate::dsp::biquad::{Biquad, FilterType};
use nnnoiseless::DenoiseState;

pub const RNN_FRAME_SIZE: usize = 480;
pub const DRY_DELAY_LEN: usize = 2 * RNN_FRAME_SIZE; // 960 samples = 20ms aligned RNNoise latency

pub struct NeuralDenoise {
    denoiser: Box<DenoiseState<'static>>,
    hpf_70: Biquad,
    
    // Frame buffers
    in_buffer: [f32; RNN_FRAME_SIZE],
    out_buffer: [f32; RNN_FRAME_SIZE],
    in_idx: usize,
    out_idx: usize,
    out_avail: usize,
    
    // Phase-aligned dry delay buffer (960 samples to match 2-frame RNNoise latency)
    dry_delay: [f32; DRY_DELAY_LEN],
    dry_delay_idx: usize,

    // Gate state machine
    vad_ema: f32,
    hangover_samples: usize,
    hangover_max: usize,
    gate_gain: f32,
    
    // Controls
    enabled: bool,
    hpf_enabled: bool,
    gate_enabled: bool,
    denoise_blend: f32, // 0.0 (full dry) to 1.0 (full wet RNNoise)
    aggressiveness: f32, // 0.0 to 1.0
    vad_probability: f32,
    sample_rate: f32,
}

impl NeuralDenoise {
    pub fn new(sample_rate: f32) -> Self {
        let mut hpf = Biquad::new();
        hpf.configure(FilterType::HighPass, 70.0, 0.707, 0.0, sample_rate);
        let hangover_max = (0.500 * sample_rate) as usize; // 500 ms hangover (matching 40-mic.conf)

        Self {
            denoiser: DenoiseState::new(),
            hpf_70: hpf,
            in_buffer: [0.0; RNN_FRAME_SIZE],
            out_buffer: [0.0; RNN_FRAME_SIZE],
            in_idx: 0,
            out_idx: 0,
            out_avail: RNN_FRAME_SIZE, // Start with latency pre-buffered as silence
            dry_delay: [0.0; DRY_DELAY_LEN],
            dry_delay_idx: 0,
            vad_ema: 0.0,
            hangover_samples: 0,
            hangover_max,
            gate_gain: 1.0,
            enabled: true,
            hpf_enabled: true,
            gate_enabled: true,
            denoise_blend: 1.0, // 100% wet default
            aggressiveness: 0.35,
            vad_probability: 0.0,
            sample_rate,
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

    pub fn set_hpf_enabled(&mut self, enabled: bool) {
        self.hpf_enabled = enabled;
    }

    pub fn set_gate_enabled(&mut self, enabled: bool) {
        self.gate_enabled = enabled;
        if !enabled {
            self.gate_gain = 1.0;
        }
    }

    pub fn set_blend(&mut self, blend: f32) {
        self.denoise_blend = blend.clamp(0.0, 1.0);
    }

    pub fn set_aggressiveness(&mut self, aggro: f32) {
        self.aggressiveness = aggro.clamp(0.0, 1.0);
    }

    pub fn set_hangover_ms(&mut self, ms: u32) {
        self.hangover_max = ((ms as f32 * 0.001) * self.sample_rate) as usize;
    }

    pub fn vad_probability(&self) -> f32 {
        self.vad_probability
    }

    #[inline(always)]
    pub fn process_sample(&mut self, sample: f32) -> f32 {
        // High-pass filter stage
        let filtered = if self.hpf_enabled {
            self.hpf_70.process(sample)
        } else {
            sample
        };

        if !self.enabled {
            return filtered;
        }

        // Store into dry delay line for phase-aligned dry/wet blending
        let dry_sample = self.dry_delay[self.dry_delay_idx];
        self.dry_delay[self.dry_delay_idx] = filtered;
        self.dry_delay_idx = (self.dry_delay_idx + 1) % DRY_DELAY_LEN;

        self.in_buffer[self.in_idx] = filtered;
        self.in_idx += 1;

        // When 480 samples are collected, run RNNoise inference
        if self.in_idx >= RNN_FRAME_SIZE {
            self.in_idx = 0;
            self.process_frame();
        }

        // Retrieve denoised sample from output buffer
        let wet_sample = if self.out_avail > 0 {
            let s = self.out_buffer[self.out_idx];
            self.out_idx += 1;
            self.out_avail -= 1;
            s
        } else {
            0.0
        };

        // Phase-aligned dry/wet blend
        (1.0 - self.denoise_blend) * dry_sample + self.denoise_blend * wet_sample
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

        // Speech gate hysteresis
        let open_threshold = 0.30;
        let close_threshold = 0.12;

        if self.vad_ema >= open_threshold {
            self.hangover_samples = self.hangover_max;
        } else if self.hangover_samples > 0 {
            self.hangover_samples = self.hangover_samples.saturating_sub(RNN_FRAME_SIZE);
        }

        let is_speech = !self.gate_enabled || self.vad_ema >= close_threshold || self.hangover_samples > 0;
        let target_gain = if is_speech {
            1.0
        } else {
            // Attenuation down to -24 dB based on aggressiveness
            (1.0 - 0.94 * self.aggressiveness).max(0.04)
        };

        // Smooth gate gain across frame samples
        for i in 0..RNN_FRAME_SIZE {
            self.gate_gain = 0.98 * self.gate_gain + 0.02 * target_gain;
            let raw_sample = frame_out[i] / 32768.0;
            self.out_buffer[i] = raw_sample * self.gate_gain;
        }

        self.out_idx = 0;
        self.out_avail = RNN_FRAME_SIZE;
    }

    pub fn reset(&mut self) {
        self.denoiser = DenoiseState::new();
        self.in_buffer.fill(0.0);
        self.out_buffer.fill(0.0);
        self.dry_delay.fill(0.0);
        self.in_idx = 0;
        self.out_idx = 0;
        self.out_avail = RNN_FRAME_SIZE;
        self.dry_delay_idx = 0;
        self.vad_ema = 0.0;
        self.hangover_samples = 0;
        self.gate_gain = 1.0;
        self.hpf_70.reset();
    }
}
