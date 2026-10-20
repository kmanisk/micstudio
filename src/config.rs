//! MicStudio clean microphone noise, gate, and level settings persistence.
//! Manages versioned settings at ~/.config/micstudio/settings.toml.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SETTINGS_VERSION: u32 = 3;

fn default_schema_version() -> u32 {
    SETTINGS_VERSION
}

fn default_dpdfnet_attenuation() -> f32 {
    100.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanMicSettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    // Primary live controls
    #[serde(default = "default_dpdfnet_attenuation")]
    pub dpdfnet_attenuation_db: f32, // 0.0 to 100.0 (dB)
    #[serde(default)]
    pub output_gain_db: f32, // -24.0 to +12.0 (dB)
    #[serde(default)]
    pub quiet_voice_boost_enabled: bool,
    #[serde(default)]
    pub quiet_voice_boost_db: f32, // 0.0 to +10.0 (dB)
    #[serde(default)]
    pub hear_voice: bool, // Sidetone / live mic monitor
    #[serde(default)]
    pub force_speaker_sidetone: bool, // Bypass laptop speaker guard to hear voice through built-in speakers
    #[serde(default)]
    pub meter_enabled: bool, // Live level meter in TUI

    // Legacy fields preserved for TOML compatibility
    #[serde(default)]
    pub rnnoise_enabled: bool,
    #[serde(default)]
    pub denoise_blend: f32,
    #[serde(default)]
    pub gate_enabled: bool,
    #[serde(default)]
    pub gate_aggressiveness: f32,
    #[serde(default)]
    pub silence_attenuation: f32,
    #[serde(default)]
    pub hangover_ms: u32,
    #[serde(default)]
    pub telemetry_enabled: bool,
    #[serde(default)]
    pub create_virtual_device: bool,
    #[serde(default)]
    pub virtual_device_name: String,
    #[serde(default)]
    pub theme: String,
}

impl Default for CleanMicSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_VERSION,
            dpdfnet_attenuation_db: 45.0,
            output_gain_db: 4.5,
            quiet_voice_boost_enabled: true,
            quiet_voice_boost_db: 4.0,
            hear_voice: false,
            force_speaker_sidetone: false,
            meter_enabled: false,
            rnnoise_enabled: true,
            denoise_blend: 100.0,
            gate_enabled: true,
            gate_aggressiveness: 35.0,
            silence_attenuation: -36.0,
            hangover_ms: 500,
            telemetry_enabled: false,
            create_virtual_device: false,
            virtual_device_name: "MicStudio Studio Source".to_string(),
            theme: "nvchad".to_string(),
        }
    }
}

impl CleanMicSettings {
    pub fn config_path() -> Result<PathBuf> {
        let home = std::env::var("HOME").context("HOME env var not set")?;
        Ok(PathBuf::from(home).join(".config/micstudio/settings.toml"))
    }

    pub fn load() -> Self {
        match Self::config_path().and_then(|p| fs::read_to_string(&p).context("Read failed")) {
            Ok(content) => match toml::from_str::<CleanMicSettings>(&content) {
                Ok(mut s) => {
                    s.validate_clamp();
                    s
                }
                Err(_) => {
                    let s = Self::default();
                    let _ = s.save();
                    s
                }
            },
            Err(_) => {
                let s = Self::default();
                let _ = s.save();
                s
            }
        }
    }

