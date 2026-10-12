//! Voice Preset Model, Storage Manager, and Parameter Synthesis.
//! Manages presets as data in ~/.config/micstudio/presets.toml and verification status in verified.json.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn default_category() -> String {
    "Natural".to_string()
}

fn default_bitcrush() -> u32 {
    24
}

fn default_sample_rate() -> u32 {
    48000
}

fn default_flanger_rate() -> f32 {
    0.5
}

fn default_delay_mix() -> f32 {
    1.0
}

fn default_hpf() -> f32 {
    20.0
}

fn default_lpf() -> f32 {
    20000.0
}

fn default_eq_freq() -> f32 {
    2500.0
}

fn default_eq_q() -> f32 {
    1.0
}

fn default_preset_attenuation() -> f32 {
    45.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub description: String,
    #[serde(default = "default_category")]
    pub category: String, // Natural, Character, Robot/Vocoder, Radio/Space, Experimental

    // DPDFNet Neural Noise Suppression Attenuation Limit (dB) tailored per voice preset
    #[serde(default = "default_preset_attenuation")]
    pub dpdfnet_attenuation_db: f32,

    // 1. Pitch & Formant Shifter (RubberBand Mono)
    #[serde(default)]
    pub pitch_semitones: f32, // -12.0 to +12.0
    #[serde(default)]
    pub formant_preserving: bool,

    // 2. Ring Modulator & Tremolo / Stutter (swh-plugins ringmod_1i1o1l)
    #[serde(default)]
    pub ringmod_depth: f32, // 0.0=none, 1.0=AM tremolo/stutter, 2.0=RM ringmod
    #[serde(default)]
    pub ringmod_freq: f32, // 1.0 to 1000.0 Hz
    #[serde(default)]
    pub ringmod_square: f32, // 0.0 or 1.0 (for square-wave chopping stutter)

    // 3. Bitcrusher & Sample Rate Decimator (swh-plugins decimator)
    #[serde(default = "default_bitcrush")]
    pub bitcrush_bits: u32, // 1 to 24 (24 = off)
    #[serde(default = "default_sample_rate")]
    pub downsample_rate: u32, // 1000 to 48000 Hz (48000 = off)

    // 4. Valve Distortion / Saturation (swh-plugins valve)
    #[serde(default)]
    pub distortion_level: f32, // 0.0 to 1.0 (0 = off)

    // 5. Flanger / Chorus Modulation (LSP LADSPA)
    #[serde(default)]
    pub flanger_enabled: bool,
    #[serde(default = "default_flanger_rate")]
    pub flanger_rate: f32,

    // 6. Echo / Reverb Delayorama (swh-plugins delayorama)
    #[serde(default = "default_delay_mix")]
    pub delay_mix: f32, // 1.0 = 100% dry/off, <1.0 = wet
    #[serde(default)]
    pub delay_time: f32, // 0.0 to 2.0 s
    #[serde(default)]
    pub delay_feedback: f32, // 0.0 to 100.0 %

    // 7. Bandpass & Presence Shaping (builtin biquad filters)
    #[serde(default = "default_hpf")]
    pub hpf_freq: f32, // 20.0 to 20000.0 Hz
    #[serde(default = "default_lpf")]
    pub lpf_freq: f32, // 20.0 to 20000.0 Hz
    #[serde(default = "default_eq_freq")]
    pub eq_freq: f32, // 20.0 to 20000.0 Hz
    #[serde(default)]
    pub eq_gain: f32, // -24.0 to +12.0 dB
    #[serde(default = "default_eq_q")]
    pub eq_q: f32,

    // Master FX Mix
    #[serde(default)]
    pub wet_dry_mix: f32, // 0.0 = 100% wet, 1.0 = 100% dry

    // Verification state
    #[serde(default)]
    pub verified: bool,

    #[serde(default)]
    pub user: bool,

    // Legacy fields preserved for backward compatibility
    #[serde(default)]
    pub vocoder_mix: f32,
    #[serde(default)]
    pub vocoder_carrier_hz: f32,
    #[serde(default)]
    pub vocoder_detune: f32,
    #[serde(default)]
    pub matrix_ring_mod: f32,
    #[serde(default)]
    pub bandpass_hpf: f32,
    #[serde(default)]
    pub bandpass_lpf: f32,
    #[serde(default)]
    pub bitcrush_downsample: u32,
    #[serde(default)]
    pub stutter_rate: f32,
    #[serde(default)]
    pub stutter_duty: f32,
    #[serde(default)]
    pub noise_gate_aggro: f32,
    #[serde(default)]
    pub eq_enabled: bool,
    #[serde(default)]
    pub eq_gains: [f32; 9],
    #[serde(default)]
    pub eq_post_gain: f32,
    #[serde(default)]
    pub compressor_enabled: bool,
    #[serde(default)]
    pub limiter_enabled: bool,
    #[serde(default = "default_noise_suppression")]
    pub noise_suppression: bool,
}

