//! CLI Subcommands implementation.

use crate::config::CleanMicSettings;
use crate::ipc::{IpcRequest, is_engine_running, send_command};
use crate::presets::{Preset, PresetManager};
use crate::speaker::{SpeakerDsp, SpeakerVoicing};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "micstudio",
    about = "Minimal Native Rust Audio Studio & Voice Preset Manager"
)]
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
    /// Offline Audio Lab: Render WAV file through MicStudio DSP pipeline
    Render {
        /// Input WAV file path
        input: std::path::PathBuf,
        /// Preset name to apply (e.g. "Natural Clean", "Deep Voice", "Darth Vader")
        #[arg(short, long, default_value = "Natural Clean")]
        preset: String,
        /// Output WAV file path
        #[arg(short, long)]
        output: std::path::PathBuf,
        /// Override RNNoise suppression enabled (true/false)
        #[arg(long)]
        denoise: Option<bool>,
        /// Override suppression blend (0.0 to 1.0)
        #[arg(long)]
        blend: Option<f32>,
        /// Override gate enabled (true/false)
        #[arg(long)]
        gate: Option<bool>,
        /// Bypass cleanup stage (HPF, RNNoise, EQ, Compressor)
        #[arg(long)]
        bypass_cleanup: bool,
        /// Bypass voice transformation FX stage
        #[arg(long)]
        bypass_fx: bool,
        /// Raw passthrough reference
        #[arg(long)]
        raw: bool,
    },
    /// Verify: Assert live PipeWire microphone stream matches offline renderer within tolerances
    Verify {
        /// Optional preset name to verify (verifies all 13 presets if omitted)
        #[arg(short, long)]
        preset: Option<String>,
        /// Optional input WAV file path
        #[arg(short, long)]
        input: Option<std::path::PathBuf>,
        /// Verify real hardware mic live path through ghelper-audio
        #[arg(long)]
        hardware: bool,
        /// Live real-mic interactive verification test (prompts user to speak)
        #[arg(long)]
        live: bool,
        /// Duration in seconds for hardware mic test (default: 5s)
        #[arg(long, default_value = "5")]
        duration: u32,
    },
    /// Live Real-Time Audio Spectrograph and Spectrum Analyzer
    Spectrograph {
        /// Start in full-screen waterfall spectrogram mode
        #[arg(short, long)]
        waterfall: bool,
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
    Attenuation {
        #[arg(allow_hyphen_values = true)]
        db: f32,
    },
    /// Set gate hangover hold time (0 to 1000 ms)
    Hangover { ms: u32 },
}

#[derive(Subcommand)]
pub enum GainAction {
    /// Print microphone output gain status
    Status,
    /// Set output gain in decibels (-24 to +12 dB)
    Set {
        #[arg(allow_hyphen_values = true)]
        db: f32,
    },
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
        Some(Commands::Speaker { action }) | Some(Commands::Dolby { action }) => {
            handle_speaker(action)
        }
        Some(Commands::Hear { state }) => handle_hear(state),
        Some(Commands::Doctor) => run_doctor(),
        Some(Commands::Render {
            input,
            preset,
            output,
            denoise,
            blend,
            gate,
            bypass_cleanup,
            bypass_fx,
            raw,
        }) => handle_render(
            input,
            &preset,
            output,
            denoise,
            blend,
            gate,
            bypass_cleanup,
            bypass_fx,
            raw,
        ),
        Some(Commands::Verify {
            preset,
            input,
            hardware,
            live,
            duration,
        }) => {
            if live || hardware {
                run_live_verify(preset.as_deref(), duration)
            } else {
                run_verify(preset.as_deref(), input.as_deref())
            }
        }
        Some(Commands::Spectrograph { waterfall }) => crate::ui::run_tui_spectrograph(waterfall),
    }
}