    pub fn validate_clamp(&mut self) {
        self.schema_version = SETTINGS_VERSION;
        self.dpdfnet_attenuation_db = self.dpdfnet_attenuation_db.clamp(0.0, 100.0);
        self.output_gain_db = self.output_gain_db.clamp(-24.0, 12.0);
        self.quiet_voice_boost_db = self.quiet_voice_boost_db.clamp(0.0, 10.0);
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create micstudio config dir")?;
        }
        let serialized = toml::to_string_pretty(self).context("Failed to serialize settings")?;
        let tmp_path = path.with_extension("toml.tmp");
        fs::write(&tmp_path, serialized).context("Failed to write temporary settings")?;
        fs::rename(&tmp_path, &path).context("Failed to atomically commit settings")?;
        Ok(())
    }

    /// Live apply: update DPDFNet attenuation, quiet speech leveling, and output gain directly via PipeWire pw-cli Props
    pub fn apply_dsp_params(&self) -> Result<()> {
        let capture_id = match get_node_id("capture.ghelper-audio")? {
            Some(id) => id,
            None => return Ok(()),
        };

        let atten = self.dpdfnet_attenuation_db.clamp(0.0, 100.0);
        let linear_gain = 10.0f32.powf(self.output_gain_db / 20.0);

        let (thresh_linear, makeup_linear) =
            if self.quiet_voice_boost_enabled && self.quiet_voice_boost_db > 0.01 {
                let boost = self.quiet_voice_boost_db.clamp(0.0, 10.0);
                let thresh_db = (-12.0 - (boost * 1.5)).clamp(-24.0, -12.0);
                let makeup_db = (3.0 + boost).clamp(3.0, 12.0);
                (
                    10.0f32.powf(thresh_db / 20.0),
                    10.0f32.powf(makeup_db / 20.0),
                )
            } else {
                // Baseline factory values from 40-mic.conf (-12 dBFS threshold, +3 dB makeup)
                (0.251189, 1.412538)
            };

        let props_payload = format!(
            "{{ params = [ \"dpdfnet:Attenuation Limit (dB)\" {:.1}, \"limiter:Output gain (G)\" {:.4}, \"compressor:Attack threshold (G)\" {:.6}, \"compressor:Makeup gain (G)\" {:.6}, \"compressor:Attack time (ms)\" 5.0, \"compressor:Hold time (ms)\" 120.0, \"compressor:Release time (ms)\" 500.0, \"compressor:Ratio\" 2.0 ] }}",
            atten, linear_gain, thresh_linear, makeup_linear
        );

        let _ = Command::new("pw-cli")
            .args(["s", &capture_id, "Props", &props_payload])
            .output();

        Ok(())
    }

    /// Reset capture.ghelper-audio compressor to factory baseline (-12 dBFS threshold, +3 dB makeup).
    /// Used when applying creative FX presets to guarantee creative FX never inherit extra quiet boost or compression.
    pub fn reset_clean_compressor_baseline() -> Result<()> {
        let capture_id = match get_node_id("capture.ghelper-audio")? {
            Some(id) => id,
            None => return Ok(()),
        };
        let cfg = Self::load();
        let atten = cfg.dpdfnet_attenuation_db.clamp(0.0, 100.0);
        let linear_gain = 10.0f32.powf(cfg.output_gain_db / 20.0);

        let props_payload = format!(
            "{{ params = [ \"dpdfnet:Attenuation Limit (dB)\" {:.1}, \"limiter:Output gain (G)\" {:.4}, \"compressor:Attack threshold (G)\" 0.251189, \"compressor:Makeup gain (G)\" 1.000000, \"compressor:Attack time (ms)\" 5.0, \"compressor:Hold time (ms)\" 120.0, \"compressor:Release time (ms)\" 500.0, \"compressor:Ratio\" 2.5 ] }}",
            atten, linear_gain
        );

        let _ = Command::new("pw-cli")
            .args(["s", &capture_id, "Props", &props_payload])
            .output();

        Ok(())
    }

    /// Full apply: DSP parameters + sidetone routing state
    pub fn apply_to_pipewire(&self) -> Result<()> {
        self.apply_dsp_params()?;
        Self::sync_hear_voice_with_force(self.hear_voice, self.force_speaker_sidetone);
        Ok(())
    }

    pub fn sync_hear_voice(enable: bool) {
        let cfg = Self::load();
        Self::sync_hear_voice_with_force(enable, cfg.force_speaker_sidetone);
    }

    pub fn is_physically_speaker() -> bool {
        let sink_out = Command::new("pactl").arg("get-default-sink").output();
        let default_sink = if let Ok(out) = sink_out {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            String::new()
        };
        if default_sink.is_empty() {
            return false;
        }
        let lower_sink = default_sink.to_lowercase();
        let mut is_headphone = lower_sink.contains("headphone")
            || lower_sink.contains("headset")
            || lower_sink.contains("bluez");

        // If it's an ALSA sink (e.g. analog-stereo), query its active port to distinguish 3.5mm wired headphones from built-in speakers
        if !is_headphone {
            if let Ok(pactl_sinks) = Command::new("pactl").args(["list", "sinks"]).output() {
                let stdout_str = String::from_utf8_lossy(&pactl_sinks.stdout);
                let mut current_sink_matches = false;
                for line in stdout_str.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("Name: ") {
                        let name = trimmed.trim_start_matches("Name: ").trim();
                        current_sink_matches = name == default_sink;
                    } else if current_sink_matches && trimmed.starts_with("Active Port: ") {
                        let port = trimmed.trim_start_matches("Active Port: ").trim().to_lowercase();
                        if port.contains("headphone") || port.contains("headset") {
                            is_headphone = true;
                        }
                        break;
                    }
                }
            }
        }

        let is_speaker = lower_sink.contains("speaker")
            || lower_sink.contains("peace_gaming")
            || (lower_sink.contains("analog-stereo") && !is_headphone);

        is_speaker && !is_headphone
    }

    pub fn is_sidetone_guarded() -> bool {
        let cfg = Self::load();
        if cfg.force_speaker_sidetone {
            return false;
        }
        Self::is_physically_speaker()
    }

    pub fn sync_hear_voice_with_source(enable: bool, _fx_active: bool) {
        let cfg = Self::load();
        Self::sync_hear_voice_with_force(enable, cfg.force_speaker_sidetone);
    }

    pub fn sync_hear_voice_with_force(enable: bool, force_speaker: bool) {
        // Disconnect any existing monitor links unconditionally from both sources
        disconnect_monitor_links("ghelper-audio:capture_MONO");
        disconnect_monitor_links("ghelper-audio-fx:capture_MONO");

        if !enable {
            return;
        }

        // In unified single-device architecture, ghelper-audio provides both clean and active voice FX
        let monitor_source = "ghelper-audio:capture_MONO";

        if force_speaker {
            // User requested explicit override: force output to laptop speakers via Dolby_Speaker and hardware analog stereo
            let _ = Command::new("pw-link")
                .args([monitor_source, "effect_input.Dolby_Speaker:playback_FL"])
                .output();
            let _ = Command::new("pw-link")
                .args([monitor_source, "effect_input.Dolby_Speaker:playback_FR"])
                .output();
            let _ = Command::new("pw-link")
                .args([monitor_source, "alsa_output.pci-0000_00_1f.3.analog-stereo:playback_FL"])
                .output();
            let _ = Command::new("pw-link")
                .args([monitor_source, "alsa_output.pci-0000_00_1f.3.analog-stereo:playback_FR"])
                .output();
            return;
        }

        // Default behavior (force_speaker == false):
        // NEVER route to laptop speakers; strictly route to headphones or external sinks.
        if Self::is_physically_speaker() {
            return;
        }

        // Query active default sink (headphones / headset)
        let sink_out = Command::new("pactl").arg("get-default-sink").output();
        let default_sink = if let Ok(out) = sink_out {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            String::new()
        };

        if !default_sink.is_empty() {
            let left_port = format!("{}:playback_FL", default_sink);
            let right_port = format!("{}:playback_FR", default_sink);
            let _ = Command::new("pw-link")
                .args([monitor_source, &left_port])
                .output();
            let _ = Command::new("pw-link")
                .args([monitor_source, &right_port])
                .output();
        }
    }
}