fn default_noise_suppression() -> bool {
    true
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: "Natural Clean".to_string(),
            description: "Clean broadcast voice with DPDFNet neural denoising, 9-band EQ, compressor & limiter".to_string(),
            category: "Natural".to_string(),
            dpdfnet_attenuation_db: 45.0,
            pitch_semitones: 0.0,
            formant_preserving: false,
            ringmod_depth: 0.0,
            ringmod_freq: 100.0,
            ringmod_square: 0.0,
            bitcrush_bits: 24,
            downsample_rate: 48000,
            distortion_level: 0.0,
            flanger_enabled: false,
            flanger_rate: 0.5,
            delay_mix: 1.0,
            delay_time: 0.0,
            delay_feedback: 0.0,
            hpf_freq: 20.0,
            lpf_freq: 20000.0,
            eq_freq: 2500.0,
            eq_gain: 0.0,
            eq_q: 1.0,
            wet_dry_mix: 0.0,
            verified: false,
            user: false,
            vocoder_mix: 0.0,
            vocoder_carrier_hz: 110.0,
            vocoder_detune: 0.0,
            matrix_ring_mod: 0.0,
            bandpass_hpf: 0.0,
            bandpass_lpf: 0.0,
            bitcrush_downsample: 1,
            stutter_rate: 0.0,
            stutter_duty: 0.5,
            noise_gate_aggro: 0.0,
            eq_enabled: false,
            eq_gains: [0.0; 9],
            eq_post_gain: 0.0,
            compressor_enabled: false,
            limiter_enabled: true,
            noise_suppression: true,
        }
    }
}

impl Preset {
    /// Generate the live pw-cli Props payload string to apply this preset to capture.ghelper-audio-fx
    pub fn build_props_payload(&self) -> String {
        let pitch_mix = if self.pitch_semitones.abs() < 0.01 && !self.formant_preserving {
            1.0 // dry
        } else {
            0.0 // 100% wet
        };
        let formant_val = if self.formant_preserving { 1.0 } else { 0.0 };
        let flanger_bypass = if self.flanger_enabled { 0.0 } else { 1.0 };
        let flanger_rate = if self.flanger_rate > 0.01 { self.flanger_rate } else { 0.5 };
        let bitcrush_bits = if self.bitcrush_bits >= 1 && self.bitcrush_bits <= 24 { self.bitcrush_bits as f32 } else { 24.0 };
        let downsample = if self.downsample_rate >= 1000 && self.downsample_rate <= 48000 { self.downsample_rate as f32 } else { 48000.0 };
        let square_level = if self.ringmod_square > 0.01 { 1.0 } else { 0.0 };
        let sine_level = if square_level > 0.5 { 0.0 } else { 1.0 };

        format!(
            "{{ params = [ \
                \"pitch_shifter:Semitones\" {:.1}, \
                \"pitch_shifter:Formant Preserving\" {:.1}, \
                \"pitch_shifter:Wet-Dry Mix\" {:.1}, \
                \"ringmod:Modulation depth (0=none, 1=AM, 2=RM)\" {:.2}, \
                \"ringmod:Frequency (Hz)\" {:.1}, \
                \"ringmod:Sine level\" {:.1}, \
                \"ringmod:Square level\" {:.1}, \
                \"decimator:Bit depth\" {:.1}, \
                \"decimator:Sample rate (Hz)\" {:.1}, \
                \"flanger:Bypass\" {:.1}, \
                \"flanger:Rate (Hz)\" {:.2}, \
                \"delay:Dry/wet mix\" {:.2}, \
                \"delay:First delay (s)\" {:.3}, \
                \"delay:Feedback (%)\" {:.1}, \
                \"eq_hpf:Freq\" {:.1}, \
                \"eq_lpf:Freq\" {:.1}, \
                \"eq_peak:Freq\" {:.1}, \
                \"eq_peak:Gain\" {:.1} \
            ] }}",
            self.pitch_semitones,
            formant_val,
            pitch_mix,
            self.ringmod_depth,
            self.ringmod_freq,
            sine_level,
            square_level,
            bitcrush_bits,
            downsample,
            flanger_bypass,
            flanger_rate,
            self.delay_mix,
            self.delay_time,
            self.delay_feedback,
            self.hpf_freq,
            self.lpf_freq,
            self.eq_freq,
            self.eq_gain
        )
    }

