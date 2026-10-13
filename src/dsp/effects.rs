//! Vocal Effects: Bandpass Shaping, Bitcrusher, Stutter Gate, Compressor, and Limiter.

use crate::dsp::biquad::{Biquad, FilterType};

/// Resonant telephone, walkie-talkie, and megaphone bandpass filter.
pub struct BandpassEffect {
    hpf: Biquad,
    lpf: Biquad,
    presence: Biquad,
    makeup_gain: f32,
    enabled: bool,
}

impl BandpassEffect {
    pub fn new(_sample_rate: f32) -> Self {
        Self {
            hpf: Biquad::new(),
            lpf: Biquad::new(),
            presence: Biquad::new(),
            makeup_gain: 1.0,
            enabled: false,
        }
    }

    pub fn set_params(&mut self, hpf_hz: f32, lpf_hz: f32, sample_rate: f32) {
        if hpf_hz > 20.0 && lpf_hz > hpf_hz {
            // 2nd-order HPF and LPF for authentic radio bandlimiting
            self.hpf
                .configure(FilterType::HighPass, hpf_hz, 0.707, 0.0, sample_rate);
            self.lpf
                .configure(FilterType::LowPass, lpf_hz, 0.707, 0.0, sample_rate);
            // Mid-range presence peak at 1.8 kHz for carbon-mic telephone bite
            self.presence
                .configure(FilterType::Peaking, 1800.0, 1.2, 4.0, sample_rate);
            self.makeup_gain = 1.414; // +3 dB makeup for band-limited energy loss
            self.enabled = true;
        } else {
            self.enabled = false;
        }
    }

    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }
        let hp = self.hpf.process(sample);
        let lp = self.lpf.process(hp);
        let mid = self.presence.process(lp);
        mid * self.makeup_gain
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn reset(&mut self) {
        self.hpf.reset();
        self.lpf.reset();
        self.presence.reset();
    }
}

/// Lo-Fi Vintage Bitcrusher (bit depth truncation & sample-and-hold downsampling).
pub struct Bitcrusher {
    bits: u32,
    downsample_factor: u32,
    hold_counter: u32,
    held_sample: f32,
    enabled: bool,
}

impl Bitcrusher {
    pub fn new() -> Self {
        Self {
            bits: 16,
            downsample_factor: 1,
            hold_counter: 0,
            held_sample: 0.0,
            enabled: false,
        }
    }

    pub fn set_params(&mut self, bits: u32, downsample: u32) {
        self.bits = bits.clamp(2, 16);
        self.downsample_factor = downsample.max(1);
        self.enabled = self.bits < 16 || self.downsample_factor > 1;
    }

    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }

        self.hold_counter += 1;
        if self.hold_counter >= self.downsample_factor {
            self.hold_counter = 0;
            // Quantize amplitude
            let levels = (1 << self.bits) as f32;
            let quantized = (sample * levels * 0.5).round() / (levels * 0.5);
            self.held_sample = quantized.clamp(-1.0, 1.0);
        }

        self.held_sample
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn reset(&mut self) {
        self.hold_counter = 0;
        self.held_sample = 0.0;
    }
}

/// Rhythmic amplitude chopper / stutter gate with anti-click smoothing.
pub struct StutterGate {
    rate_hz: f32,
    duty_cycle: f32,
    phase: f32,
    current_gain: f32,
    enabled: bool,
    sample_rate: f32,
}

impl StutterGate {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            rate_hz: 0.0,
            duty_cycle: 0.5,
            phase: 0.0,
            current_gain: 1.0,
            enabled: false,
            sample_rate,
        }
    }

    pub fn set_params(&mut self, rate_hz: f32, duty_cycle: f32) {
        self.rate_hz = rate_hz.clamp(0.0, 40.0);
        self.duty_cycle = duty_cycle.clamp(0.1, 0.9);
        self.enabled = self.rate_hz > 0.5;
    }

    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }

        if sample.abs() > 1e-4 {
            let step = self.rate_hz / self.sample_rate;
            self.phase += step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }

        let target_gain = if self.phase < self.duty_cycle {
            1.0
        } else {
            0.0
        };
        // 5ms anti-click exponential smoothing
        let smooth_coeff = 1.0 - (-1.0 / (0.005 * self.sample_rate)).exp();
        self.current_gain += smooth_coeff * (target_gain - self.current_gain);
        sample * self.current_gain
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.current_gain = 1.0;
    }
}