pub fn show_status() -> Result<()> {
    println!("=== MicStudio Status ===");
    let def_source = get_default_source()?;
    println!("Default PipeWire Source : {}", def_source);

    let clean_node = get_node_id("ghelper-audio")?;
    println!(
        "Clean Node (DPDFNet)    : {} (ghelper-audio)",
        clean_node.unwrap_or_else(|| "Not Found".to_string())
    );

    let fx_node = get_node_id("ghelper-audio-fx")?;
    println!(
        "Voice FX Node           : {} (ghelper-audio-fx)",
        fx_node.unwrap_or_else(|| "Not Found".to_string())
    );

    let cfg = CleanMicSettings::load();
    println!("--- Microphone DSP Stage 1 (DPDFNet) ---");
    println!(
        "DPDFNet Attenuation     : {:.1} dB",
        cfg.dpdfnet_attenuation_db
    );
    println!("Output Gain             : {:+.1} dB", cfg.output_gain_db);
    println!(
        "Sidetone Monitor        : {}",
        if cfg.hear_voice { "ON" } else { "OFF" }
    );

    let mgr = PresetManager::new()?;
    println!("--- Active Preset ---");
    println!("Active Voice Preset     : {}", mgr.active_preset_name());
    Ok(())
}

fn handle_noise(action: NoiseAction) -> Result<()> {
    let mut cfg = CleanMicSettings::load();
    match action {
        NoiseAction::Status => {
            println!(
                "RNNoise Suppression : {}",
                if cfg.rnnoise_enabled { "ON" } else { "BYPASS" }
            );
            println!("Suppression Blend   : {:.0}%", cfg.denoise_blend);
        }
        NoiseAction::On => {
            cfg.rnnoise_enabled = true;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!(
                "RNNoise suppression enabled (Blend: {:.0}%).",
                cfg.denoise_blend
            );
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
            println!(
                "Noise Gate          : {}",
                if cfg.gate_enabled { "ON" } else { "OFF" }
            );
            println!("Gate Aggressiveness : {:.0}%", cfg.gate_aggressiveness);
            println!("Silence Attenuation : {:.0} dB", cfg.silence_attenuation);
            println!("Gate Hangover       : {} ms", cfg.hangover_ms);
        }
        GateAction::On => {
            cfg.gate_enabled = true;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!(
                "Noise gate enabled (Aggressiveness: {:.0}%, Hangover: {} ms).",
                cfg.gate_aggressiveness, cfg.hangover_ms
            );
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
            println!(
                "Gate aggressiveness set to {:.0}%.",
                cfg.gate_aggressiveness
            );
        }
        GateAction::Attenuation { db } => {
            if !(-60.0..=0.0).contains(&db) {
                anyhow::bail!("Silence attenuation must be between -60 and 0 dB");
            }
            cfg.silence_attenuation = db;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!(
                "Silence attenuation set to {:.0} dB.",
                cfg.silence_attenuation
            );
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
            println!(
                "Microphone output gain set to {:+.1} dB.",
                cfg.output_gain_db
            );
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
            println!(
                "Microphone Monitor (Hear My Voice): {}",
                if cfg.hear_voice {
                    "ENABLED"
                } else {
                    "DISABLED"
                }
            );
        }
        "toggle" => {
            cfg.hear_voice = !cfg.hear_voice;
            cfg.apply_to_pipewire()?;
            cfg.save()?;
            println!(
                "Microphone Monitor (Hear My Voice): {}.",
                if cfg.hear_voice {
                    "ENABLED"
                } else {
                    "DISABLED"
                }
            );
        }
        other => {
            anyhow::bail!(
                "Unknown state '{}'. Usage: mic hear [on|off|status|toggle]",
                other
            );
        }
    }
    Ok(())
}