    /// Return only the active, nonzero parameters for clean display in the TUI
    pub fn nonzero_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();

        if self.pitch_semitones.abs() > 0.01 {
            let label = if self.formant_preserving {
                format!("{:+.*} st (Formant Preserved)", 1, self.pitch_semitones)
            } else {
                format!("{:+.*} st", 1, self.pitch_semitones)
            };
            params.push(("Pitch Shift".to_string(), label));
        }

        if self.ringmod_depth > 0.01 {
            let desc = if self.ringmod_square > 0.5 {
                format!("{:.1} Hz AM Square (Stutter)", self.ringmod_freq)
            } else if self.ringmod_depth >= 1.5 {
                format!("RM @ {:.0} Hz (Metallic)", self.ringmod_freq)
            } else {
                format!("AM @ {:.0} Hz (Tremolo)", self.ringmod_freq)
            };
            params.push(("Modulation".to_string(), desc));
        }

        if self.bitcrush_bits < 24 || self.downsample_rate < 48000 {
            params.push((
                "Lo-Fi / Crush".to_string(),
                format!("{}-bit ({} Hz)", self.bitcrush_bits, self.downsample_rate),
            ));
        }

        if self.distortion_level > 0.01 {
            params.push((
                "Valve Drive".to_string(),
                format!("{:.0}%", self.distortion_level * 100.0),
            ));
        }

        if self.flanger_enabled {
            params.push((
                "Flanger".to_string(),
                format!("{:.2} Hz Space LFO", self.flanger_rate),
            ));
        }

        if self.delay_mix < 0.99 {
            params.push((
                "Echo / Delay".to_string(),
                format!(
                    "{:.0}ms ({:.0}% wet, {:.0}% fb)",
                    self.delay_time * 1000.0,
                    (1.0 - self.delay_mix) * 100.0,
                    self.delay_feedback
                ),
            ));
        }

        if self.hpf_freq > 25.0 || self.lpf_freq < 19000.0 {
            params.push((
                "Bandpass Filter".to_string(),
                format!("{:.0} Hz – {:.0} Hz", self.hpf_freq, self.lpf_freq),
            ));
        }

        if self.eq_gain.abs() > 0.1 {
            params.push((
                "Presence EQ".to_string(),
                format!("{:+.1} dB @ {:.0} Hz", self.eq_gain, self.eq_freq),
            ));
        }

        if self.wet_dry_mix > 0.01 {
            params.push((
                "Effect Mix".to_string(),
                format!("{:.0}% Wet", (1.0 - self.wet_dry_mix) * 100.0),
            ));
        }

        if (self.dpdfnet_attenuation_db - 45.0).abs() > 0.1 {
            params.push((
                "DPDFNet Attn".to_string(),
                format!("{:.0} dB", self.dpdfnet_attenuation_db),
            ));
        }

        params
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub schema_version: u32,
    pub active_preset: String,
    #[serde(default = "PresetConfig::default_factory_presets")]
    pub presets: Vec<Preset>,
}

impl Default for PresetConfig {
    fn default() -> Self {
        Self {
            schema_version: 2,
            active_preset: "Natural Clean".to_string(),
            presets: Self::default_factory_presets(),
        }
    }
}

