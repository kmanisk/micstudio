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
    pub output_gain_db: f32,          // -24.0 to +12.0 (dB)
    #[serde(default)]
    pub hear_voice: bool,             // Sidetone / live mic monitor
    #[serde(default)]
    pub meter_enabled: bool,          // Live level meter in TUI

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
}

impl Default for CleanMicSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_VERSION,
            dpdfnet_attenuation_db: 100.0,
            output_gain_db: 0.0,
            hear_voice: false,
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

    /// Live apply: update DPDFNet attenuation and output gain directly via PipeWire pw-cli Props
    pub fn apply_dsp_params(&self) -> Result<()> {
        let capture_id = match get_node_id("capture.ghelper-audio")? {
            Some(id) => id,
            None => return Ok(()),
        };

        let atten = self.dpdfnet_attenuation_db.clamp(0.0, 100.0);
        let linear_gain = 10.0f32.powf(self.output_gain_db / 20.0);

        let props_payload = format!(
            "{{ params = [ \"dpdfnet:Attenuation Limit (dB)\" {:.1}, \"compressor:Output gain (G)\" {:.4} ] }}",
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
        Self::sync_hear_voice(self.hear_voice);
        Ok(())
    }

    pub fn sync_hear_voice(enable: bool) {
        Self::sync_hear_voice_with_source(enable, is_voice_fx_active());
    }

    pub fn sync_hear_voice_with_source(enable: bool, fx_active: bool) {
        // Disconnect any existing monitor links from both sources
        disconnect_monitor_links("ghelper-audio:capture_MONO");
        disconnect_monitor_links("ghelper-audio-fx:capture_MONO");

        if !enable {
            return;
        }

        // Determine active monitor source: if FX is active, monitor the FX mic, otherwise clean mic
        let monitor_source = if fx_active {
            "ghelper-audio-fx:capture_MONO"
        } else {
            "ghelper-audio:capture_MONO"
        };

        // Query active default sink
        let sink_out = Command::new("pactl").arg("get-default-sink").output();
        let default_sink = if let Ok(out) = sink_out {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            String::new()
        };

        if !default_sink.is_empty() {
            let left_port = format!("{}:playback_FL", default_sink);
            let right_port = format!("{}:playback_FR", default_sink);
            let _ = Command::new("pw-link").args([monitor_source, &left_port]).output();
            let _ = Command::new("pw-link").args([monitor_source, &right_port]).output();
        }
    }
}

pub fn is_voice_fx_active() -> bool {
    let output = Command::new("wpctl").args(["inspect", "@DEFAULT_AUDIO_SOURCE@"]).output();
    if let Ok(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);
        return text.contains("ghelper-audio-fx");
    }
    false
}

fn disconnect_monitor_links(source_port: &str) {
    if let Ok(out) = Command::new("pw-link").args(["-I", "-l"]).output() {
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
                        // Only disconnect if target is a playback sink (e.g. bluez, alsa playback)
                        if target_clean.contains("playback_") {
                            let _ = Command::new("pw-link").args(["-d", source_port, target_clean]).output();
                        }
                    }
                }
            } else {
                // Header line: port name
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    cur_port = parts[1].to_string();
                }
            }
        }
    }
}

pub fn get_node_id(node_name: &str) -> Result<Option<String>> {
    let output = Command::new("pw-dump")
        .arg("Node")
        .output()
        .context("Failed to run pw-dump")?;
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    if let Some(arr) = json.as_array() {
        for item in arr {
            if let Some(name) = item.get("info").and_then(|i| i.get("props")).and_then(|p| p.get("node.name")).and_then(|n| n.as_str()) {
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