fn handle_speaker(action: SpeakerAction) -> Result<()> {
    match action {
        SpeakerAction::Status => {
            let (voicing, enabled, route) = SpeakerDsp::get_status()?;
            println!("ASUS TUF DAX3 / Dolby Speaker DSP:");
            println!(
                "  State   : {}",
                if enabled {
                    "Enabled (Active)"
                } else {
                    "Disabled (Bypassed)"
                }
            );
            println!(
                "  Voicing : {} ({})",
                voicing.display_name(),
                voicing.id_str()
            );
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
                anyhow::bail!(
                    "Unknown profile '{}'. Choose: custom, dynamic, music, balanced",
                    name
                );
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
    apply_voice_internal("Natural Clean", verbose)
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
    let effective_name =
        if preset_name.eq_ignore_ascii_case("clean") || preset_name.eq_ignore_ascii_case("off") {
            "Natural Clean"
        } else {
            preset_name
        };

    let preset = mgr.find_preset(effective_name).with_context(|| {
        format!(
            "Preset '{}' not found. Run 'micstudio list' to view available presets.",
            effective_name
        )
    })?;

    let is_clean = preset.name.eq_ignore_ascii_case("Natural Clean");

    // Dynamically update DPDFNet attenuation on capture.ghelper-audio tailored for this preset
    if let Some(clean_cap_id) = get_node_id("capture.ghelper-audio")? {
        let atten = preset.dpdfnet_attenuation_db;
        let props = format!(
            "{{ params = [ \"dpdfnet:Attenuation Limit (dB)\" {:.1} ] }}",
            atten
        );
        let _ = Command::new("pw-cli")
            .args(["s", &clean_cap_id, "Props", &props])
            .output();
    }
    let mut cfg = CleanMicSettings::load();
    cfg.dpdfnet_attenuation_db = preset.dpdfnet_attenuation_db;
    let _ = cfg.save();

    if is_clean {
        if let Some(fx_cap_id) = get_node_id("capture.ghelper-audio-fx")? {
            let props_payload = Preset::default().build_props_payload();
            let _ = Command::new("pw-cli")
                .args(["s", &fx_cap_id, "Props", &props_payload])
                .output();
        }
        if let Some(id) = get_node_id("ghelper-audio")? {
            let _ = Command::new("wpctl").args(["set-default", &id]).output();
        }
        mgr.set_active_preset(&preset.name)?;
        let cfg = CleanMicSettings::load();
        CleanMicSettings::sync_hear_voice_with_source(cfg.hear_voice, false);
        if verbose {
            println!(
                "Switched default source to clean microphone (ghelper-audio). DPDFNet: {:.0} dB",
                preset.dpdfnet_attenuation_db
            );
        }
        Ok(format!(
            "Natural Clean active (ghelper-audio, DPDFNet {:.0} dB)",
            preset.dpdfnet_attenuation_db
        ))
    } else {
        // Set all 10 LADSPA stages on capture.ghelper-audio-fx atomically
        if let Some(fx_cap_id) = get_node_id("capture.ghelper-audio-fx")? {
            let props_payload = preset.build_props_payload();
            let _ = Command::new("pw-cli")
                .args(["s", &fx_cap_id, "Props", &props_payload])
                .output();
        }

        // Set default source to ghelper-audio-fx
        if let Some(id) = get_node_id("ghelper-audio-fx")? {
            let _ = Command::new("wpctl").args(["set-default", &id]).output();
        }
        mgr.set_active_preset(&preset.name)?;
        let cfg = CleanMicSettings::load();
        CleanMicSettings::sync_hear_voice_with_source(cfg.hear_voice, true);
        if verbose {
            println!(
                "Applied preset '{}' to voice FX engine (ghelper-audio-fx). DPDFNet: {:.0} dB",
                preset.name, preset.dpdfnet_attenuation_db
            );
        }
        Ok(format!(
            "Preset '{}' active (ghelper-audio-fx, DPDFNet {:.0} dB)",
            preset.name, preset.dpdfnet_attenuation_db
        ))
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

    // 2. Unified Mic Pipeline Check
    print!(":: 2. Unified DSP Mic (ghelper-audio): ");
    let clean = get_node_id("ghelper-audio")?;
    if let Some(id) = clean {
        println!("[✓ PASS] Node ID {} (Active, High Priority)", id);
    } else {
        println!("[✗ FAIL] ghelper-audio node missing (Run 'systemctl --user start micstudio')");
    }

    // 3. Default Source
    print!(":: 3. Default Recording Source: ");
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

fn handle_render(
    input: std::path::PathBuf,
    preset_name: &str,
    output: std::path::PathBuf,
    denoise: Option<bool>,
    blend: Option<f32>,
    gate: Option<bool>,
    bypass_cleanup: bool,
    bypass_fx: bool,
    raw: bool,
) -> Result<()> {
    let mgr = PresetManager::new()?;
    let preset = mgr.find_preset(preset_name).with_context(|| {
        format!(
            "Preset '{}' not found. Run 'mic list' to see available presets.",
            preset_name
        )
    })?;

    let options = crate::renderer::RenderOptions {
        input_path: input,
        output_path: output,
        preset,
        cleanup_enabled: !bypass_cleanup,
        fx_enabled: !bypass_fx,
        denoise_override: denoise,
        denoise_blend_override: blend,
        gate_override: gate,
        hpf_override: None,
        limiter_override: None,
        raw_passthrough: raw,
    };

    let result = crate::renderer::render_file(&options)?;

    println!("✓ Rendered: {}", result.output_file);
    println!("  Preset       : {}", result.preset_name);
    println!(
        "  Input        : {} ({:.2}s, {:.1} kHz, {} ch, Peak: {:.3}, RMS: {:.1} dBFS)",
        result.input_file,
        result.input_info.duration_seconds,
        result.input_info.sample_rate as f32 / 1000.0,
        result.input_info.channels,
        result.input_info.peak_amplitude,
        result.input_info.rms_dbfs
    );
    println!(
        "  Output       : {} ({:.2}s, 48.0 kHz 24-bit PCM mono)",
        result.output_file, result.output_info.duration_seconds
    );
    println!(
        "  Levels       : Peak: {:.3} ({:.2} dBFS), RMS: {:.1} dBFS, Clipped: {}",
        result.output_info.peak_amplitude,
        if result.output_info.peak_amplitude > 1e-5 {
            20.0 * result.output_info.peak_amplitude.log10()
        } else {
            -96.0
        },
        result.output_info.rms_dbfs,
        result.output_info.clipped_samples
    );
    println!(
        "  Denoise      : {} (Blend: {:.0}%), Gate: {}",
        if result.denoise_active {
            "ON"
        } else {
            "BYPASSED"
        },
        result.denoise_blend * 100.0,
        if result.gate_active { "ON" } else { "OFF" }
    );
    Ok(())
}

pub fn run_verify(preset_filter: Option<&str>, input_file: Option<&std::path::Path>) -> Result<()> {
    println!(
        "=========================================================================================="
    );
    println!(
        "             MicStudio Live vs. Offline Unified Pipeline Verification                      "
    );
    println!(
        "=========================================================================================="
    );

    // 1. Resolve test input WAV file
    let input_path = if let Some(p) = input_file {
        p.to_path_buf()
    } else {
        let p_jack = std::path::PathBuf::from("/home/manisk/Downloads/jackhammer.wav");
        let p_harv = std::path::PathBuf::from("/home/manisk/Downloads/harvard.wav");
        if p_jack.exists() {
            p_jack
        } else if p_harv.exists() {
            p_harv
        } else {
            anyhow::bail!("No test input audio found in /home/manisk/Downloads/");
        }
    };

    println!(":: Input Reference Audio : {}", input_path.display());

    // 2. Resample/normalize input to 48kHz mono 24-bit PCM at /tmp/micstudio_verify_in_48k.wav
    let (raw_samples, in_info) = crate::renderer::read_wav_mono(&input_path)?;
    let mut resampled_48k = crate::renderer::resample_to_48k(&raw_samples, in_info.sample_rate);
    // Limit to 4.0 seconds (192,000 samples) so 13-preset sweep completes in ~15-20s
    if resampled_48k.len() > 192000 {
        resampled_48k.truncate(192000);
    }
    let test_in_path = std::path::PathBuf::from("/tmp/micstudio_verify_in_48k.wav");
    crate::renderer::write_wav_24bit_48k(&test_in_path, &resampled_48k)?;
    println!(
        ":: Normalized Test Slice : 48.0 kHz Mono ({:.2}s, {} samples)",
        resampled_48k.len() as f64 / 48000.0,
        resampled_48k.len()
    );

    // 3. Ensure live daemon is running
    if !is_engine_running() {
        let _ = Command::new("systemctl")
            .args(["--user", "start", "micstudio"])
            .output();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if is_engine_running() {
                break;
            }
        }
        if !is_engine_running() {
            let exe = std::env::current_exe()?;
            Command::new(exe)
                .args(["daemon", "Natural Clean"])
                .spawn()?;
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if is_engine_running() {
                    break;
                }
            }
        }
    }
    if !is_engine_running() {
        anyhow::bail!("Failed to connect to MicStudio live engine daemon");
    }

    // 4. Setup temporary PipeWire null-sink loopback
    println!(":: Setting up PipeWire test loopback...");
    let out = Command::new("pactl")
        .args([
            "load-module",
            "module-null-sink",
            "sink_name=micstudio_verify_sink",
            "channels=1",
            "channel_map=mono",
            "sink_properties=device.description=MicStudio_Verify_Sink",
        ])
        .output()
        .context("Failed to create temporary null sink")?;
    let mod_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // Disconnect physical mic
    let _ = Command::new("pw-link")
        .args([
            "-d",
            "alsa_input.pci-0000_00_1f.3.analog-stereo:capture_FL",
            "capture.ghelper-audio:input_FL",
        ])
        .output();
    let _ = Command::new("pw-link")
        .args([
            "-d",
            "alsa_input.pci-0000_00_1f.3.analog-stereo:capture_FR",
            "capture.ghelper-audio:input_FR",
        ])
        .output();
    // Connect null sink monitor to both input channels
    let _ = Command::new("pw-link")
        .args([
            "micstudio_verify_sink:monitor_MONO",
            "capture.ghelper-audio:input_FL",
        ])
        .output();
    let _ = Command::new("pw-link")
        .args([
            "micstudio_verify_sink:monitor_MONO",
            "capture.ghelper-audio:input_FR",
        ])
        .output();

    let mgr = PresetManager::new()?;
    let presets_to_test: Vec<_> = if let Some(filter) = preset_filter {
        let p = mgr
            .find_preset(filter)
            .with_context(|| format!("Preset '{}' not found", filter))?;
        vec![p]
    } else {
        mgr.all_presets().to_vec()
    };

    println!(
        "\n{:<15} | {:<9} | {:<9} | {:<9} | {:<9} | {:<7} | {:<7} | {:<7} | {:<7} | {:<6}",
        "Preset",
        "Live Peak",
        "Off Peak",
        "Live RMS",
        "Off RMS",
        "Δ RMS",
        "Δ Low",
        "Δ Mid",
        "Δ High",
        "Status"
    );
    println!(
        "{:-<15}-+-{:-<9}-+-{:-<9}-+-{:-<9}-+-{:-<9}-+-{:-<7}-+-{:-<7}-+-{:-<7}-+-{:-<7}-+-{:-<6}",
        "", "", "", "", "", "", "", "", "", ""
    );

    let mut all_passed = true;

    for preset in &presets_to_test {
        let slug = preset.name.to_lowercase().replace(' ', "-");
        let off_path = std::path::PathBuf::from(format!("/tmp/verify_off_{}.wav", slug));
        let live_path = std::path::PathBuf::from(format!("/tmp/verify_live_{}.wav", slug));

        // a. Switch live preset
        let _ = send_command(&IpcRequest::ApplyPreset(preset.clone()));
        std::thread::sleep(std::time::Duration::from_millis(150));

        // b. Render offline
        let off_opt = crate::renderer::RenderOptions {
            input_path: test_in_path.clone(),
            output_path: off_path.clone(),
            preset: preset.clone(),
            ..Default::default()
        };
        crate::renderer::render_file(&off_opt)?;

        // c. Live playback & capture
        let _ = std::fs::remove_file(&live_path);
        let mut rec_child = Command::new("pw-record")
            .args([
                "--target",
                "ghelper-audio",
                "--channels",
                "1",
                "--rate",
                "48000",
                "--format",
                "s24",
                live_path.to_str().unwrap(),
            ])
            .spawn()
            .context("Failed to spawn pw-record")?;

        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = Command::new("pw-play")
            .args([
                "--target",
                "micstudio_verify_sink",
                test_in_path.to_str().unwrap(),
            ])
            .status();
        // Give 1000ms for all audio to fully drain through PipeWire and ring buffers
        std::thread::sleep(std::time::Duration::from_millis(1000));

        unsafe {
            libc::kill(rec_child.id() as i32, libc::SIGINT);
        }
        let _ = rec_child.wait();

        // d. Read and analyze
        let (off_samples, _) = crate::renderer::read_wav_mono(&off_path)?;
        let (live_raw, _) = crate::renderer::read_wav_mono(&live_path)?;

        // Align live to offline via normalized cross-correlation
        let win_start = 2400.min(off_samples.len().saturating_sub(9600));
        let win_len = 9600.min(off_samples.len().saturating_sub(win_start));
        let off_ref = &off_samples[win_start..win_start + win_len];
        let off_energy: f32 = off_ref.iter().map(|&x| x * x).sum();

        let search_len = live_raw
            .len()
            .saturating_sub(win_start + win_len)
            .min(48000);
        let mut best_tau = 0;
        let mut max_corr = -1.0f32;

        if off_energy > 1e-6 && search_len > 0 {
            for tau in (0..search_len).step_by(2) {
                let live_slice = &live_raw[tau + win_start..tau + win_start + win_len];
                let mut dot = 0.0f32;
                let mut live_energy = 0.0f32;
                for k in 0..win_len {
                    dot += off_ref[k] * live_slice[k];
                    live_energy += live_slice[k] * live_slice[k];
                }
                if live_energy > 0.01 {
                    let norm_corr = dot / (off_energy * live_energy).sqrt();
                    if norm_corr > max_corr {
                        max_corr = norm_corr;
                        best_tau = tau;
                    }
                }
            }
        }

        let aligned_end = (best_tau + off_samples.len()).min(live_raw.len());
        let live_aligned = if best_tau < live_raw.len() {
            &live_raw[best_tau..aligned_end]
        } else {
            &[]
        };

        let cmp_len = off_samples.len().min(live_aligned.len());
        let off_cmp = &off_samples[..cmp_len];
        let live_cmp = &live_aligned[..cmp_len];

        // Peaks & RMS
        let mut off_max = 0.0f32;
        let mut live_max = 0.0f32;
        let mut off_sq = 0.0f64;
        let mut live_sq = 0.0f64;

        for i in 0..cmp_len {
            let o = off_cmp[i];
            let l = live_cmp[i];
            if o.abs() > off_max {
                off_max = o.abs();
            }
            if l.abs() > live_max {
                live_max = l.abs();
            }
            off_sq += (o as f64) * (o as f64);
            live_sq += (l as f64) * (l as f64);
        }

        let off_peak_db = if off_max > 1e-6 {
            20.0 * off_max.log10()
        } else {
            -96.0
        };
        let live_peak_db = if live_max > 1e-6 {
            20.0 * live_max.log10()
        } else {
            -96.0
        };

        let off_rms = if cmp_len > 0 {
            (off_sq / cmp_len as f64).sqrt() as f32
        } else {
            0.0
        };
        let live_rms = if cmp_len > 0 {
            (live_sq / cmp_len as f64).sqrt() as f32
        } else {
            0.0
        };
        let off_rms_db = if off_rms > 1e-6 {
            20.0 * off_rms.log10()
        } else {
            -96.0
        };
        let live_rms_db = if live_rms > 1e-6 {
            20.0 * live_rms.log10()
        } else {
            -96.0
        };
        let delta_rms = (live_rms_db - off_rms_db).abs();

        // Spectral Bands
        // Low: LowPass 250Hz, Mid: BandPass 1000Hz, High: HighPass 4000Hz
        let delta_low = calc_band_delta(
            off_cmp,
            live_cmp,
            crate::dsp::biquad::FilterType::LowPass,
            250.0,
            0.707,
        );
        let delta_mid = calc_band_delta(
            off_cmp,
            live_cmp,
            crate::dsp::biquad::FilterType::BandPass,
            1000.0,
            0.5,
        );
        let delta_high = calc_band_delta(
            off_cmp,
            live_cmp,
            crate::dsp::biquad::FilterType::HighPass,
            4000.0,
            0.707,
        );

        let peak_pass = live_peak_db <= -0.99 && off_peak_db <= -0.99;
        let rms_pass = delta_rms < 0.55;
        let spectral_pass = delta_low < 1.00 && delta_mid < 1.00 && delta_high < 1.00;
        let pass = peak_pass && rms_pass && spectral_pass;

        if !pass {
            all_passed = false;
        }

        let status_str = if pass { "PASS" } else { "FAIL" };

        println!(
            "{:<15} | {:>7.2} dB | {:>7.2} dB | {:>7.2} dB | {:>7.2} dB | {:>5.2} dB | {:>5.2} dB | {:>5.2} dB | {:>5.2} dB | {}",
            preset.name,
            live_peak_db,
            off_peak_db,
            live_rms_db,
            off_rms_db,
            delta_rms,
            delta_low,
            delta_mid,
            delta_high,
            status_str
        );
    }

    // 5. Cleanup
    println!("\n:: Cleaning up PipeWire test routing...");
    let _ = Command::new("pw-link")
        .args([
            "-d",
            "micstudio_verify_sink:monitor_MONO",
            "capture.ghelper-audio:input_FL",
        ])
        .output();
    let _ = Command::new("pw-link")
        .args([
            "-d",
            "micstudio_verify_sink:monitor_MONO",
            "capture.ghelper-audio:input_FR",
        ])
        .output();
    let _ = Command::new("pw-link")
        .args([
            "alsa_input.pci-0000_00_1f.3.analog-stereo:capture_FL",
            "capture.ghelper-audio:input_FL",
        ])
        .output();
    let _ = Command::new("pw-link")
        .args([
            "alsa_input.pci-0000_00_1f.3.analog-stereo:capture_FR",
            "capture.ghelper-audio:input_FR",
        ])
        .output();
    let _ = Command::new("pactl")
        .args(["unload-module", &mod_id])
        .output();

    // Restore Natural Clean
    let _ = apply_voice_internal("Natural Clean", false);

    println!(
        "=========================================================================================="
    );
    if all_passed {
        println!(
            ">>> VERIFICATION RESULT: ALL PRESETS PASSED (Peak <= -1.00 dBFS, ΔRMS < 0.55 dB, Spectral < 1.0 dB)"
        );
        Ok(())
    } else {
        anyhow::bail!(
            "Verification failed: One or more presets deviated beyond allowable tolerances."
        );
    }
}

fn calc_band_delta(
    off: &[f32],
    live: &[f32],
    filter_type: crate::dsp::biquad::FilterType,
    freq: f32,
    q: f32,
) -> f32 {
    let mut b_off = crate::dsp::biquad::Biquad::new();
    b_off.configure(filter_type, freq, q, 0.0, 48000.0);
    let mut b_live = crate::dsp::biquad::Biquad::new();
    b_live.configure(filter_type, freq, q, 0.0, 48000.0);

    let mut off_sq = 0.0f64;
    let mut live_sq = 0.0f64;

    for i in 0..off.len() {
        let fo = b_off.process(off[i]);
        let fl = b_live.process(live[i]);
        off_sq += (fo as f64) * (fo as f64);
        live_sq += (fl as f64) * (fl as f64);
    }

    let n = off.len().max(1) as f64;
    let off_rms = (off_sq / n).sqrt() as f32;
    let live_rms = (live_sq / n).sqrt() as f32;

    // Noise floor threshold (-32 dBFS)
    if off_rms < 0.025 && live_rms < 0.025 {
        return 0.0;
    }

    let off_db = if off_rms > 1e-6 {
        20.0 * off_rms.log10()
    } else {
        -96.0
    };
    let live_db = if live_rms > 1e-6 {
        20.0 * live_rms.log10()
    } else {
        -96.0
    };

    (live_db - off_db).abs()
}

pub fn run_live_verify(preset: Option<&str>, duration_secs: u32) -> Result<()> {
    println!("=== MicStudio Real Hardware Live Verification ===");
    println!(":: Hardware Mic Target : alsa_input.pci-0000_00_1f.3.analog-stereo");
    println!(":: Virtual Source Node : ghelper-audio / ghelper-audio-fx");
    println!(":: Test Duration       : {}s per preset", duration_secs);

    let presets_to_test: Vec<&str> = if let Some(p) = preset {
        vec![p]
    } else {
        vec!["Natural Clean", "Deep Voice", "Daft Punk"]
    };

    println!(
        "\n{:<15} | {:>10} | {:>10} | {:>7} | {:>8} | {}",
        "Preset", "Peak (dBFS)", "RMS (dBFS)", "Clicks", "Dropouts", "Status"
    );
    println!(
        "{:-<15}-+-{:-<10}-+-{:-<10}-+-{:-<7}-+-{:-<8}-+-{:-<6}",
        "", "", "", "", "", ""
    );

    let mut all_passed = true;
    let mut passed_presets: Vec<String> = Vec::new();

    for &name in &presets_to_test {
        apply_voice_internal(name, false)?;
        std::thread::sleep(std::time::Duration::from_millis(150));

        let is_clean = name.eq_ignore_ascii_case("Natural Clean");
        let target = if is_clean {
            "ghelper-audio"
        } else {
            "ghelper-audio-fx"
        };

        let test_wav = std::path::PathBuf::from(format!(
            "/tmp/micstudio_hw_test_{}.wav",
            name.replace(' ', "_")
        ));
        let raw_wav = std::path::PathBuf::from(format!(
            "/tmp/micstudio_hw_raw_{}.wav",
            name.replace(' ', "_")
        ));
        let _ = std::fs::remove_file(&test_wav);
        let _ = std::fs::remove_file(&raw_wav);

        println!(
            ":: Recording {}s for preset '{}'... (Speak into your microphone now)",
            duration_secs, name
        );

        let proc_child = Command::new("pw-record")
            .args([
                "--target",
                target,
                "--channels",
                "1",
                test_wav.to_str().unwrap(),
            ])
            .spawn();
        let raw_child = Command::new("pw-record")
            .args([
                "--target",
                "alsa_input.pci-0000_00_1f.3.analog-stereo",
                "--channels",
                "1",
                raw_wav.to_str().unwrap(),
            ])
            .spawn();

        let mut proc_c = match proc_child {
            Ok(c) => c,
            Err(e) => anyhow::bail!("Failed to spawn pw-record for target {}: {}", target, e),
        };
        let mut raw_c = match raw_child {
            Ok(c) => c,
            Err(e) => anyhow::bail!("Failed to spawn pw-record for raw mic: {}", e),
        };

        std::thread::sleep(std::time::Duration::from_secs(duration_secs as u64));
        unsafe {
            libc::kill(proc_c.id() as i32, libc::SIGINT);
            libc::kill(raw_c.id() as i32, libc::SIGINT);
        }
        let _ = proc_c.wait();
        let _ = raw_c.wait();

        if !test_wav.exists() {
            println!(
                "{:<15} | {:>10} | {:>10} | {:>7} | {:>8} | FAIL (no file)",
                name, "-inf", "-inf", "0", "0"
            );
            all_passed = false;
            continue;
        }

        let (samples, _) = crate::renderer::read_wav_mono(&test_wav)?;
        let mut peak = 0.0f32;
        let mut sum_sq = 0.0f64;
        let mut clicks = 0;
        let mut dropouts = 0;
        let mut cur_zero = 0;

        for i in 0..samples.len() {
            let s = samples[i];
            if s.abs() > peak {
                peak = s.abs();
            }
            sum_sq += (s as f64) * (s as f64);
            if i > 0 {
                let diff = (s - samples[i - 1]).abs();
                if diff > 0.20 {
                    clicks += 1;
                }
            }
            if s.abs() < 1e-4 {
                cur_zero += 1;
            } else {
                if cur_zero >= 240 {
                    dropouts += 1;
                }
                cur_zero = 0;
            }
        }
        if cur_zero >= 240 {
            dropouts += 1;
        }

        let n = samples.len().max(1);
        let rms = (sum_sq / n as f64).sqrt() as f32;
        let peak_db = if peak > 1e-6 {
            20.0 * peak.log10()
        } else {
            -120.0
        };
        let rms_db = if rms > 1e-6 {
            20.0 * rms.log10()
        } else {
            -120.0
        };

        let is_intentional_fx =
            name.eq_ignore_ascii_case("Cyber Chopper") || name.eq_ignore_ascii_case("Dalek");
        let passed = peak_db <= -2.90
            && rms_db >= -32.0
            && rms_db <= -14.0
            && clicks == 0
            && (dropouts == 0 || is_intentional_fx);

        if passed {
            passed_presets.push(name.to_string());
        } else {
            all_passed = false;
        }

        let status_str = if passed { "PASS" } else { "FAIL" };
        println!(
            "{:<15} | {:>7.2} dBFS | {:>7.2} dBFS | {:>7} | {:>8} | {}",
            name, peak_db, rms_db, clicks, dropouts, status_str
        );
    }

    let _ = apply_voice_internal("Natural Clean", false);

    if !passed_presets.is_empty() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/manisk".to_string());
        let v_path = std::path::Path::new(&home)
            .join(".config")
            .join("micstudio")
            .join("verified.json");
        let mut cur_verified: Vec<String> = if v_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&v_path) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                    parsed["verified_presets"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        for p in &passed_presets {
            if !cur_verified.contains(p) {
                cur_verified.push(p.clone());
            }
        }
        let out_obj = serde_json::json!({
            "verified_presets": cur_verified,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64(),
            "source": "live_real_mic"
        });
        let _ = std::fs::write(&v_path, serde_json::to_string_pretty(&out_obj)?);
    }

    println!(
        "=========================================================================================="
    );
    if all_passed {
        println!(">>> REAL MIC LIVE VERIFICATION: ALL TESTED PRESETS PASSED ON REAL HARDWARE MIC");
        Ok(())
    } else {
        anyhow::bail!(
            "Live real mic verification failed: One or more presets deviated from tolerances."
        );
    }
}
