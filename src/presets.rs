//! Voice Preset Model and Storage Manager.
//! Stores built-in factory presets and user custom presets in ~/.config/micstudio/presets.toml.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub description: String,
    
    // Core parameters
    pub pitch_semitones: f32, // -24.0 to +24.0
    pub vocoder_mix: f32,     // 0.0 to 1.0
    pub vocoder_carrier_hz: f32,
    pub vocoder_detune: f32,
    pub matrix_ring_mod: f32, // 0.0 to 1.0
    
    // Filter & shaping
    pub bandpass_hpf: f32,    // Hz (0 = off)
    pub bandpass_lpf: f32,    // Hz (0 = off)
    pub bitcrush_bits: u32,   // 2 to 16
    pub bitcrush_downsample: u32, // 1 to 64
    pub stutter_rate: f32,    // Hz (0 = off)
    pub stutter_duty: f32,    // 0.1 to 0.9
    
    // Clean processing stages
    pub noise_suppression: bool,
    pub noise_gate_aggro: f32, // 0.0 to 1.0
    pub eq_enabled: bool,
    pub eq_gains: [f32; 9],
    pub eq_post_gain: f32,
    pub compressor_enabled: bool,
    pub limiter_enabled: bool,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: "Natural Clean".to_string(),
            description: "Transparent broadcast voice with neural denoising, 9-band EQ, and limiter".to_string(),
            pitch_semitones: 0.0,
            vocoder_mix: 0.0,
            vocoder_carrier_hz: 110.0,
            vocoder_detune: 0.0,
            matrix_ring_mod: 0.0,
            bandpass_hpf: 0.0,
            bandpass_lpf: 0.0,
            bitcrush_bits: 16,
            bitcrush_downsample: 1,
            stutter_rate: 0.0,
            stutter_duty: 0.5,
            noise_suppression: true,
            noise_gate_aggro: 0.70,
            eq_enabled: true,
            eq_gains: [-4.0, -2.0, -1.0, -2.0, 1.0, 3.0, 4.0, 4.0, 2.0],
            eq_post_gain: 0.0,
            compressor_enabled: true,
            limiter_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub schema_version: u32,
    pub active_preset: String,
    pub user_presets: Vec<Preset>,
}

impl Default for PresetConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            active_preset: "Natural Clean".to_string(),
            user_presets: Vec::new(),
        }
    }
}

pub struct PresetManager {
    config_dir: PathBuf,
    config: PresetConfig,
    factory_presets: Vec<Preset>,
}

impl PresetManager {
    pub fn new() -> Result<Self> {
        let home = std::env::var("HOME").context("HOME not set")?;
        let config_dir = Path::new(&home).join(".config").join("micstudio");
        let mut mgr = Self {
            config_dir,
            config: PresetConfig::default(),
            factory_presets: Self::init_factory_presets(),
        };
        mgr.load()?;
        Ok(mgr)
    }

    pub fn factory_presets(&self) -> &[Preset] {
        &self.factory_presets
    }

    pub fn user_presets(&self) -> &[Preset] {
        &self.config.user_presets
    }

    pub fn all_presets(&self) -> Vec<Preset> {
        let mut list = self.factory_presets.clone();
        list.extend(self.config.user_presets.clone());
        list
    }

    pub fn find_preset(&self, name: &str) -> Option<Preset> {
        let name_lower = name.trim().to_lowercase();
        self.all_presets()
            .into_iter()
            .find(|p| p.name.to_lowercase() == name_lower)
    }

    pub fn active_preset_name(&self) -> &str {
        &self.config.active_preset
    }

    pub fn set_active_preset(&mut self, name: &str) -> Result<()> {
        self.config.active_preset = name.to_string();
        self.save()
    }

    pub fn save_user_preset(&mut self, preset: Preset) -> Result<()> {
        if let Some(pos) = self.config.user_presets.iter().position(|p| p.name.eq_ignore_ascii_case(&preset.name)) {
            self.config.user_presets[pos] = preset;
        } else {
            self.config.user_presets.push(preset);
        }
        self.save()
    }

