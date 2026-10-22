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
    0.0
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

fn default_model_gain() -> f32 {
    2.0
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

    // Master FX Output Gain / Model Volume Boost (dB, -12.0 to +12.0 dB, default +2.0 dB for creative models, 0.0 for Clean)
    #[serde(default = "default_model_gain")]
    pub model_gain_db: f32,

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
    pub delay_mix: f32, // 0.0 = 100% dry/off, >0.0 = wet echo
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
            model_gain_db: 0.0,
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
            delay_mix: 0.0,
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
    /// Generate the live pw-cli Props payload string to apply this preset to capture.ghelper-audio
    pub fn build_props_payload(&self) -> String {
        let clean_cfg = crate::config::CleanMicSettings::load();
        self.build_props_payload_with_clean(&clean_cfg)
    }

    /// Generate the live pw-cli Props payload with explicit CleanMicSettings
    pub fn build_props_payload_with_clean(&self, clean_cfg: &crate::config::CleanMicSettings) -> String {
        let pitch_mix = if self.pitch_semitones.abs() < 0.01 && !self.formant_preserving {
            1.0 // dry
        } else {
            0.0 // 100% wet
        };
        let formant_val = if self.formant_preserving { 1.0 } else { 0.0 };
        let flanger_bypass = if self.flanger_enabled { 0.0 } else { 1.0 };
        let flanger_rate = if self.flanger_rate > 0.01 {
            self.flanger_rate
        } else {
            0.5
        };
        let bitcrush_bits = if self.bitcrush_bits >= 1 && self.bitcrush_bits <= 24 {
            self.bitcrush_bits as f32
        } else {
            24.0
        };
        let downsample = if self.downsample_rate >= 1000 && self.downsample_rate <= 48000 {
            self.downsample_rate as f32
        } else {
            48000.0
        };
        let square_level = if self.ringmod_square > 0.01 { 1.0 } else { 0.0 };
        let sine_level = if square_level > 0.5 { 0.0 } else { 1.0 };

        let is_clean = self.name.eq_ignore_ascii_case("Natural Clean");

        // Clean Broadcast EQ cascade: active on Natural Clean, flat 0.0 dB on creative presets
        let (eq_80_gain, eq_400_gain, eq_3k5_gain) = if is_clean {
            (-2.0f32, -1.5f32, 1.5f32)
        } else {
            (0.0f32, 0.0f32, 0.0f32)
        };

        // Anti-Shout Compressor settings:
        // 5ms attack, 120ms hold, 500ms release, soft knee
        let (thresh_linear, makeup_linear, ratio) = if is_clean {
            if clean_cfg.quiet_voice_boost_enabled && clean_cfg.quiet_voice_boost_db > 0.01 {
                let boost = clean_cfg.quiet_voice_boost_db.clamp(0.0, 10.0);
                let thresh_db = (-12.0 - (boost * 1.5)).clamp(-24.0, -12.0);
                let makeup_db = (3.0 + boost).clamp(3.0, 12.0);
                (
                    10.0f32.powf(thresh_db / 20.0),
                    10.0f32.powf(makeup_db / 20.0),
                    2.0f32,
                )
            } else {
                (0.251189f32, 1.412538f32, 2.0f32) // -12 dBFS threshold, +3 dB makeup
            }
        } else {
            // Creative models: quiet voice boost is strictly isolated to clean!
            // Baseline 0 dB makeup gain, model boost handled at limiter
            (0.251189f32, 1.000000f32, 2.5f32)
        };

        // Apply model and user boost before limiting; post-limiter gain stays at unity.
        let user_gain_linear = 10.0f32.powf(clean_cfg.output_gain_db / 20.0);
        let model_gain_linear = if is_clean {
            user_gain_linear
        } else {
            10.0f32.powf(self.model_gain_db / 20.0) * user_gain_linear
        };

        format!(
            "{{ params = [ \
                \"dpdfnet:Attenuation Limit (dB)\" {:.1}, \
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
                \"eq_peak:Gain\" {:.1}, \
                \"eq_80:Gain\" {:.1}, \
                \"eq_400:Gain\" {:.1}, \
                \"eq_3k5:Gain\" {:.1}, \
                \"compressor:Attack threshold (G)\" {:.6}, \
                \"compressor:Makeup gain (G)\" {:.6}, \
                \"compressor:Ratio\" {:.1}, \
                \"compressor:Attack time (ms)\" 5.0, \
                \"compressor:Hold time (ms)\" 120.0, \
                \"compressor:Release time (ms)\" 500.0, \
                \"limiter:Input gain (G)\" {:.4}, \"limiter:Output gain (G)\" 1.0000 \
            ] }}",
            self.dpdfnet_attenuation_db,
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
            self.eq_gain,
            eq_80_gain,
            eq_400_gain,
            eq_3k5_gain,
            thresh_linear,
            makeup_linear,
            ratio,
            model_gain_linear
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

        if self.delay_mix > 0.01 {
            params.push((
                "Echo / Delay".to_string(),
                format!(
                    "{:.0}ms ({:.0}% wet, {:.0}% fb)",
                    self.delay_time * 1000.0,
                    self.delay_mix * 100.0,
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

        if self.model_gain_db.abs() > 0.01 {
            params.push((
                "Model Boost".to_string(),
                format!("{:+.1} dB", self.model_gain_db),
            ));
        }

        params
    }

    /// Return ordered active stages summary for UI display
    pub fn stage_pipeline_summary(&self) -> Vec<String> {
        let mut stages = vec![format!("DPDFNet ({:.0}dB)", self.dpdfnet_attenuation_db)];
        if self.pitch_semitones.abs() > 0.01 {
            let label = if self.formant_preserving {
                format!("Pitch ({:+.1}st, Formants)", self.pitch_semitones)
            } else {
                format!("Pitch ({:+.1}st)", self.pitch_semitones)
            };
            stages.push(label);
        }
        if self.ringmod_depth > 0.01 {
            let label = if self.ringmod_square > 0.5 {
                format!("Chopper ({:.0}Hz)", self.ringmod_freq)
            } else {
                format!("Modulation ({:.0}Hz)", self.ringmod_freq)
            };
            stages.push(label);
        }
        if self.bitcrush_bits < 24 || self.downsample_rate < 48000 {
            stages.push(format!(
                "Decimator ({}-bit, {}Hz)",
                self.bitcrush_bits, self.downsample_rate
            ));
        }
        if self.distortion_level > 0.01 {
            stages.push(format!(
                "Tube Drive ({:.0}%)",
                self.distortion_level * 100.0
            ));
        }
        if self.flanger_enabled {
            stages.push(format!("Flanger ({:.1}Hz)", self.flanger_rate));
        }
        if self.delay_mix > 0.01 {
            stages.push(format!("Echo ({:.0}ms)", self.delay_time * 1000.0));
        }
        if self.hpf_freq > 25.0 || self.lpf_freq < 19000.0 {
            stages.push(format!(
                "Bandpass ({:.0}-{:.0}Hz)",
                self.hpf_freq, self.lpf_freq
            ));
        }
        if self.eq_gain.abs() > 0.1 {
            stages.push(format!("Tone EQ ({:+.1}dB)", self.eq_gain));
        }
        if self.model_gain_db > 0.01 {
            stages.push(format!("Boost ({:+.1}dB)", self.model_gain_db));
        }
        stages.push("LSP Limiter (-3dBFS)".to_string());
        stages
    }

    /// Return relevant technical capability notes or architectural limitations
    pub fn limitations_note(&self) -> Option<String> {
        if self.category == "Robot/Vocoder" {
            Some("Carrier generated via in-process modulation (self-carrier; no external carrier stream needed)".to_string())
        } else if self.pitch_semitones.abs() > 0.01 && self.formant_preserving {
            Some("Formant preservation active via RubberBand live phase-vocoder engine".to_string())
        } else if self.name == "Natural Clean" {
            Some(
                "Reference broadcast baseline: pure 6-stage DPDFNet + EQ + Compressor + Limiter"
                    .to_string(),
            )
        } else {
            None
        }
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
        let mut presets = vec![
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
                description: "Natural warm baritone voice with preserved vocal formants (-3.5 st)".to_string(),
                dpdfnet_attenuation_db: 45.0,
                pitch_semitones: -3.5,
                formant_preserving: true,
                hpf_freq: 80.0,
                eq_freq: 2400.0,
                eq_gain: 1.5,
                ..Default::default()
            },
            Preset {
                name: "Radio Announcer".to_string(),
                category: "Natural".to_string(),
                description: "Warm, resonant broadcast studio tone with broadcast EQ & tube warmth".to_string(),
                dpdfnet_attenuation_db: 45.0,
                pitch_semitones: 0.0,
                formant_preserving: true,
                distortion_level: 0.04,
                hpf_freq: 85.0,
                eq_freq: 3200.0,
                eq_gain: 2.5,
                eq_q: 0.8,
                ..Default::default()
            },
            Preset {
                name: "Higher Voice".to_string(),
                category: "Natural".to_string(),
                description: "Natural pitch elevation with preserved vocal formants (+3.0 st)".to_string(),
                dpdfnet_attenuation_db: 45.0,
                pitch_semitones: 3.0,
                formant_preserving: true,
                hpf_freq: 100.0,
                eq_freq: 3500.0,
                eq_gain: 1.5,
                ..Default::default()
            },
            Preset {
                name: "Gruff Growl".to_string(),
                category: "Natural".to_string(),
                description: "Raspy, heavy baritone growl with subharmonic grit and tube drive (-3.0 st)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: -3.0,
                formant_preserving: false,
                ringmod_depth: 0.15,
                ringmod_freq: 48.0,
                distortion_level: 0.14,
                hpf_freq: 90.0,
                eq_freq: 2600.0,
                eq_gain: 2.0,
                eq_q: 1.2,
                ..Default::default()
            },

            // ==================== CHARACTER ====================
            Preset {
                name: "Chipmunk".to_string(),
                category: "Character".to_string(),
                description: "High-pitched animated cartoon character with upward formant shift (+7.0 st)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: 7.0,
                formant_preserving: false,
                hpf_freq: 120.0,
                lpf_freq: 12000.0,
                eq_freq: 2800.0,
                eq_gain: 1.5,
                ..Default::default()
            },
            Preset {
                name: "Darth Vader".to_string(),
                category: "Character".to_string(),
                description: "Deep villain voice (-5.0 st) with acoustic respirator resonance & crisp articulation".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: -5.0,
                formant_preserving: false,
                ringmod_depth: 0.10,
                ringmod_freq: 38.0,
                distortion_level: 0.08,
                hpf_freq: 75.0,
                lpf_freq: 9000.0,
                eq_freq: 2800.0,
                eq_gain: 2.5,
                ..Default::default()
            },
            Preset {
                name: "Megatron".to_string(),
                category: "Character".to_string(),
                description: "Metallic robotic warlord (-3.5 st) with 75 Hz ring mod, comb flanger & 16-bit drive".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: -3.5,
                formant_preserving: false,
                ringmod_depth: 0.35,
                ringmod_freq: 75.0,
                bitcrush_bits: 16,
                distortion_level: 0.12,
                flanger_enabled: true,
                flanger_rate: 0.7,
                eq_freq: 3200.0,
                eq_gain: 2.5,
                eq_q: 1.2,
                ..Default::default()
            },
            Preset {
                name: "Giant".to_string(),
                category: "Character".to_string(),
                description: "Enormous subterranean behemoth (-6.0 st) with short cavern slapback".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: -6.0,
                formant_preserving: false,
                delay_mix: 0.15,
                delay_time: 0.08,
                delay_feedback: 20.0,
                hpf_freq: 70.0,
                eq_freq: 2200.0,
                eq_gain: 2.0,
                ..Default::default()
            },
            Preset {
                name: "Goblin".to_string(),
                category: "Character".to_string(),
                description: "Mischievous nasal goblin (+4.5 st) with 1.6 kHz formant peak & controlled rasp".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: 4.5,
                formant_preserving: false,
                ringmod_depth: 0.12,
                ringmod_freq: 120.0,
                distortion_level: 0.10,
                hpf_freq: 150.0,
                lpf_freq: 7500.0,
                eq_freq: 1600.0,
                eq_gain: 3.0,
                eq_q: 1.5,
                ..Default::default()
            },
            Preset {
                name: "Child".to_string(),
                category: "Character".to_string(),
                description: "Believable youthful voice with natural formant preservation (+2.8 st)".to_string(),
                dpdfnet_attenuation_db: 45.0,
                pitch_semitones: 2.8,
                formant_preserving: true,
                hpf_freq: 110.0,
                eq_freq: 2800.0,
                eq_gain: 1.0,
                ..Default::default()
            },

            // ==================== ROBOT / VOCODER ====================
            Preset {
                name: "Robot".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Synthesized robotic voice with 90 Hz amplitude modulation & focused speech band".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: 0.0,
                ringmod_depth: 0.55,
                ringmod_freq: 90.0,
                hpf_freq: 150.0,
                lpf_freq: 6000.0,
                eq_freq: 2200.0,
                eq_gain: 2.5,
                eq_q: 1.2,
                ..Default::default()
            },
            Preset {
                name: "Cylon".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Battlestar Centurion robotic voice (-3.0 st, 16 Hz square tremolo & comb slap)".to_string(),
                dpdfnet_attenuation_db: 50.0,
                pitch_semitones: -3.0,
                ringmod_depth: 0.85,
                ringmod_freq: 16.0,
                ringmod_square: 1.0,
                distortion_level: 0.10,
                delay_mix: 0.15,
                delay_time: 0.04,
                delay_feedback: 15.0,
                hpf_freq: 120.0,
                ..Default::default()
            },
            Preset {
                name: "Daft Punk".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Polished French electro synth voice (+1.0 st, 150 Hz RM, 1.2 Hz flanger sweep)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: 1.0,
                ringmod_depth: 0.40,
                ringmod_freq: 150.0,
                flanger_enabled: true,
                flanger_rate: 1.2,
                bitcrush_bits: 16,
                distortion_level: 0.06,
                eq_freq: 3800.0,
                eq_gain: 2.5,
                ..Default::default()
            },
            Preset {
                name: "Kraftwerk".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Vintage restrained electronic synth voice (120 Hz modulation, 5 kHz lowpass cut)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: 0.0,
                ringmod_depth: 0.35,
                ringmod_freq: 120.0,
                hpf_freq: 180.0,
                lpf_freq: 5000.0,
                ..Default::default()
            },
            Preset {
                name: "Dalek".to_string(),
                category: "Robot/Vocoder".to_string(),
                description: "Iconic BBC Doctor Who Dalek ring modulation (+1.0 st, 30 Hz RM & valve drive)".to_string(),
                dpdfnet_attenuation_db: 50.0,
                pitch_semitones: 1.0,
                ringmod_depth: 1.0,
                ringmod_freq: 30.0,
                distortion_level: 0.18,
                hpf_freq: 220.0,
                eq_freq: 2500.0,
                eq_gain: 3.5,
                eq_q: 1.5,
                ..Default::default()
            },

            // ==================== RADIO / SPACE ====================
            Preset {
                name: "Walkie Talkie".to_string(),
                category: "Radio/Space".to_string(),
                description: "Military radio transceiver bandpass (480–3100 Hz, 8 kHz telecom downsample)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                hpf_freq: 480.0,
                lpf_freq: 3100.0,
                bitcrush_bits: 12,
                downsample_rate: 8000,
                distortion_level: 0.14,
                eq_freq: 1800.0,
                eq_gain: 3.0,
                eq_q: 1.2,
                ..Default::default()
            },
            Preset {
                name: "Old Telephone".to_string(),
                category: "Radio/Space".to_string(),
                description: "Vintage POTS landline telephone (320–3400 Hz bandpass, clean carbon mic curve)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                hpf_freq: 320.0,
                lpf_freq: 3400.0,
                bitcrush_bits: 12,
                downsample_rate: 8000,
                distortion_level: 0.05,
                eq_freq: 1100.0,
                eq_gain: 2.0,
                ..Default::default()
            },
            Preset {
                name: "Megaphone".to_string(),
                category: "Radio/Space".to_string(),
                description: "Projected public address horn (550–4200 Hz, 2 kHz horn bell resonance & drive)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                hpf_freq: 550.0,
                lpf_freq: 4200.0,
                distortion_level: 0.16,
                eq_freq: 2000.0,
                eq_gain: 3.5,
                eq_q: 1.5,
                ..Default::default()
            },
            Preset {
                name: "Cave".to_string(),
                category: "Radio/Space".to_string(),
                description: "Deep cavern acoustic echo (140 ms pre-delay, 35% feedback, 5.5 kHz air damping)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                delay_mix: 0.30,
                delay_time: 0.14,
                delay_feedback: 35.0,
                lpf_freq: 5500.0,
                ..Default::default()
            },
            Preset {
                name: "Stadium Echo".to_string(),
                category: "Radio/Space".to_string(),
                description: "Massive public arena PA system echo (240 ms slap, 45% feedback)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                delay_mix: 0.35,
                delay_time: 0.24,
                delay_feedback: 45.0,
                eq_freq: 3000.0,
                eq_gain: 2.0,
                ..Default::default()
            },

            // ==================== EXPERIMENTAL ====================
            Preset {
                name: "Sci-Fi Alien".to_string(),
                category: "Experimental".to_string(),
                description: "Harmonic extraterrestrial voice (+4.0 st, 190 Hz RM, flanger modulation)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: 4.0,
                ringmod_depth: 0.40,
                ringmod_freq: 190.0,
                flanger_enabled: true,
                flanger_rate: 0.6,
                ..Default::default()
            },
            Preset {
                name: "Ghost".to_string(),
                category: "Experimental".to_string(),
                description: "Ethereal spectral whisper (+2.0 st, ambient feedback delay & liquid flanger)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                pitch_semitones: 2.0,
                delay_mix: 0.25,
                delay_time: 0.18,
                delay_feedback: 45.0,
                flanger_enabled: true,
                flanger_rate: 0.3,
                ..Default::default()
            },
            Preset {
                name: "Demon".to_string(),
                category: "Experimental".to_string(),
                description: "Abyssal demonic entity (-6.0 st, 42 Hz growl, tube distortion & presence)".to_string(),
                dpdfnet_attenuation_db: 50.0,
                pitch_semitones: -6.0,
                ringmod_depth: 0.18,
                ringmod_freq: 42.0,
                distortion_level: 0.22,
                hpf_freq: 80.0,
                eq_freq: 2400.0,
                eq_gain: 2.5,
                ..Default::default()
            },
            Preset {
                name: "Cyber Chopper".to_string(),
                category: "Experimental".to_string(),
                description: "Rhythmic cyber stutter voice (6 Hz square AM stutter, -2.0 st pitch)".to_string(),
                dpdfnet_attenuation_db: 50.0,
                pitch_semitones: -2.0,
                ringmod_depth: 0.85,
                ringmod_freq: 6.0,
                ringmod_square: 1.0,
                ..Default::default()
            },
            Preset {
                name: "Underwater".to_string(),
                category: "Experimental".to_string(),
                description: "Submerged acoustic immersion (650 Hz lowpass, liquid flanger chorus)".to_string(),
                dpdfnet_attenuation_db: 45.0,
                lpf_freq: 650.0,
                flanger_enabled: true,
                flanger_rate: 0.35,
                eq_freq: 180.0,
                eq_gain: 3.0,
                ..Default::default()
            },
            Preset {
                name: "Lo-Fi Static".to_string(),
                category: "Experimental".to_string(),
                description: "Vintage 10-bit arcade downsampling (6000 Hz sample rate, valve saturation)".to_string(),
                dpdfnet_attenuation_db: 48.0,
                bitcrush_bits: 10,
                downsample_rate: 6000,
                distortion_level: 0.18,
                hpf_freq: 200.0,
                lpf_freq: 5000.0,
                ..Default::default()
            },
        ];
        for p in presets.iter_mut() {
            if !p.name.eq_ignore_ascii_case("Natural Clean") && (p.model_gain_db - 0.0).abs() < 1e-4
            {
                p.model_gain_db = 2.0;
            }
        }
        presets
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

    /// Reread presets.toml and verified status from disk without restarting application
    pub fn reload_from_disk(&mut self) -> Result<()> {
        self.load()
    }

    pub fn all_presets(&self) -> &[Preset] {
        &self.config.presets
    }

    pub fn factory_presets(&self) -> Vec<Preset> {
        self.config
            .presets
            .iter()
            .filter(|p| !p.user)
            .cloned()
            .collect()
    }

    pub fn user_presets(&self) -> Vec<Preset> {
        self.config
            .presets
            .iter()
            .filter(|p| p.user)
            .cloned()
            .collect()
    }

    pub fn save_user_preset(&mut self, mut preset: Preset) -> Result<()> {
        preset.user = true;
        self.config
            .presets
            .retain(|p| !p.name.eq_ignore_ascii_case(&preset.name));
        self.config.presets.push(preset);
        self.save()
    }

    pub fn delete_user_preset(&mut self, name: &str) -> Result<bool> {
        let before = self.config.presets.len();
        self.config
            .presets
            .retain(|p| !(p.user && p.name.eq_ignore_ascii_case(name)));
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

    /// Reset a factory preset to its default parameters
    pub fn reset_preset(&mut self, name: &str) -> Result<bool> {
        let factory_defs = PresetConfig::default_factory_presets();
        let Some(def) = factory_defs
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
        else {
            return Ok(false);
        };
        if let Some(target) = self
            .config
            .presets
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(name))
        {
            let verified = target.verified;
            *target = def;
            target.verified = verified;
            self.save()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Reset ALL factory presets back to their default factory parameters
    pub fn reset_all_presets(&mut self) -> Result<()> {
        let factory_defs = PresetConfig::default_factory_presets();
        let mut user_presets: Vec<Preset> = self
            .config
            .presets
            .iter()
            .filter(|p| p.user)
            .cloned()
            .collect();
        let mut new_presets = factory_defs;
        new_presets.append(&mut user_presets);
        self.config.presets = new_presets;
        self.save()
    }

    /// Update a preset in-place (e.g. from live control tweaks)
    pub fn update_preset_parameters(&mut self, preset: &Preset) -> Result<()> {
        if let Some(target) = self
            .config
            .presets
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(&preset.name))
        {
            *target = preset.clone();
            self.save()?;
        }
        Ok(())
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
                    return arr
                        .iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect();
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
                let mut existing: HashSet<String> = parsed
                    .presets
                    .iter()
                    .map(|p| p.name.to_lowercase())
                    .collect();
                for def in PresetConfig::default_factory_presets() {
                    if !existing.contains(&def.name.to_lowercase()) {
                        parsed.presets.push(def.clone());
                        existing.insert(def.name.to_lowercase());
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
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_path =
            self.config_dir
                .join(format!("presets.toml.tmp.{}.{}", std::process::id(), nanos));
        fs::write(&tmp_path, data)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }
}
