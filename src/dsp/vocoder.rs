//! 16-Band Vocoder Filterbank and Carrier Synthesizer with Matrix Ring-Modulation.
//! High-intelligibility robotic speech synthesis with syllabic envelope tracking.

use crate::dsp::biquad::{Biquad, FilterType};

pub const NUM_VOCODER_BANDS: usize = 16;
pub const VOCODER_FREQS: [f32; NUM_VOCODER_BANDS] = [
    150.0, 220.0, 320.0, 440.0, 600.0, 800.0, 1050.0, 1350.0,
    1750.0, 2250.0, 2900.0, 3700.0, 4700.0, 5900.0, 7400.0, 9200.0,
];

pub struct Vocoder {
    voice_filters: [Biquad; NUM_VOCODER_BANDS],
    carrier_filters: [Biquad; NUM_VOCODER_BANDS],
    envelopes: [f32; NUM_VOCODER_BANDS],
    
    // Internal dual detuned sawtooth carrier oscillators
    carrier_phase1: f32,
    carrier_phase2: f32,
    carrier_freq: f32,
    detune: f32, // Hz
    
    // Noise generator state for unvoiced consonant articulation
    noise_seed: u32,

    // Matrix ring-mod oscillator (35 - 55 Hz)
    ring_phase: f32,
    ring_intensity: f32, // 0.0 to 1.0
    
    // Envelope follower time constants
    att_coeff: f32,
    rel_coeff: f32,

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
            vf[i].configure(FilterType::BandPass, VOCODER_FREQS[i], 3.5, 0.0, sample_rate);
            cf[i].configure(FilterType::BandPass, VOCODER_FREQS[i], 3.5, 0.0, sample_rate);
        }

        // Syllabic envelope: 6 ms attack, 25 ms release
        let att_coeff = 1.0 - (-1.0 / (0.006 * sample_rate)).exp();
        let rel_coeff = 1.0 - (-1.0 / (0.025 * sample_rate)).exp();

        Self {
            voice_filters: vf,
            carrier_filters: cf,
            envelopes: [0.0; NUM_VOCODER_BANDS],
            carrier_phase1: 0.0,
            carrier_phase2: 0.0,
            carrier_freq: 110.0,
            detune: 1.5,
            noise_seed: 0x12345678,
            ring_phase: 0.0,
            ring_intensity: 0.0,
            att_coeff,
            rel_coeff,
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
        self.enabled = self.mix > 0.02 || self.ring_intensity > 0.02;
    }

    #[inline(always)]
    fn next_noise(&mut self) -> f32 {
        self.noise_seed = self.noise_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.noise_seed >> 9) as f32) / 8388608.0 - 1.0
    }

    #[inline(always)]
    pub fn process(&mut self, voice_input: f32) -> f32 {
        if !self.enabled {
            return voice_input;
        }

        let mut out = voice_input;

        // 1. Channel Vocoder Synthesis
        if self.mix > 0.02 {
            let step1 = self.carrier_freq / self.sample_rate;
            let step2 = (self.carrier_freq + self.detune) / self.sample_rate;

            let saw1 = 2.0 * self.carrier_phase1 - 1.0;
            let saw2 = 2.0 * self.carrier_phase2 - 1.0;
            let noise = self.next_noise();

            if voice_input.abs() > 1e-4 {
                self.carrier_phase1 += step1;
                if self.carrier_phase1 >= 1.0 {
                    self.carrier_phase1 -= 1.0;
                }

                self.carrier_phase2 += step2;
                if self.carrier_phase2 >= 1.0 {
                    self.carrier_phase2 -= 1.0;
                }
            }

            let mut vocoded_sum = 0.0f32;

            for i in 0..NUM_VOCODER_BANDS {
                // Analysis: Filter voice input into frequency band
                let v_band = self.voice_filters[i].process(voice_input);

                // Envelope follower with syllabic attack/release
                let v_mag = v_band.abs();
                if v_mag > self.envelopes[i] {
                    self.envelopes[i] += self.att_coeff * (v_mag - self.envelopes[i]);
                } else {
                    self.envelopes[i] += self.rel_coeff * (v_mag - self.envelopes[i]);
                }

                // Synthesis: Build carrier. For high bands (> 3.5 kHz), blend in subtle noise
                // so consonants ('s', 't', 'k') are crisp and articulate.
                let raw_carrier = if VOCODER_FREQS[i] > 3500.0 {
                    0.35 * saw1 + 0.35 * saw2 + 0.30 * noise
                } else {
                    0.50 * (saw1 + saw2)
                };

                let c_band = self.carrier_filters[i].process(raw_carrier);
                vocoded_sum += c_band * self.envelopes[i];
            }

            // Normalization makeup gain to match voice level
            let vocoded_norm = vocoded_sum * 12.0;
            out = (1.0 - self.mix) * out + self.mix * vocoded_norm;
        }

        // 2. Matrix Ring Modulation (Harmonic villain / robotic sidebands)
        if self.ring_intensity > 0.02 {
            let ring_step = 42.0 / self.sample_rate;
            let ring_carrier = (2.0 * std::f32::consts::PI * self.ring_phase).sin();
            if voice_input.abs() > 1e-4 {
                self.ring_phase += ring_step;
                if self.ring_phase >= 1.0 {
                    self.ring_phase -= 1.0;
                }
            }

            let modulated = out * ring_carrier;
            // Clean sideband blend with soft saturation
            out = (1.0 - self.ring_intensity) * out + self.ring_intensity * (modulated * 1.5);
        }

        out
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