pub fn is_voice_fx_active() -> bool {
    if let Ok(mgr) = crate::presets::PresetManager::new() {
        return !mgr.active_preset_name().eq_ignore_ascii_case("Natural Clean");
    }
    false
}

pub fn disconnect_monitor_links(source_port: &str) {
    if let Ok(out) = Command::new("pw-link").arg("-l").output() {
        let text = String::from_utf8_lossy(&out.stdout);
        let mut cur_port = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if line.starts_with(' ') && line.contains("|->") {
                if cur_port == source_port {
                    if let Some(target) = line.split("|->").nth(1) {
                        let target_clean = target.trim();
                        // Only disconnect sidetone playback destinations (sinks/headphones), NEVER recording/meter streams
                        if target_clean.contains("playback_") {
                            let _ = Command::new("pw-link")
                                .args(["-d", source_port, target_clean])
                                .output();
                        }
                    }
                }
            } else if line.starts_with(' ') && line.contains("|<-") {
                if let Some(src) = line.split("|<-").nth(1) {
                    if src.trim() == source_port {
                        // Only disconnect if destination port is a playback sink, NEVER meter/recording inputs
                        if cur_port.contains("playback_") {
                            let _ = Command::new("pw-link")
                                .args(["-d", source_port, &cur_port])
                                .output();
                        }
                    }
                }
            } else if !line.starts_with(' ') {
                // Header line: port name (unindented)
                cur_port = trimmed.to_string();
            }
        }
    }
}