impl PresetConfig {
    pub fn default_factory_presets() -> Vec<Preset> {
        vec![
            // ==================== NATURAL ====================
            Preset {
                name: "Natural Clean".to_string(),
                category: "Natural".to_string(),
                description: "Clean broadcast voice with DPDFNet neural denoising, 9-band EQ, compressor & limiter".to_string(),
                dpdfnet_attenuation_db: 45.0,
                ..Default::default()
            },
            Preset {
                name: "Deep Voice".to_string(),
                category: "Natural".to_string(),
                description: "Natural warm radio announcer voice with formant preservation (-4.5 st)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: -4.5,
                formant_preserving: true,
                eq_freq: 200.0,
                eq_gain: 2.0,
                ..Default::default()
            },
            Preset {
                name: "Radio Announcer".to_string(),
                category: "Natural".to_string(),
                description: "Warm resonant broadcast studio tone with slight tube body (-2.0 st)".to_string(),
                dpdfnet_attenuation_db: 45.0,
                pitch_semitones: -2.0,
                formant_preserving: true,
                distortion_level: 0.08,
                eq_freq: 2500.0,
                eq_gain: 3.0,
                ..Default::default()
            },
            Preset {
                name: "Higher Voice".to_string(),
                category: "Natural".to_string(),
                description: "Natural pitch elevation with preserved vocal formants (+3.5 st)".to_string(),
                dpdfnet_attenuation_db: 45.0,
                pitch_semitones: 3.5,
                formant_preserving: true,
                ..Default::default()
            },
            Preset {
                name: "Gruff Growl".to_string(),
                category: "Natural".to_string(),
                description: "Deep gravelly baritone with aggressive harmonic tube saturation (-4.0 st)".to_string(),
                dpdfnet_attenuation_db: 50.0,
                pitch_semitones: -4.0,
                formant_preserving: false,
                distortion_level: 0.25,
                eq_freq: 180.0,
                eq_gain: 3.5,
                ..Default::default()
            },

            // ==================== CHARACTER ====================
            Preset {
                name: "Chipmunk".to_string(),
                category: "Character".to_string(),
                description: "High-pitched animated cartoon character voice (+8.0 st)".to_string(),
                dpdfnet_attenuation_db: 55.0,
                pitch_semitones: 8.0,
                formant_preserving: false,
                ..Default::default()
            },
            Preset {
                name: "Darth Vader".to_string(),
                category: "Character".to_string(),
                description: "Deep villain voice (-6.0 st) with heavy presence and subtle metallic breath".to_string(),
                dpdfnet_attenuation_db: 65.0,
                pitch_semitones: -6.0,
                formant_preserving: false,
                ringmod_depth: 0.12,
                ringmod_freq: 35.0,
                distortion_level: 0.12,
                eq_freq: 120.0,
                eq_gain: 4.0,
                ..Default::default()
            },
            Preset {
                name: "Megatron".to_string(),
                category: "Character".to_string(),
                description: "Robotic overlord (-5.0 st) with aggressive ring modulation and 14-bit crunch".to_string(),
                dpdfnet_attenuation_db: 75.0,
                pitch_semitones: -5.0,
                ringmod_depth: 0.55,
                ringmod_freq: 68.0,
                bitcrush_bits: 14,
                distortion_level: 0.20,
                ..Default::default()
            },
            Preset {
                name: "Giant".to_string(),
                category: "Character".to_string(),
                description: "Enormous subterranean behemoth (-8.5 st) with echoing cavern resonance".to_string(),
                dpdfnet_attenuation_db: 55.0,
                pitch_semitones: -8.5,
                formant_preserving: false,
                delay_mix: 0.35,
                delay_time: 0.12,
                delay_feedback: 35.0,
                eq_freq: 100.0,
                eq_gain: 5.0,
                ..Default::default()
            },
            Preset {
                name: "Goblin".to_string(),
                category: "Character".to_string(),
                description: "Shrieking mischievous goblin (+6.0 st) with harsh nasal saturation".to_string(),
                dpdfnet_attenuation_db: 60.0,
                pitch_semitones: 6.0,
                ringmod_depth: 0.20,
                ringmod_freq: 180.0,
                distortion_level: 0.22,
                eq_freq: 3200.0,
                eq_gain: 4.0,
                ..Default::default()
            },
            Preset {
                name: "Child".to_string(),
                category: "Character".to_string(),
                description: "Believable youthful child voice with natural formant preservation (+4.5 st)".to_string(),
                dpdfnet_attenuation_db: 50.0,
                pitch_semitones: 4.5,
                formant_preserving: true,
                eq_freq: 2800.0,
                eq_gain: 2.0,
                ..Default::default()
            },

            // ==================== ROBOT / VOCODER ====================
            Preset {
                name: "Robot".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Resonant metallic ring-modulated robot voice (-2.0 st, 110 Hz RM)".to_string(),
                dpdfnet_attenuation_db: 85.0,
                pitch_semitones: -2.0,
                ringmod_depth: 1.0,
                ringmod_freq: 110.0,
                ..Default::default()
            },
            Preset {
                name: "Cylon".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Deep mechanical Centurion vocoder (-4.0 st, 85 Hz carrier, short delay)".to_string(),
                dpdfnet_attenuation_db: 85.0,
                pitch_semitones: -4.0,
                ringmod_depth: 0.85,
                ringmod_freq: 85.0,
                delay_mix: 0.25,
                delay_time: 0.06,
                distortion_level: 0.15,
                ..Default::default()
            },
            Preset {
                name: "Daft Punk".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "French electro vocoder synthesis (125 Hz carrier, 14-bit talkbox)".to_string(),
                dpdfnet_attenuation_db: 90.0,
                pitch_semitones: 1.0,
                ringmod_depth: 0.90,
                ringmod_freq: 125.0,
                bitcrush_bits: 14,
                distortion_level: 0.10,
                ..Default::default()
            },
            Preset {
                name: "Kraftwerk".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Vintage restrained electronic synth voice (145 Hz modulation, lowpass cut)".to_string(),
                dpdfnet_attenuation_db: 85.0,
                pitch_semitones: 0.0,
                ringmod_depth: 0.70,
                ringmod_freq: 145.0,
                lpf_freq: 5500.0,
                ..Default::default()
            },
            Preset {
                name: "Dalek".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Iconic BBC Doctor Who Dalek ring modulation (+1.5 st, 30 Hz RM)".to_string(),
                dpdfnet_attenuation_db: 85.0,
                pitch_semitones: 1.5,
                ringmod_depth: 1.0,
                ringmod_freq: 30.0,
                distortion_level: 0.25,
                ..Default::default()
            },

            // ==================== RADIO / SPACE ====================
            Preset {
                name: "Walkie Talkie".to_string(),
                category: "Radio/Space".to_string(),
                description: "Bandpass filtered military comms (450–3200 Hz) with 12-bit lo-fi crunch".to_string(),
                dpdfnet_attenuation_db: 60.0,
                hpf_freq: 450.0,
                lpf_freq: 3200.0,
                bitcrush_bits: 12,
                downsample_rate: 8000,
                distortion_level: 0.20,
                eq_freq: 1800.0,
                eq_gain: 3.0,
                ..Default::default()
            },
            Preset {
                name: "Old Telephone".to_string(),
                category: "Radio/Space".to_string(),
                description: "Vintage carbon-microphone telephone line (300–2800 Hz, 10-bit downsampled)".to_string(),
                dpdfnet_attenuation_db: 60.0,
                hpf_freq: 300.0,
                lpf_freq: 2800.0,
                bitcrush_bits: 10,
                downsample_rate: 6000,
                distortion_level: 0.15,
                ..Default::default()
            },
            Preset {
                name: "Megaphone".to_string(),
                category: "Radio/Space".to_string(),
                description: "Acoustic public horn bullhorn (600–4000 Hz) with harsh horn saturation".to_string(),
                dpdfnet_attenuation_db: 65.0,
                hpf_freq: 600.0,
                lpf_freq: 4000.0,
                distortion_level: 0.35,
                eq_freq: 2200.0,
                eq_gain: 6.0,
                ..Default::default()
            },
            Preset {
                name: "Cave".to_string(),
                category: "Radio/Space".to_string(),
                description: "Deep subterranean cavern echo (150ms pre-delay, 45% feedback)".to_string(),
                dpdfnet_attenuation_db: 55.0,
                delay_mix: 0.45,
                delay_time: 0.15,
                delay_feedback: 45.0,
                lpf_freq: 7000.0,
                ..Default::default()
            },
            Preset {
                name: "Stadium Echo".to_string(),
                category: "Radio/Space".to_string(),
                description: "Massive public arena PA system echo (280ms slap, 55% feedback)".to_string(),
                dpdfnet_attenuation_db: 55.0,
                delay_mix: 0.40,
                delay_time: 0.28,
                delay_feedback: 55.0,
                eq_freq: 3000.0,
                eq_gain: 3.0,
                ..Default::default()
            },

            // ==================== EXPERIMENTAL ====================
            Preset {
                name: "Sci-Fi Alien".to_string(),
                category: "Experimental".to_string(),
                description: "Harmonic extraterrestrial voice (+5.0 st, 220 Hz RM, flanger modulation)".to_string(),
                dpdfnet_attenuation_db: 65.0,
                pitch_semitones: 5.0,
                ringmod_depth: 0.65,
                ringmod_freq: 220.0,
                flanger_enabled: true,
                flanger_rate: 0.8,
                ..Default::default()
            },
            Preset {
                name: "Ghost".to_string(),
                category: "Experimental".to_string(),
                description: "Ethereal spectral whisper (+2.0 st, long ambient feedback delay, flanger)".to_string(),
                dpdfnet_attenuation_db: 55.0,
                pitch_semitones: 2.0,
                delay_mix: 0.50,
                delay_time: 0.20,
                delay_feedback: 60.0,
                flanger_enabled: true,
                flanger_rate: 0.3,
                ..Default::default()
            },
            Preset {
                name: "Demon".to_string(),
                category: "Experimental".to_string(),
                description: "Abyssal demonic entity (-7.0 st, 40 Hz growl, heavy valve distortion)".to_string(),
                dpdfnet_attenuation_db: 70.0,
                pitch_semitones: -7.0,
                ringmod_depth: 0.30,
                ringmod_freq: 40.0,
                distortion_level: 0.40,
                eq_freq: 120.0,
                eq_gain: 5.0,
                ..Default::default()
            },
            Preset {
                name: "Cyber Chopper".to_string(),
                category: "Experimental".to_string(),
                description: "Rhythmic gated cyber voice (8 Hz square AM stutter, -2.0 st pitch)".to_string(),
                dpdfnet_attenuation_db: 80.0,
                pitch_semitones: -2.0,
                ringmod_depth: 1.0,
                ringmod_freq: 8.0,
                ringmod_square: 1.0,
                ..Default::default()
            },
            Preset {
                name: "Underwater".to_string(),
                category: "Experimental".to_string(),
                description: "Submerged acoustic immersion (750 Hz lowpass, liquid flanger chorus)".to_string(),
                dpdfnet_attenuation_db: 50.0,
                lpf_freq: 750.0,
                flanger_enabled: true,
                flanger_rate: 0.4,
                eq_freq: 150.0,
                eq_gain: 4.0,
                ..Default::default()
            },
            Preset {
                name: "Lo-Fi Static".to_string(),
                category: "Experimental".to_string(),
                description: "Extreme 8-bit retro arcade degradation (4000 Hz sample rate, valve crunch)".to_string(),
                dpdfnet_attenuation_db: 65.0,
                bitcrush_bits: 8,
                downsample_rate: 4000,
                distortion_level: 0.30,
                hpf_freq: 250.0,
                lpf_freq: 4500.0,
                ..Default::default()
            },
        ]
    }
}

