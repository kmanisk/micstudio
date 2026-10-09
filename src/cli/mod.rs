//! CLI Subcommands implementation.

use crate::config::CleanMicSettings;
use crate::ipc::{is_engine_running, send_command, IpcRequest, IpcResponse};
use crate::presets::PresetManager;
use crate::speaker::{SpeakerDsp, SpeakerVoicing};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::process::Command;

#[derive(Parser)]
#[command(name = "micstudio", about = "Minimal Native Rust Audio Studio & Voice Preset Manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Print current microphone and voice engine status
    Status,
    /// List all built-in and user voice presets
    List,
    /// Switch to the native clean broadcast microphone and stop optional FX
    Clean,
    /// Manage voice effects
    Voice {
        /// Preset name to apply, or 'off' to disable voice FX
        preset: String,
    },
    /// Manage custom presets
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },
    /// Manage live RNNoise suppression on clean microphone
    Noise {
        #[command(subcommand)]
        action: NoiseAction,
    },
    /// Manage live noise gate on clean microphone
    Gate {
        #[command(subcommand)]
        action: GateAction,
    },
    /// Manage live output gain on clean microphone
    Gain {
        #[command(subcommand)]
        action: GainAction,
    },
    /// Manage ASUS TUF DAX3 / Dolby Atmos Speaker DSP
    Speaker {
        #[command(subcommand)]
        action: SpeakerAction,
    },
    /// Alias for Speaker: Manage ASUS TUF DAX3 / Dolby Atmos Speaker DSP
    Dolby {
        #[command(subcommand)]
        action: SpeakerAction,
    },
    /// Manage live sidetone / hear my voice monitoring
    Hear {
        /// "on", "off", "status", or omit to toggle
        state: Option<String>,
    },
    /// Run diagnostic checks on the microphone and PipeWire audio stack
    Doctor,
    /// Internal: Run audio engine headlessly in the background
    #[command(hide = true)]
    Daemon {
        preset: String,
    },
}

#[derive(Subcommand)]
pub enum PresetAction {
    /// Save current or active settings to a new custom preset
    Save { name: String },
    /// Delete a user preset
    Delete { name: String },
}

#[derive(Subcommand)]
pub enum NoiseAction {
    /// Print noise suppression status
    Status,
    /// Enable RNNoise suppression
    On,
    /// Bypass RNNoise suppression
    Off,
    /// Set suppression blend percentage (0 to 100%)
    Blend { percentage: f32 },
}