/// Ensures the live meter and spectrograph input is linked to ghelper-audio
pub fn ensure_meter_linked() {
    let _ = Command::new("pw-link")
        .args(["ghelper-audio:capture_MONO", "micstudio-meter-capture:input_MONO"])
        .output();
}

pub fn get_node_id(node_name: &str) -> Result<Option<String>> {
    let output = Command::new("pw-dump")
        .arg("Node")
        .output()
        .context("Failed to run pw-dump")?;
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    if let Some(arr) = json.as_array() {
        for item in arr {
            if let Some(name) = item
                .get("info")
                .and_then(|i| i.get("props"))
                .and_then(|p| p.get("node.name"))
                .and_then(|n| n.as_str())
            {
                if name == node_name {
                    if let Some(id) = item.get("id").and_then(|id| id.as_u64()) {
                        return Ok(Some(id.to_string()));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Dynamically parse 40-mic.conf to list the ordered filter graph stages
pub fn parse_dynamic_stages() -> Vec<String> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return fallback_stages(),
    };
    let conf_path = PathBuf::from(home).join(".config/pipewire/pipewire.conf.d/40-mic.conf");
    let content = match fs::read_to_string(&conf_path) {
        Ok(c) => c,
        Err(_) => return fallback_stages(),
    };

    let mut stages = Vec::new();
    let mut saw_eq = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(pos) = trimmed.find("name") {
            let rest = &trimmed[pos + 4..];
            if let Some(eq_pos) = rest.find('=') {
                let val_part = rest[eq_pos + 1..].trim();
                let name = val_part
                    .trim_matches(|c| c == '"' || c == '\'' || c == ' ' || c == '\t' || c == '{')
                    .split_whitespace()
                    .next()
                    .unwrap_or("");

                if name.starts_with("capture") || name.starts_with("ghelper") || name.is_empty() {
                    continue;
                }

                if name.starts_with("eq_") {
                    if !saw_eq {
                        stages.push("9-Band EQ".to_string());
                        saw_eq = true;
                    }
                } else if name == "hpf_70" {
                    stages.push("70Hz HPF".to_string());
                } else if name == "dpdfnet" {
                    stages.push("DPDFNet Denoise".to_string());
                } else if name == "dpdfnet_latency" {
                    // skip latency compensation node for display brevity
                } else if name == "compressor" {
                    stages.push("LSP Compressor".to_string());
                } else if name == "limiter" {
                    stages.push("Peak Limiter".to_string());
                } else {
                    stages.push(name.to_string());
                }
            }
        }
    }

    if stages.is_empty() {
        fallback_stages()
    } else {
        stages
    }
}

fn fallback_stages() -> Vec<String> {
    vec![
        "70Hz HPF".into(),
        "DPDFNet Denoise".into(),
        "9-Band EQ".into(),
        "LSP Compressor".into(),
        "Peak Limiter".into(),
    ]
}
