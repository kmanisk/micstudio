//! 9-Band Studio Parametric Equalizer matching the Dusky vocal profile.

use crate::dsp::biquad::{Biquad, FilterType};

pub const EQ_FREQS: [f32; 9] = [
    80.0, 120.0, 250.0, 400.0, 1500.0, 3500.0, 6000.0, 9000.0, 12000.0,
];
pub const EQ_QS: [f32; 9] = [0.707, 0.707, 1.0, 1.0, 1.0, 0.707, 1.0, 0.707, 1.0];

#[derive(Debug, Clone)]
pub struct ParametricEq {
    bands: [Biquad; 9],
    gains: [f32; 9],
    post_gain_db: f32,
    enabled: bool,
    sample_rate: f32,
}

impl ParametricEq {
    pub fn new(sample_rate: f32) -> Self {
        let mut eq = Self {
            bands: [Biquad::new(); 9],
            gains: [0.0; 9],
            post_gain_db: 0.0,
            enabled: false,
            sample_rate,
        };
        eq.recalculate();
        eq
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_gains(&mut self, gains: [f32; 9]) {
        self.gains = gains;
        self.recalculate();
    }

    pub fn set_post_gain(&mut self, gain_db: f32) {
        self.post_gain_db = gain_db.clamp(-36.0, 36.0);
    }

    pub fn recalculate(&mut self) {
        for (i, b) in self.bands.iter_mut().enumerate() {
            let filter_type = if i < 2 {
                FilterType::LowShelf
            } else if i >= 7 {
                FilterType::HighShelf
            } else {
                FilterType::Peaking
            };
            b.configure(
                filter_type,
                EQ_FREQS[i],
                EQ_QS[i],
                self.gains[i],
                self.sample_rate,
            );
        }
    }

    #[inline(always)]
    pub fn process(&mut self, mut sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }
        for b in &mut self.bands {
            sample = b.process(sample);
        }
        if self.post_gain_db != 0.0 {
            let linear_post = 10.0f32.powf(self.post_gain_db / 20.0);
            sample *= linear_post;
        }
        sample
    }

    pub fn reset(&mut self) {
        for b in &mut self.bands {
            b.reset();
        }
    }
}