#[derive(Subcommand)]
pub enum GateAction {
    /// Print noise gate status
    Status,
    /// Enable noise gate
    On,
    /// Disable noise gate
    Off,
    /// Set gate aggressiveness percentage (0 to 100%)
    Aggressiveness { percentage: f32 },
    /// Set silence attenuation (-60 to 0 dB)
    Attenuation { #[arg(allow_hyphen_values = true)] db: f32 },
    /// Set gate hangover hold time (0 to 1000 ms)
    Hangover { ms: u32 },
}

#[derive(Subcommand)]
pub enum GainAction {
    /// Print microphone output gain status
    Status,
    /// Set output gain in decibels (-24 to +12 dB)
    Set { #[arg(allow_hyphen_values = true)] db: f32 },
}

#[derive(Subcommand)]
pub enum SpeakerAction {
    /// Print current Speaker DSP status
    Status,
    /// Enable Speaker DSP processing
    On,
    /// Bypass Speaker DSP processing
    Off,
    /// Switch voicing profile (custom, dynamic, music, balanced)
    Profile { name: String },
}

pub fn handle_cli(cli: Cli) -> Result<()> {
    match cli.command {
        None => {
            // Launch Ratatui TUI
            crate::ui::run_tui()
        }
        Some(Commands::Status) => show_status(),
        Some(Commands::List) => list_presets(),
        Some(Commands::Clean) => set_clean(),
        Some(Commands::Voice { preset }) => {
            if preset.eq_ignore_ascii_case("off") {
                set_clean()
            } else {
                apply_voice(&preset)
            }
        }
        Some(Commands::Preset { action }) => match action {
            PresetAction::Save { name } => save_preset(&name),
            PresetAction::Delete { name } => delete_preset(&name),
        },
        Some(Commands::Noise { action }) => handle_noise(action),
        Some(Commands::Gate { action }) => handle_gate(action),
        Some(Commands::Gain { action }) => handle_gain(action),
        Some(Commands::Speaker { action }) | Some(Commands::Dolby { action }) => handle_speaker(action),
        Some(Commands::Hear { state }) => handle_hear(state),
        Some(Commands::Doctor) => run_doctor(),
        Some(Commands::Daemon { preset }) => run_daemon(&preset),
    }
}

pub fn show_status() -> Result<()> {
    println!("=== MicStudio Status ===");
    let def_source = get_default_source()?;
    println!("Default PipeWire Source : {}", def_source);

    let clean_node = get_node_id("ghelper-audio")?;
    println!("Native Clean Mic Node   : {} (ghelper-audio)", clean_node.unwrap_or_else(|| "Not Found".to_string()));

    let fx_node = get_node_id("ghelper-audio-fx")?;
    println!("Voice FX Mic Node       : {} (ghelper-audio-fx)", fx_node.unwrap_or_else(|| "Not Active".to_string()));

    // Clean Mic Settings
    let cfg = CleanMicSettings::load();
    println!("--- Clean Microphone Processing (ghelper-audio) ---");
    println!("RNNoise Suppression     : {}", if cfg.rnnoise_enabled { format!("ON (Blend: {:.0}%)", cfg.denoise_blend) } else { "BYPASSED".to_string() });
    println!("Noise Gate              : {}", if cfg.gate_enabled { format!("ON (Aggro: {:.0}%, Atten: {:.0} dB, Hangover: {} ms)", cfg.gate_aggressiveness, cfg.silence_attenuation, cfg.hangover_ms) } else { "OFF".to_string() });
    println!("Output Gain             : {:+.1} dB", cfg.output_gain_db);

    // Speaker DSP Settings
    if let Ok((voicing, enabled, route)) = SpeakerDsp::get_status() {
        println!("--- ASUS TUF DAX3 / Dolby Atmos Speaker DSP ---");
        println!("Speaker DSP State       : {}", if enabled { format!("ACTIVE ({})", voicing.id_str()) } else { "BYPASSED (Flat)".to_string() });
        println!("Physical Audio Route    : {}", route);
    }

    println!("--- Voice FX Engine ---");
    if is_engine_running() {
        if let Ok(IpcResponse::Status { active_preset, telemetry }) = send_command(&IpcRequest::GetStatus) {
            println!("On-Demand FX Engine     : ACTIVE");
            println!("Active Preset           : {}", active_preset);
            println!("Input Level             : {:.1} dB", telemetry.input_rms_db);
            println!("Output Level            : {:.1} dB (Peak: {:.2})", telemetry.output_rms_db, telemetry.peak_level);
            println!("VAD Voice Probability   : {:.0}%", telemetry.vad_probability * 100.0);
        } else {
            println!("On-Demand FX Engine     : RUNNING (Socket connected)");
        }
    } else {
        println!("On-Demand FX Engine     : INACTIVE (Zero CPU / RAM)");
    }
    Ok(())
}

fn handle_noise(action: NoiseAction) -> Result<()> {
    let mut cfg = CleanMicSettings::load();
    match action {
        NoiseAction::Status => {
            println!("RNNoise Suppression : {}", if cfg.rnnoise_enabled { "ON" } else { "BYPASS" });
            println!("Suppression Blend   : {:.0}%", cfg.denoise_blend);
        }
        NoiseAction::On => {
            cfg.rnnoise_enabled = true;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("RNNoise suppression enabled (Blend: {:.0}%).", cfg.denoise_blend);
        }
        NoiseAction::Off => {
            cfg.rnnoise_enabled = false;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("RNNoise suppression bypassed (Dry signal passthrough).");
        }
        NoiseAction::Blend { percentage } => {
            if !(0.0..=100.0).contains(&percentage) {
                anyhow::bail!("Blend percentage must be between 0 and 100%");
            }
            cfg.denoise_blend = percentage;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Suppression blend set to {:.0}%.", cfg.denoise_blend);
        }
    }
    Ok(())
}

fn handle_gate(action: GateAction) -> Result<()> {
    let mut cfg = CleanMicSettings::load();
    match action {
        GateAction::Status => {
            println!("Noise Gate          : {}", if cfg.gate_enabled { "ON" } else { "OFF" });
            println!("Gate Aggressiveness : {:.0}%", cfg.gate_aggressiveness);
            println!("Silence Attenuation : {:.0} dB", cfg.silence_attenuation);
            println!("Gate Hangover       : {} ms", cfg.hangover_ms);
        }
        GateAction::On => {
            cfg.gate_enabled = true;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Noise gate enabled (Aggressiveness: {:.0}%, Hangover: {} ms).", cfg.gate_aggressiveness, cfg.hangover_ms);
        }
        GateAction::Off => {
            cfg.gate_enabled = false;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Noise gate disabled.");
        }
        GateAction::Aggressiveness { percentage } => {
            if !(0.0..=100.0).contains(&percentage) {
                anyhow::bail!("Aggressiveness percentage must be between 0 and 100%");
            }
            cfg.gate_aggressiveness = percentage;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Gate aggressiveness set to {:.0}%.", cfg.gate_aggressiveness);
        }
        GateAction::Attenuation { db } => {
            if !(-60.0..=0.0).contains(&db) {
                anyhow::bail!("Silence attenuation must be between -60 and 0 dB");
            }
            cfg.silence_attenuation = db;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Silence attenuation set to {:.0} dB.", cfg.silence_attenuation);
        }
        GateAction::Hangover { ms } => {
            if ms > 1000 {
                anyhow::bail!("Hangover must be between 0 and 1000 ms");
            }
            cfg.hangover_ms = ms;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Gate hangover set to {} ms.", cfg.hangover_ms);
        }
    }
    Ok(())
}

fn handle_gain(action: GainAction) -> Result<()> {
    let mut cfg = CleanMicSettings::load();
    match action {
        GainAction::Status => {
            println!("Microphone Output Gain : {:+.1} dB", cfg.output_gain_db);
        }
        GainAction::Set { db } => {
            if !(-24.0..=12.0).contains(&db) {
                anyhow::bail!("Output gain must be between -24.0 and +12.0 dB");
            }
            cfg.output_gain_db = db;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Microphone output gain set to {:+.1} dB.", cfg.output_gain_db);
        }
    }
    Ok(())
}

fn handle_hear(state: Option<String>) -> Result<()> {
    let mut cfg = CleanMicSettings::load();
    let action = state.as_deref().unwrap_or("toggle").to_lowercase();
    match action.as_str() {
        "on" | "enable" | "true" | "1" => {
            cfg.hear_voice = true;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Microphone Monitor (Hear My Voice): ENABLED (sidetone active).");
        }
        "off" | "disable" | "false" | "0" => {
            cfg.hear_voice = false;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Microphone Monitor (Hear My Voice): DISABLED.");
        }
        "status" => {
            println!("Microphone Monitor (Hear My Voice): {}", if cfg.hear_voice { "ENABLED" } else { "DISABLED" });
        }
        "toggle" => {
            cfg.hear_voice = !cfg.hear_voice;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!("Microphone Monitor (Hear My Voice): {}.", if cfg.hear_voice { "ENABLED" } else { "DISABLED" });
        }
        other => {
            anyhow::bail!("Unknown state '{}'. Usage: mic hear [on|off|status|toggle]", other);
        }
    }
    Ok(())
}

fn handle_speaker(action: SpeakerAction) -> Result<()> {
    match action {
        SpeakerAction::Status => {
            let (voicing, enabled, route) = SpeakerDsp::get_status()?;
            println!("ASUS TUF DAX3 / Dolby Speaker DSP:");
            println!("  State   : {}", if enabled { "Enabled (Active)" } else { "Disabled (Bypassed)" });
            println!("  Voicing : {} ({})", voicing.display_name(), voicing.id_str());
            println!("  Route   : {}", route);
        }
        SpeakerAction::On => {
            SpeakerDsp::set_enabled(true)?;
            println!("Enabled Speaker DSP.");
        }
        SpeakerAction::Off => {
            SpeakerDsp::set_enabled(false)?;
            println!("Bypassed Speaker DSP.");
        }
        SpeakerAction::Profile { name } => {
            if let Some(v) = SpeakerVoicing::from_str(&name) {
                SpeakerDsp::apply_voicing(v)?;
                println!("Applied speaker profile: {}", v.display_name());
            } else {
                anyhow::bail!("Unknown profile '{}'. Choose: custom, dynamic, music, balanced", name);
            }
        }
    }
    Ok(())
}

pub fn list_presets() -> Result<()> {
    let mgr = PresetManager::new()?;
    println!("=== MicStudio Voice Presets ===");
    println!("\nFactory Presets:");
    for p in mgr.factory_presets() {
        println!("  • {:<16} - {}", p.name, p.description);
    }
    if !mgr.user_presets().is_empty() {
        println!("\nUser Presets:");
        for p in mgr.user_presets() {
            println!("  ★ {:<16} - {}", p.name, p.description);
        }
    }
    Ok(())
}

pub fn set_clean_internal(verbose: bool) -> Result<String> {
    if is_engine_running() {
        let _ = send_command(&IpcRequest::StopEngine);
    }
    let mut msg = String::new();
    if let Some(id) = get_node_id("ghelper-audio")? {
        Command::new("wpctl")
            .args(["set-default", &id])
            .output()
            .context("Failed to run wpctl set-default")?;
        msg = format!("Clean Mic (ghelper-audio) [Node ID: {}]", id);
        if verbose {
            println!("Switched default source to Native Clean Mic (ghelper-audio) [Node ID: {}]", id);
            println!("Voice FX engine stopped (Zero idle resources).");
        }

        // Clean up any stale recording links from voice FX mic (ghelper-audio-fx)
        disconnect_recording_links_from("ghelper-audio-fx:capture_MONO");

        // Sync hear voice monitor back to clean mic if active
        let cfg = CleanMicSettings::load();
        if cfg.hear_voice {
            CleanMicSettings::sync_hear_voice_with_source(true, false);
        }
    } else if verbose {
        eprintln!("Warning: ghelper-audio node not found in PipeWire graph.");
    }
    Ok(msg)
}

pub fn set_clean() -> Result<()> {
    set_clean_internal(true)?;
    Ok(())
}

pub fn set_clean_silent() -> Result<String> {
    set_clean_internal(false)
}

pub fn apply_voice_internal(preset_name: &str, verbose: bool) -> Result<String> {
    let mut mgr = PresetManager::new()?;
    let preset = mgr.find_preset(preset_name)
        .with_context(|| format!("Preset '{}' not found. Run 'micstudio list' to view available presets.", preset_name))?;

    let mut status_msg = String::new();
    if is_engine_running() {
        match send_command(&IpcRequest::ApplyPreset(preset.clone()))? {
            IpcResponse::Success => {
                mgr.set_active_preset(&preset.name)?;
                status_msg = format!("Preset '{}' active", preset.name);
                if verbose {
                    println!("Applied preset '{}' to active engine.", preset.name);
                }
            }
            IpcResponse::Error(e) => {
                if verbose {
                    eprintln!("Engine error: {}", e);
                }
            }
            _ => {}
        }
    } else {
        // Start engine daemon in background
        mgr.set_active_preset(&preset.name)?;
        let exe = std::env::current_exe()?;
        Command::new(exe)
            .args(["daemon", &preset.name])
            .spawn()
            .context("Failed to spawn micstudio background engine")?;

        // Wait up to 1 second for ghelper-audio-fx to appear
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Ok(Some(_)) = get_node_id("ghelper-audio-fx") {
                break;
            }
        }
        status_msg = format!("Voice FX engine active with preset '{}'", preset.name);
        if verbose {
            println!("Started Voice FX engine with preset '{}'.", preset.name);
        }
    }

