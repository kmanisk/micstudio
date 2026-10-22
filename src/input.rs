//! Physical capture selection, persisted by the existing WirePlumber process.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::process::Command;

pub const INPUT_SETTING: &str = "micstudio.input";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDevice {
    pub name: String,
    pub description: String,
}

pub fn parse_devices(value: &Value) -> Vec<InputDevice> {
    let mut devices = Vec::new();
    for node in value.as_array().into_iter().flatten() {
        let props = &node["info"]["props"];
        let name = props["node.name"].as_str().unwrap_or_default();
        if node["type"] != "PipeWire:Interface:Node"
            || props["media.class"] != "Audio/Source"
            || props["device.id"].is_null()
            || name.is_empty()
            || name.starts_with("bluez_")
            || name == "ghelper-audio"
            || props["node.virtual"] == true
            || props["node.virtual"] == "true"
        {
            continue;
        }
        devices.push(InputDevice {
            name: name.to_string(),
            description: props["node.description"]
                .as_str()
                .unwrap_or(name)
                .to_string(),
        });
    }
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices.dedup_by(|a, b| a.name == b.name);
    devices
}

pub fn devices() -> Result<Vec<InputDevice>> {
    let output = Command::new("pw-dump").output().context("Run pw-dump")?;
    if !output.status.success() {
        bail!("PipeWire device discovery failed");
    }
    Ok(parse_devices(&serde_json::from_slice(&output.stdout)?))
}

pub fn selected() -> Result<String> {
    let output = Command::new("wpctl")
        .args(["settings", INPUT_SETTING])
        .output()?;
    if !output.status.success() {
        bail!("MicStudio input policy is unavailable; install the WirePlumber policy first");
    }
    let text = String::from_utf8(output.stdout)?;
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix("Value: "))
        .context("WirePlumber did not return the input preference")?;
    let value = value.split(" (Saved:").next().unwrap_or(value);
    // wpctl displays strings as JSON, including the empty automatic preference.
    Ok(serde_json::from_str(value).unwrap_or_else(|_| value.to_string()))
}

pub fn select(name: &str) -> Result<()> {
    if !name.is_empty() && !devices()?.iter().any(|device| device.name == name) {
        bail!("Capture device is unavailable or is not an eligible physical input: {name}");
    }
    let value = serde_json::to_string(name)?;
    let output = Command::new("wpctl")
        .args(["settings", "--save", INPUT_SETTING, &value])
        .output()?;
    if !output.status.success() {
        bail!(
            "Failed to persist input selection: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn discovery_excludes_virtual_monitor_bluetooth_and_non_capture_nodes() {
        let node = |name: &str, class: &str, physical: bool| {
            let mut props = json!({"node.name": name, "media.class": class});
            if physical {
                props["device.id"] = json!(12);
            }
            json!({"type": "PipeWire:Interface:Node", "info": {"props": props}})
        };
        let data = json!([
            node("alsa_input.usb-mic", "Audio/Source", true),
            node("ghelper-audio", "Audio/Source", false),
            node("bluez_input.headset", "Audio/Source", true),
            node("monitor", "Audio/Source", false),
            node("alsa_output.speaker", "Audio/Sink", true),
            node("capture.ghelper-audio", "Stream/Input/Audio", false),
            node("alsa_input.usb-mic", "Audio/Source", true)
        ]);
        assert_eq!(
            parse_devices(&data),
            vec![InputDevice {
                name: "alsa_input.usb-mic".into(),
                description: "alsa_input.usb-mic".into()
            }]
        );
        assert!(parse_devices(&json!(null)).is_empty());
    }
}