/// Studio Vocal Compressor (gentle vocal leveling with anti-shout dynamic hold protection).
pub struct Compressor {
    threshold_linear: f32,
    ratio: f32,
    attack_coeff: f32,
    release_coeff: f32,
    makeup_linear: f32,
    boost_threshold: f32,
    boost_linear: f32,
    envelope: f32,
    hold_samples: usize,
    hold_counter: usize,
    enabled: bool,
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        // -18 dBFS threshold, 3.5:1 ratio, 5ms attack, 120ms hold, 500ms release, +4 dB makeup
        let threshold_db = -18.0f32;
        let makeup_db = 4.0f32;
        let attack_ms = 5.0f32;
        let hold_ms = 120.0f32;
        let release_ms = 500.0f32;

        Self {
            threshold_linear: 10.0f32.powf(threshold_db / 20.0),
            ratio: 3.5,
            attack_coeff: (-1.0 / (attack_ms * 0.001 * sample_rate)).exp(),
            release_coeff: (-1.0 / (release_ms * 0.001 * sample_rate)).exp(),
            makeup_linear: 10.0f32.powf(makeup_db / 20.0),
            boost_threshold: 10.0f32.powf(-50.0 / 20.0), // -50 dBFS
            boost_linear: 10.0f32.powf(6.0 / 20.0),      // +6 dB boost for whisper/soft speech
            envelope: 0.0,
            hold_samples: (hold_ms * 0.001 * sample_rate) as usize,
            hold_counter: 0,
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }

        let abs_sample = sample.abs();
        if abs_sample > self.envelope {
            self.envelope =
                self.attack_coeff * self.envelope + (1.0 - self.attack_coeff) * abs_sample;
            self.hold_counter = self.hold_samples;
        } else if self.hold_counter > 0 {
            self.hold_counter -= 1;
            // Hold peak envelope steady to guard against shouting transients without pumping
        } else {
            self.envelope =
                self.release_coeff * self.envelope + (1.0 - self.release_coeff) * abs_sample;
        }

        let mut gain = if self.envelope > self.threshold_linear {
            let over_db = 20.0 * (self.envelope / self.threshold_linear).log10();
            let reduced_db = over_db * (1.0 - 1.0 / self.ratio);
            10.0f32.powf(-reduced_db / 20.0)
        } else {
            1.0
        };

        // Upward expansion for quiet speech above noise floor
        if self.envelope > self.boost_threshold && self.envelope < self.threshold_linear {
            let boost_factor = (self.threshold_linear - self.envelope)
                / (self.threshold_linear - self.boost_threshold);
            gain *= 1.0 + (self.boost_linear - 1.0) * boost_factor;
        }

        sample * gain * self.makeup_linear
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
        self.hold_counter = 0;
    }
}

/// Brickwall True Peak Limiter (-1 dBFS safety ceiling).
pub struct Limiter {
    ceiling_linear: f32,
    envelope: f32,
    enabled: bool,
}

impl Limiter {
    pub fn new() -> Self {
        Self {
            ceiling_linear: 0.891, // -1 dBFS
            envelope: 0.0,
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }

        let abs_sample = sample.abs();
        if abs_sample > self.envelope {
            self.envelope = abs_sample; // Instantaneous brickwall peak detection
        } else {
            // Smooth release (~40ms release at 48kHz)
            self.envelope = 0.9995 * self.envelope + 0.0005 * abs_sample;
        }

        let limited = if self.envelope > self.ceiling_linear {
            let gain = self.ceiling_linear / self.envelope;
            sample * gain
        } else {
            sample
        };

        limited.clamp(-self.ceiling_linear, self.ceiling_linear)
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
    }
}