pub struct PresetManager {
    config_dir: PathBuf,
    config: PresetConfig,
}

impl PresetManager {
    pub fn new() -> Result<Self> {
        let home = std::env::var("HOME").context("HOME not set")?;
        let config_dir = Path::new(&home).join(".config").join("micstudio");
        let mut mgr = Self {
            config_dir,
            config: PresetConfig::default(),
        };
        mgr.load()?;
        Ok(mgr)
    }

    pub fn all_presets(&self) -> &[Preset] {
        &self.config.presets
    }

    pub fn factory_presets(&self) -> Vec<Preset> {
        self.config.presets.iter().filter(|p| !p.user).cloned().collect()
    }

    pub fn user_presets(&self) -> Vec<Preset> {
        self.config.presets.iter().filter(|p| p.user).cloned().collect()
    }

    pub fn save_user_preset(&mut self, mut preset: Preset) -> Result<()> {
        preset.user = true;
        self.config.presets.retain(|p| !p.name.eq_ignore_ascii_case(&preset.name));
        self.config.presets.push(preset);
        self.save()
    }

    pub fn delete_user_preset(&mut self, name: &str) -> Result<bool> {
        let before = self.config.presets.len();
        self.config.presets.retain(|p| !(p.user && p.name.eq_ignore_ascii_case(name)));
        let deleted = self.config.presets.len() < before;
        if deleted {
            self.save()?;
        }
        Ok(deleted)
    }