    // Set default source to ghelper-audio-fx
    if let Some(id) = get_node_id("ghelper-audio-fx")? {
        Command::new("wpctl")
            .args(["set-default", &id])
            .output()
            .ok();
        if verbose {
            println!("Default Source set to Voice FX (ghelper-audio-fx) [Node ID: {}]", id);
        }

        // Clean up any stale recording links from clean mic (ghelper-audio) so apps don't get double input
        disconnect_recording_links_from("ghelper-audio:capture_MONO");

        // Sync hear voice monitor if enabled so sidetone monitors the FX source
        let cfg = CleanMicSettings::load();
        if cfg.hear_voice {
            CleanMicSettings::sync_hear_voice_with_source(true, true);
        }
    }
    Ok(status_msg)
}

fn disconnect_recording_links_from(source_port: &str) {
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
                        // Disconnect if target is an app capture port (e.g. input_MONO, input_FL)
                        if target_clean.contains("input_") {
                            let _ = Command::new("pw-link").args(["-d", source_port, target_clean]).output();
                        }
                    }
                }
            } else {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    cur_port = parts[1].to_string();
                }
            }
        }
    }
}

pub fn apply_voice(preset_name: &str) -> Result<()> {
    apply_voice_internal(preset_name, true)?;
    Ok(())
}

