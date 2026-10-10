//! 16-Band Vocoder Filterbank and Carrier Synthesizer with Matrix Ring-Modulation.

use crate::dsp::biquad::{Biquad, FilterType};

pub const NUM_VOCODER_BANDS: usize = 16;
pub const VOCODER_FREQS: [f32; NUM_VOCODER_BANDS] = [
    120.0, 180.0, 260.0, 360.0, 500.0, 680.0, 900.0, 1180.0,
    1500.0, 1900.0, 2400.0, 3000.0, 3800.0, 4800.0, 6000.0, 7500.0,
];

pub struct Vocoder {
    voice_filters: [Biquad; NUM_VOCODER_BANDS],
    carrier_filters: [Biquad; NUM_VOCODER_BANDS],
    envelopes: [f32; NUM_VOCODER_BANDS],
    
    // Internal sawtooth carrier oscillators
    carrier_phase1: f32,
    carrier_phase2: f32,
    carrier_freq: f32,
    detune: f32, // Hz
    
    // Matrix ring-mod oscillator (35 Hz)
    ring_phase: f32,
    ring_intensity: f32, // 0.0 to 1.0
    
    // Controls
    enabled: bool,
    mix: f32, // 0.0 (dry) to 1.0 (full wet vocoder)
    sample_rate: f32,
}

impl Vocoder {
    pub fn new(sample_rate: f32) -> Self {
        let mut vf = [Biquad::new(); NUM_VOCODER_BANDS];
        let mut cf = [Biquad::new(); NUM_VOCODER_BANDS];

        for i in 0..NUM_VOCODER_BANDS {
            vf[i].configure(FilterType::BandPass, VOCODER_FREQS[i], 3.0, 0.0, sample_rate);
            cf[i].configure(FilterType::BandPass, VOCODER_FREQS[i], 3.0, 0.0, sample_rate);
        }

        Self {
            voice_filters: vf,
            carrier_filters: cf,
            envelopes: [0.0; NUM_VOCODER_BANDS],
            carrier_phase1: 0.0,
            carrier_phase2: 0.0,
            carrier_freq: 110.0,
            detune: 1.5,
            ring_phase: 0.0,
            ring_intensity: 0.0,
            enabled: false,
            mix: 0.0,
            sample_rate,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_params(&mut self, mix: f32, carrier_hz: f32, detune: f32, ring_mod: f32) {
        self.mix = mix.clamp(0.0, 1.0);
        self.carrier_freq = carrier_hz.clamp(40.0, 880.0);
        self.detune = detune.clamp(0.0, 50.0);
        self.ring_intensity = ring_mod.clamp(0.0, 1.0);
        self.enabled = self.mix > 0.05 || self.ring_intensity > 0.05;
    }

    #[inline(always)]
    pub fn process(&mut self, voice_input: f32) -> f32 {
        if !self.enabled {
            return voice_input;
        }

        // 1. Synthesize dual detuned sawtooth carrier
        let step1 = self.carrier_freq / self.sample_rate;
        let step2 = (self.carrier_freq + self.detune) / self.sample_rate;

        let saw1 = 2.0 * (self.carrier_phase1 - 0.5);
        let saw2 = 2.0 * (self.carrier_phase2 - 0.5);
        let carrier = 0.5 * (saw1 + saw2);

        self.carrier_phase1 += step1;
        if self.carrier_phase1 >= 1.0 {
            self.carrier_phase1 -= 1.0;
        }

        self.carrier_phase2 += step2;
        if self.carrier_phase2 >= 1.0 {
            self.carrier_phase2 -= 1.0;
        }

        // 2. Filterbank Vocoder Modulation
        let mut vocoded = 0.0f32;
        for i in 0..NUM_VOCODER_BANDS {
            let v_band = self.voice_filters[i].process(voice_input);
            let c_band = self.carrier_filters[i].process(carrier);

            // Envelope follower (fast attack 5ms, slow release 30ms)
            let v_rect = v_band.abs();
            if v_rect > self.envelopes[i] {
                self.envelopes[i] = 0.8 * self.envelopes[i] + 0.2 * v_rect;
            } else {
                self.envelopes[i] = 0.98 * self.envelopes[i] + 0.02 * v_rect;
            }

            vocoded += c_band * self.envelopes[i];
        }

        // Normalize and scale vocoder speech
        vocoded *= 6.0;

        // 3. Matrix Ring Modulation (35 Hz carrier with smooth saturation)
        let ring_out = if self.ring_intensity > 0.01 {
            let ring_step = 35.0 / self.sample_rate;
            let ring_carrier = (2.0 * std::f32::consts::PI * self.ring_phase).sin();
            self.ring_phase += ring_step;
            if self.ring_phase >= 1.0 {
                self.ring_phase -= 1.0;
            }

            (voice_input * (1.0 - self.ring_intensity * 0.4) + (voice_input * ring_carrier) * self.ring_intensity * 1.2).tanh()
        } else {
            voice_input
        };

        // Combine base vocoder or dry signal with ring modulator
        let base_signal = (1.0 - self.mix) * voice_input + self.mix * vocoded;
        if self.ring_intensity > 0.01 {
            (1.0 - self.ring_intensity) * base_signal + self.ring_intensity * ring_out
        } else {
            base_signal
        }
    }

    pub fn reset(&mut self) {
        for f in &mut self.voice_filters {
            f.reset();
        }
        for f in &mut self.carrier_filters {
            f.reset();
        }
        self.envelopes.fill(0.0);
        self.carrier_phase1 = 0.0;
        self.carrier_phase2 = 0.0;
        self.ring_phase = 0.0;
    }
}