    pub fn find_preset(&self, name: &str) -> Option<Preset> {
        let name_lower = name.trim().to_lowercase();
        self.config
            .presets
            .iter()
            .find(|p| p.name.to_lowercase() == name_lower)
            .cloned()
    }

    pub fn active_preset_name(&self) -> &str {
        &self.config.active_preset
    }

    pub fn set_active_preset(&mut self, name: &str) -> Result<()> {
        self.config.active_preset = name.to_string();
        self.save()
    }

    fn config_path(&self) -> PathBuf {
        self.config_dir.join("presets.toml")
    }

    fn verified_path(&self) -> PathBuf {
        self.config_dir.join("verified.json")
    }

    pub fn load_verified_set(&self) -> HashSet<String> {
        let path = self.verified_path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(set) = serde_json::from_str::<HashSet<String>>(&data) {
                return set;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(arr) = val.get("verified_presets").and_then(|v| v.as_array()) {
                    return arr.iter().filter_map(|s| s.as_str().map(String::from)).collect();
                }
            }
        }
        HashSet::new()
    }

    pub fn mark_verified(&mut self, preset_name: &str) -> Result<()> {
        let mut set = self.load_verified_set();
        set.insert(preset_name.to_string());
        let list: Vec<String> = set.iter().cloned().collect();
        let obj = serde_json::json!({
            "verified_presets": list,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64(),
            "source": "live_real_mic"
        });
        fs::write(self.verified_path(), serde_json::to_string_pretty(&obj)?)?;

        for p in &mut self.config.presets {
            if p.name.eq_ignore_ascii_case(preset_name) {
                p.verified = true;
            }
        }
        self.save()?;
        Ok(())
    }

    fn load(&mut self) -> Result<()> {
        let path = self.config_path();
        if path.exists() {
            let data = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            if let Ok(mut parsed) = toml::from_str::<PresetConfig>(&data) {
                // If loaded config has fewer than 20 presets, ensure all defaults are present
                if parsed.presets.len() < 20 {
                    let mut existing: HashSet<String> = parsed.presets.iter().map(|p| p.name.to_lowercase()).collect();
                    for def in PresetConfig::default_factory_presets() {
                        if !existing.contains(&def.name.to_lowercase()) {
                            parsed.presets.push(def.clone());
                            existing.insert(def.name.to_lowercase());
                        }
                    }
                }
                for def in PresetConfig::default_factory_presets() {
                    if let Some(p) = parsed.presets.iter_mut().find(|p| p.name.eq_ignore_ascii_case(&def.name) && !p.user) {
                        p.dpdfnet_attenuation_db = def.dpdfnet_attenuation_db;
                    }
                }
                let verified_set = self.load_verified_set();
                for p in &mut parsed.presets {
                    if verified_set.contains(&p.name) {
                        p.verified = true;
                    }
                }
                self.config = parsed;
                let _ = self.save();
                return Ok(());
            }
        }

        self.config = PresetConfig::default();
        let verified_set = self.load_verified_set();
        for p in &mut self.config.presets {
            if verified_set.contains(&p.name) {
                p.verified = true;
            }
        }
        self.save()?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let data = toml::to_string_pretty(&self.config)?;
        let path = self.config_path();
        let tmp_path = self.config_dir.join("presets.toml.tmp");
        fs::write(&tmp_path, data)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }
}