pub fn apply_voice_silent(preset_name: &str) -> Result<String> {
    apply_voice_internal(preset_name, false)
}

fn save_preset(name: &str) -> Result<()> {
    let mut mgr = PresetManager::new()?;
    let current_name = mgr.active_preset_name().to_string();
    let base = mgr.find_preset(&current_name).unwrap_or_default();
    let mut custom = base.clone();
    custom.name = name.to_string();
    custom.description = format!("User preset based on {}", base.name);
    mgr.save_user_preset(custom)?;
    println!("Saved custom preset '{}'.", name);
    Ok(())
}

fn delete_preset(name: &str) -> Result<()> {
    let mut mgr = PresetManager::new()?;
    if mgr.delete_user_preset(name)? {
        println!("Deleted user preset '{}'.", name);
    } else {
        eprintln!("User preset '{}' not found.", name);
    }
    Ok(())
}

pub fn run_daemon(preset_name: &str) -> Result<()> {
    let mgr = PresetManager::new()?;
    let preset = mgr.find_preset(preset_name).unwrap_or_default();
    let stop_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    crate::engine::pipewire_engine::run_engine(preset, stop_signal)
}

pub fn run_doctor() -> Result<()> {
    println!("========================================================");
    println!("       MicStudio Audio Stack & Hardware Doctor          ");
    println!("========================================================");

    // 1. Hardware Mic Check
    print!(":: 1. Realtek ALC256 Hardware Mic: ");
    let raw_mic = get_node_id("alsa_input.pci-0000_00_1f.3.analog-stereo")?;
    if let Some(id) = raw_mic {
        println!("[✓ PASS] Node ID {}", id);
    } else {
        println!("[✗ FAIL] Raw ALC256 mic node not found");
    }

    // 2. Clean Mic Pipeline Check
    print!(":: 2. Clean Broadcast Mic (ghelper-audio): ");
    let clean = get_node_id("ghelper-audio")?;
    if let Some(id) = clean {
        println!("[✓ PASS] Node ID {} (Active, High Priority)", id);
    } else {
        println!("[✗ FAIL] Clean mic node missing (Check 40-mic.conf)");
    }

    // 3. Fallback Pitch FX Mic Check
    print!(":: 3. Static Pitch FX (ghelper-audio-fx): ");
    let fx = get_node_id("ghelper-audio-fx")?;
    if let Some(id) = fx {
        println!("[✓ PASS] Node ID {}", id);
    } else {
        println!("[– IDLE] Ready for on-demand activation");
    }

    // 4. Default Source
    print!(":: 4. Default Recording Source: ");
    let def = get_default_source()?;
    println!("{}", def);

    // 5. Engine Status
    print!(":: 5. Voice FX Engine IPC: ");
    if is_engine_running() {
        println!("[✓ ACTIVE] Socket connected");
    } else {
        println!("[– IDLE] Stopped (Zero CPU / RAM consumed)");
    }

    // 6. Dolby Speaker DSP Check
    print!(":: 6. ASUS TUF DAX3 / Dolby Speaker DSP: ");
    if let Ok((voicing, enabled, _)) = SpeakerDsp::get_status() {
        if enabled {
            println!("[✓ PASS] Active ({})", voicing.id_str());
        } else {
            println!("[– BYPASS] Disabled by user");
        }
    } else {
        println!("[? UNKNOWN]");
    }

    println!("========================================================");
    Ok(())
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

fn get_default_source() -> Result<String> {
    let output = Command::new("wpctl")
        .args(["inspect", "@DEFAULT_AUDIO_SOURCE@"])
        .output()
        .context("Failed to run wpctl")?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("node.description =") {
            if let Some((_, val)) = line.split_once('=') {
                let desc = val.trim().trim_matches('"').to_string();
                if desc.eq_ignore_ascii_case("Dusky Mic") {
                    return Ok("MicStudio Clean".to_string());
                }
                return Ok(desc);
            }
        }
    }
    Ok("MicStudio Clean".to_string())
}