    pub fn delete_user_preset(&mut self, name: &str) -> Result<bool> {
        let initial_len = self.config.user_presets.len();
        self.config.user_presets.retain(|p| !p.name.eq_ignore_ascii_case(name));
        if self.config.user_presets.len() != initial_len {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn config_path(&self) -> PathBuf {
        self.config_dir.join("presets.toml")
    }

    fn load(&mut self) -> Result<()> {
        let path = self.config_path();
        if path.exists() {
            let data = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            self.config = toml::from_str(&data)
                .with_context(|| format!("Failed to parse TOML from {}", path.display()))?;
        } else {
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let data = toml::to_string_pretty(&self.config)?;
        let path = self.config_path();
        let tmp_path = self.config_dir.join("presets.toml.tmp");
        fs::write(&tmp_path, data)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    fn init_factory_presets() -> Vec<Preset> {
        vec![
            Preset {
                name: "Natural Clean".to_string(),
                description: "Clean broadcast voice".to_string(),
                ..Default::default()
            },
            Preset {
                name: "Daft Punk".to_string(),
                description: "Electronic vocoder & talkbox".to_string(),
                vocoder_mix: 0.95,
                vocoder_carrier_hz: 110.0,
                vocoder_detune: 4.0,
                matrix_ring_mod: 0.10,
                ..Default::default()
            },
            Preset {
                name: "Darth Vader".to_string(),
                description: "Heavy mechanical pitch (-8 st)".to_string(),
                pitch_semitones: -8.0,
                bandpass_hpf: 70.0,
                bandpass_lpf: 4500.0,
                matrix_ring_mod: 0.35,
                ..Default::default()
            },
            Preset {
                name: "Chipmunk".to_string(),
                description: "Extreme high pitch (+12 st)".to_string(),
                pitch_semitones: 12.0,
                bandpass_hpf: 150.0,
                bandpass_lpf: 0.0,
                ..Default::default()
            },
            Preset {
                name: "Cylon Robot".to_string(),
                description: "16-band resonant robot vocoder".to_string(),
                vocoder_mix: 0.90,
                vocoder_carrier_hz: 85.0,
                vocoder_detune: 6.0,
                matrix_ring_mod: 0.20,
                ..Default::default()
            },
            Preset {
                name: "Kraftwerk".to_string(),
                description: "Classic synth speech".to_string(),
                vocoder_mix: 0.95,
                vocoder_carrier_hz: 130.0,
                vocoder_detune: 2.0,
                matrix_ring_mod: 0.05,
                ..Default::default()
            },
            Preset {
                name: "Matrix Agent".to_string(),
                description: "Deep monotone voice (-4 st)".to_string(),
                pitch_semitones: -4.0,
                bandpass_hpf: 90.0,
                bandpass_lpf: 5500.0,
                matrix_ring_mod: 0.15,
                ..Default::default()
            },
            Preset {
                name: "Robot Phone".to_string(),
                description: "Bandpass filtered robot voice".to_string(),
                bandpass_hpf: 400.0,
                bandpass_lpf: 3000.0,
                bitcrush_bits: 10,
                bitcrush_downsample: 2,
                matrix_ring_mod: 0.30,
                ..Default::default()
            },
            Preset {
                name: "Sci-Fi Alien".to_string(),
                description: "Ring-modulated alien (+7 st)".to_string(),
                pitch_semitones: 7.0,
                bandpass_hpf: 600.0,
                bandpass_lpf: 3500.0,
                matrix_ring_mod: 0.60,
                ..Default::default()
            },
            Preset {
                name: "T-Pain Autotune".to_string(),
                description: "Sharp pitch quantization".to_string(),
                pitch_semitones: 3.0,
                vocoder_mix: 0.35,
                vocoder_carrier_hz: 140.0,
                vocoder_detune: 1.0,
                ..Default::default()
            },
            Preset {
                name: "Stephen Hawking".to_string(),
                description: "Robotic speech synthesizer".to_string(),
                vocoder_mix: 1.0,
                vocoder_carrier_hz: 100.0,
                vocoder_detune: 0.0,
                matrix_ring_mod: 0.0,
                ..Default::default()
            },
            Preset {
                name: "Megatron".to_string(),
                description: "Metallic robotic overlord".to_string(),
                pitch_semitones: -6.0,
                bandpass_hpf: 100.0,
                bandpass_lpf: 5000.0,
                matrix_ring_mod: 0.50,
                vocoder_mix: 0.40,
                vocoder_carrier_hz: 75.0,
                vocoder_detune: 8.0,
                ..Default::default()
            },
            Preset {
                name: "Custom".to_string(),
                description: "User customizable configuration slot".to_string(),
                ..Default::default()
            },
        ]
    }
}
