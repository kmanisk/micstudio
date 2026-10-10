//! MicStudio clean microphone noise, gate, and level settings persistence.
//! Manages versioned settings at ~/.config/micstudio/settings.toml.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SETTINGS_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanMicSettings {
    pub schema_version: u32,
    pub rnnoise_enabled: bool,
    pub denoise_blend: f32,       // 0.0 to 100.0 (%)
    pub gate_enabled: bool,
    pub gate_aggressiveness: f32, // 0.0 to 100.0 (%)
    pub silence_attenuation: f32, // -60.0 to 0.0 (dB)
    pub hangover_ms: u32,         // 0 to 1000 (ms)
    pub output_gain_db: f32,      // -24.0 to +12.0 (dB)
    #[serde(default)]
    pub hear_voice: bool,         // Sidetone / live mic monitor
    #[serde(default)]
    pub telemetry_enabled: bool,  // Live signal telemetry in TUI
    #[serde(default)]
    pub create_virtual_device: bool, // Expose separate virtual source device
    #[serde(default = "default_device_name")]
    pub virtual_device_name: String, // Custom name for virtual source
}

fn default_device_name() -> String {
    "MicStudio Studio Source".to_string()
}

impl Default for CleanMicSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_VERSION,
            rnnoise_enabled: true,
            denoise_blend: 100.0,
            gate_enabled: true,
            gate_aggressiveness: 56.0,
            silence_attenuation: -36.0,
            hangover_ms: 300,
            output_gain_db: 2.0,
            hear_voice: false,
            telemetry_enabled: false,
            create_virtual_device: false,
            virtual_device_name: default_device_name(),
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
        self.denoise_blend = self.denoise_blend.clamp(0.0, 100.0);
        self.gate_aggressiveness = self.gate_aggressiveness.clamp(0.0, 100.0);
        self.silence_attenuation = self.silence_attenuation.clamp(-60.0, 0.0);
        self.hangover_ms = self.hangover_ms.clamp(0, 1000);
        self.output_gain_db = self.output_gain_db.clamp(-24.0, 12.0);
        if self.virtual_device_name.trim().is_empty() {
            self.virtual_device_name = default_device_name();
        }
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

    /// Fast path: apply live LADSPA parameters to the running filter chain without re-querying node IDs
    pub fn apply_dsp_params(&self) -> Result<()> {
        let capture_id = match cached_capture_node_id() {
            Some(id) => id,
            None => match get_node_id("capture.ghelper-audio")? {
                Some(id) => id,
                None => return Ok(()),
            },
        };

        let dry_mix = if !self.rnnoise_enabled {
            1.0f32
        } else {
            (100.0 - self.denoise_blend) / 100.0
        };

        let (vad_thresh, vad_grace) = if !self.gate_enabled {
            (0u32, 1000u32)
        } else {
            let thresh = (10.0 + (self.gate_aggressiveness / 100.0) * 80.0).round() as u32;
            (thresh.clamp(0, 99), self.hangover_ms.clamp(0, 1000))
        };

        let linear_gain = 10.0f32.powf(self.output_gain_db / 20.0);

        let props_payload = format!(
            "{{ params = [ \"rnnoise:Dry Mix\" {:.4}, \"rnnoise:VAD Threshold (%)\" {}, \"rnnoise:VAD Grace Period (ms)\" {}, \"compressor:Output gain (G)\" {:.4} ] }}",
            dry_mix, vad_thresh, vad_grace, linear_gain
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
            if trimmed.is_empty() { continue; }
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

static CACHED_CAPTURE_ID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

pub fn cached_capture_node_id() -> Option<String> {
    CACHED_CAPTURE_ID.get_or_init(|| get_node_id("capture.ghelper-audio").ok().flatten()).clone()
}

fn get_node_id(node_name: &str) -> Result<Option<String>> {
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
