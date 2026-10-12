//! ASUS TUF F16 Upstream DAX3 / Dolby Atmos Speaker DSP controller.
//! Direct zero-daemon control for authenticated Windows DAX3 presets and Linux voicings.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeakerVoicing {
    Custom,   // Custom 2 (Dolby Access)
    Dynamic,  // Dynamic (DAX3 Default)
    Music,    // Music (DAX3 Audiophile)
    Balanced, // Balanced (Linux Reference)
}

impl SpeakerVoicing {
    pub const ALL: [SpeakerVoicing; 4] = [
        SpeakerVoicing::Custom,
        SpeakerVoicing::Dynamic,
        SpeakerVoicing::Music,
        SpeakerVoicing::Balanced,
    ];

    pub fn id_str(&self) -> &'static str {
        match self {
            SpeakerVoicing::Custom => "custom",
            SpeakerVoicing::Dynamic => "dynamic",
            SpeakerVoicing::Music => "music",
            SpeakerVoicing::Balanced => "balanced",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            SpeakerVoicing::Custom => "Custom 2 (Dolby Access)",
            SpeakerVoicing::Dynamic => "Dynamic (DAX3 Default)",
            SpeakerVoicing::Music => "Music (DAX3 Audiophile)",
            SpeakerVoicing::Balanced => "Balanced (Linux)",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            SpeakerVoicing::Custom => "+11.6 dB presence, user calibrated curve",
            SpeakerVoicing::Dynamic => "Leveler + Spatial enhancement",
            SpeakerVoicing::Music => "Audiophile uncompressed dynamics",
            SpeakerVoicing::Balanced => "Reference acoustic baseline",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "custom" | "c" => Some(SpeakerVoicing::Custom),
            "dynamic" | "dyn" => Some(SpeakerVoicing::Dynamic),
            "music" | "mu" => Some(SpeakerVoicing::Music),
            "balanced" | "b" => Some(SpeakerVoicing::Balanced),
            _ => None,
        }
    }
}

pub struct SpeakerDsp;

impl SpeakerDsp {
    pub fn state_file() -> Result<PathBuf> {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home).join(".config/asus-speakers/state.conf"))
    }

    pub fn get_status() -> Result<(SpeakerVoicing, bool, String)> {
        let file = Self::state_file()?;
        let mut voicing = SpeakerVoicing::Dynamic;
        let mut enabled = true;

        if file.exists() {
            let content = fs::read_to_string(&file)?;
            for line in content.lines() {
                if let Some(v) = line.strip_prefix("ACTIVE_VOICING=") {
                    if let Some(pv) = SpeakerVoicing::from_str(v.trim()) {
                        voicing = pv;
                    }
                } else if let Some(e) = line.strip_prefix("ENABLED=") {
                    enabled = e.trim() == "true";
                }
            }
        }

        let route = Self::query_route(enabled, voicing);
        Ok((voicing, enabled, route))
    }

    pub fn apply_voicing(voicing: SpeakerVoicing) -> Result<()> {
        let _ = Command::new("spk")
            .arg(voicing.id_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        Ok(())
    }

    pub fn set_enabled(enable: bool) -> Result<()> {
        let cmd = if enable { "on" } else { "off" };
        let _ = Command::new("spk")
            .arg(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        Ok(())
    }

    fn query_route(enabled: bool, voicing: SpeakerVoicing) -> String {
        let output = Command::new("pactl").arg("get-default-sink").output();
        let default_sink = if let Ok(out) = output {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            String::new()
        };

        if default_sink == "alsa_output.pci-0000_00_1f.3.analog-stereo" {
            if !enabled {
                "Laptop Speakers (ALC256) • DSP Bypassed".to_string()
            } else {
                format!("Laptop Speakers (ALC256) • {}", voicing.display_name())
            }
        } else if default_sink.starts_with("bluez_output") {
            "Bluetooth Audio Sink".to_string()
        } else if default_sink.contains("hdmi") || default_sink.contains("displayport") {
            "HDMI/DP Audio Sink".to_string()
        } else if !default_sink.is_empty() {
            default_sink
        } else {
            "Default Audio Sink".to_string()
        }
    }
}
