//! MicStudio Application State & Actions.

use crate::config::{CleanMicSettings, parse_dynamic_stages};
use crate::dsp::spectrum::LiveSpectrumData;
use crate::presets::{Preset, PresetManager};
use crate::ui::theme::ThemeChoice;
use anyhow::Result;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveControl {
    Dpdfnet = 0,
    OutputGain = 1,
    EffectMix = 2,
    Sidetone = 3,
    Meter = 4,
    Spectrograph = 5,
}

impl LiveControl {
    pub const COUNT: usize = 6;

    pub fn from_index(idx: usize) -> Self {
        match idx % Self::COUNT {
            0 => Self::Dpdfnet,
            1 => Self::OutputGain,
            2 => Self::EffectMix,
            3 => Self::Sidetone,
            4 => Self::Meter,
            _ => Self::Spectrograph,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectrographMode {
    RtaBars,
    Waterfall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    ResetSelected(String),
    ResetAll,
    QuitUnsaved,
}

pub struct AppState {
    pub presets: Vec<Preset>,
    pub cursor: usize, // Index in filtered_indices
    pub active_preset_name: String,
    pub selected_control: usize,
    pub dpdfnet_attenuation: f32,
    pub output_gain_db: f32,
    pub quiet_boost_enabled: bool,
    pub quiet_boost_db: f32,
    pub effect_mix_pct: u32,
    pub sidetone_enabled: bool,
    pub sidetone_guarded: bool,
    pub force_speaker_sidetone: bool,
    pub meter_enabled: bool,
    pub input_rms_db: f32,
    pub spectrograph_enabled: bool,
    pub spectrograph_fullscreen: bool,
    pub spectrograph_mode: SpectrographMode,
    pub spectrum: LiveSpectrumData,
    pub show_help: bool,
    pub show_details_modal: bool,
    pub dynamic_stages: Vec<String>,
    pub status_message: Option<(String, Instant)>,
    pub should_quit: bool,
    pub unsaved_changes: bool,
    pub confirm_action: Option<ConfirmAction>,
    pub theme: ThemeChoice,

    // Search and filtering
    pub search_mode: bool,
    pub search_query: String,

    // Scrolling
    pub scroll_offset: usize,

    // Audition state
    pub audition_running: Arc<AtomicBool>,
    pub audition_status: Arc<Mutex<Option<(String, Instant)>>>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let mgr = PresetManager::new()?;
        let presets = mgr.all_presets().to_vec();
        let active_name = mgr.active_preset_name().to_string();

        let cfg = CleanMicSettings::load();
        let dynamic_stages = parse_dynamic_stages();

        let mut initial_cursor = 0;
        for (i, p) in presets.iter().enumerate() {
            if p.name.eq_ignore_ascii_case(&active_name) {
                initial_cursor = i;
                break;
            }
        }

        Ok(Self {
            presets,
            cursor: initial_cursor,
            active_preset_name: active_name,
            selected_control: 0,
            dpdfnet_attenuation: cfg.dpdfnet_attenuation_db,
            output_gain_db: cfg.output_gain_db,
            quiet_boost_enabled: cfg.quiet_voice_boost_enabled,
            quiet_boost_db: cfg.quiet_voice_boost_db,
            effect_mix_pct: 100,
            sidetone_enabled: cfg.hear_voice,
            sidetone_guarded: CleanMicSettings::is_sidetone_guarded(),
            force_speaker_sidetone: cfg.force_speaker_sidetone,
            meter_enabled: cfg.meter_enabled,
            input_rms_db: -96.0,
            spectrograph_enabled: cfg.hear_voice,
            spectrograph_fullscreen: false,
            spectrograph_mode: SpectrographMode::RtaBars,
            spectrum: LiveSpectrumData::default(),
            show_help: false,
            show_details_modal: false,
            dynamic_stages,
            status_message: None,
            should_quit: false,
            unsaved_changes: false,
            confirm_action: None,
            theme: ThemeChoice::from_str_lenient(&cfg.theme),
            search_mode: false,
            search_query: String::new(),
            scroll_offset: 0,
            audition_running: Arc::new(AtomicBool::new(false)),
            audition_status: Arc::new(Mutex::new(None)),
        })
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.search_query.trim().to_lowercase();
        if q.is_empty() {
            (0..self.presets.len()).collect()
        } else {
            self.presets
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.name.to_lowercase().contains(&q) || p.category.to_lowercase().contains(&q)
                })
                .map(|(i, _)| i)
                .collect()
        }
    }

    pub fn selected_preset(&self) -> &Preset {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            &self.presets[0]
        } else {
            let actual_idx = indices[self.cursor.min(indices.len() - 1)];
            &self.presets[actual_idx]
        }
    }

    pub fn move_cursor_up(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        } else {
            self.cursor = len - 1;
        }
    }

    pub fn move_cursor_down(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        if self.cursor + 1 < len {
            self.cursor += 1;
        } else {
            self.cursor = 0;
        }
    }

    pub fn jump_to_preset(&mut self, num: usize) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        if num >= 1 && num <= len {
            self.cursor = num - 1;
        }
    }

    pub fn enter_search(&mut self) {
        self.search_mode = true;
    }

    pub fn exit_search(&mut self, clear: bool) {
        self.search_mode = false;
        if clear {
            self.search_query.clear();
            self.cursor = 0;
        }
    }

    pub fn handle_search_char(&mut self, c: char) {
        self.search_query.push(c);
        let len = self.filtered_indices().len();
        if self.cursor >= len && len > 0 {
            self.cursor = len - 1;
        }
    }

    pub fn handle_search_backspace(&mut self) {
        self.search_query.pop();
        let len = self.filtered_indices().len();
        if self.cursor >= len && len > 0 {
            self.cursor = len - 1;
        }
    }

    pub fn cycle_control_next(&mut self) {
        self.selected_control = (self.selected_control + 1) % LiveControl::COUNT;
    }

    pub fn cycle_control_prev(&mut self) {
        self.selected_control = if self.selected_control > 0 {
            self.selected_control - 1
        } else {
            LiveControl::COUNT - 1
        };
    }

    pub fn adjust_selected_control(&mut self, direction: i32) -> Result<()> {
        match LiveControl::from_index(self.selected_control) {
            LiveControl::Dpdfnet => {
                let delta = direction as f32 * 5.0;
                let new_val = (self.dpdfnet_attenuation + delta).clamp(0.0, 100.0);
                self.dpdfnet_attenuation = new_val;
                self.unsaved_changes = true;
                let mut cfg = CleanMicSettings::load();
                cfg.dpdfnet_attenuation_db = new_val;
                cfg.apply_dsp_params()?;
                self.set_status(format!("DPDFNet Attenuation: {:.0} dB [LIVE - Press 'w' to save]", new_val));
            }
            LiveControl::OutputGain => {
                let delta = direction as f32 * 0.5;
                let new_val = (self.output_gain_db + delta).clamp(-24.0, 12.0);
                self.output_gain_db = new_val;
                self.unsaved_changes = true;
                let mut cfg = CleanMicSettings::load();
                cfg.output_gain_db = new_val;
                cfg.apply_dsp_params()?;
                let _ = crate::cli::apply_voice_silent(&self.active_preset_name);
                self.set_status(format!("Output Gain: {:+.1} dB [LIVE - Press 'w' to save]", new_val));
            }
            LiveControl::EffectMix => {
                let is_clean = self
                    .active_preset_name
                    .eq_ignore_ascii_case("Natural Clean");
                if is_clean {
                    self.set_status(
                        "Natural Clean is direct broadcast speech (100% clean, no FX mix)"
                            .to_string(),
                    );
                } else {
                    let delta = direction * 5;
                    let new_val = (self.effect_mix_pct as i32 + delta).clamp(0, 100) as u32;
                    self.effect_mix_pct = new_val;
                    self.unsaved_changes = true;
                    let wet_dry = 1.0 - (new_val as f32 / 100.0);
                    if let Some(p) = self
                        .presets
                        .iter_mut()
                        .find(|p| p.name.eq_ignore_ascii_case(&self.active_preset_name))
                    {
                        p.wet_dry_mix = wet_dry;
                        let _ = crate::cli::apply_voice_silent(&p.name);
                    }
                    self.set_status(format!("Effect Mix: {}% Wet [LIVE - Press 'w' to save]", new_val));
                }
            }
            LiveControl::Sidetone => {
                self.toggle_sidetone()?;
            }
            LiveControl::Meter => {
                self.toggle_meter();
            }
            LiveControl::Spectrograph => {
                self.toggle_spectrograph_mode();
            }
        }
        Ok(())
    }

    pub fn toggle_spectrograph(&mut self) {
        self.spectrograph_enabled = !self.spectrograph_enabled;
        self.set_status(format!(
            "Live Spectrograph: {}",
            if self.spectrograph_enabled {
                "ON"
            } else {
                "OFF"
            }
        ));
    }

    pub fn toggle_spectrograph_fullscreen(&mut self) {
        self.spectrograph_fullscreen = !self.spectrograph_fullscreen;
        if self.spectrograph_fullscreen {
            self.spectrograph_enabled = true;
            self.meter_enabled = true;
        }
    }

    pub fn toggle_spectrograph_mode(&mut self) {
        self.spectrograph_mode = match self.spectrograph_mode {
            SpectrographMode::RtaBars => SpectrographMode::Waterfall,
            SpectrographMode::Waterfall => SpectrographMode::RtaBars,
        };
    }

    pub fn toggle_sidetone(&mut self) -> Result<()> {
        let is_speaker = CleanMicSettings::is_physically_speaker();
        if !self.sidetone_enabled {
            self.sidetone_enabled = true;
            self.force_speaker_sidetone = false;
        } else if is_speaker && !self.force_speaker_sidetone {
            // Second toggle when on speakers: force speaker override!
            self.force_speaker_sidetone = true;
        } else {
            self.sidetone_enabled = false;
            self.force_speaker_sidetone = false;
        }

        let mut cfg = CleanMicSettings::load();
        cfg.hear_voice = self.sidetone_enabled;
        cfg.force_speaker_sidetone = self.force_speaker_sidetone;
        cfg.save()?;
        CleanMicSettings::sync_hear_voice(self.sidetone_enabled);
        self.sidetone_guarded = CleanMicSettings::is_sidetone_guarded();
        if self.sidetone_enabled {
            self.spectrograph_enabled = true;
            self.meter_enabled = true;
        } else if !self.spectrograph_fullscreen {
            self.spectrograph_enabled = false;
        }
        Ok(())
    }

    pub fn toggle_meter(&mut self) {
        self.meter_enabled = !self.meter_enabled;
        let mut cfg = CleanMicSettings::load();
        cfg.meter_enabled = self.meter_enabled;
        let _ = cfg.save();
        if !self.meter_enabled {
            self.input_rms_db = -96.0;
        }
    }

    pub fn toggle_details_modal(&mut self) {
        self.show_details_modal = !self.show_details_modal;
    }

    pub fn close_details_modal(&mut self) {
        self.show_details_modal = false;
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn close_help(&mut self) {
        self.show_help = false;
    }

    pub fn apply_selected(&mut self) -> Result<()> {
        let name = self.selected_preset().name.clone();
        let atten = self.selected_preset().dpdfnet_attenuation_db;
        let wet_dry = self.selected_preset().wet_dry_mix;
        let mix_pct = ((1.0 - wet_dry).clamp(0.0, 1.0) * 100.0).round() as u32;
        let _ = crate::cli::apply_voice_extended(&name, false, false)?;
        self.active_preset_name = name;
        self.dpdfnet_attenuation = atten;
        self.effect_mix_pct = mix_pct;
        self.unsaved_changes = true;
        self.set_status(format!(
            "Active: '{}' [LIVE TEST - Press 'p' to audition, 'w' to save]",
            self.active_preset_name
        ));
        Ok(())
    }

    pub fn revert_clean(&mut self) -> Result<()> {
        let _ = crate::cli::apply_voice_extended("Natural Clean", false, false)?;
        self.active_preset_name = "Natural Clean".to_string();
        self.dpdfnet_attenuation = 45.0;
        self.effect_mix_pct = 100;
        self.unsaved_changes = true;
        let indices = self.filtered_indices();
        for (i, &idx) in indices.iter().enumerate() {
            if self.presets[idx].name.eq_ignore_ascii_case("Natural Clean") {
                self.cursor = i;
                break;
            }
        }
        self.set_status("Active: 'Natural Clean' [LIVE TEST - Press 'w' to save]".to_string());
        Ok(())
    }

    pub fn request_reset_selected(&mut self) {
        let name = self.selected_preset().name.clone();
        self.confirm_action = Some(ConfirmAction::ResetSelected(name));
    }

    pub fn request_reset_all(&mut self) {
        self.confirm_action = Some(ConfirmAction::ResetAll);
    }

    pub fn request_quit(&mut self) {
        if self.unsaved_changes {
            self.confirm_action = Some(ConfirmAction::QuitUnsaved);
        } else {
            self.should_quit = true;
        }
    }

    pub fn cycle_theme(&mut self) {
        self.theme = self.theme.next();
        self.unsaved_changes = true;
        self.set_status(format!("UI Theme: {} [Press 'w' to save]", self.theme.name()));
    }

    pub fn discard_and_restore_saved(&mut self) {
        let cfg = CleanMicSettings::load();
        let _ = cfg.apply_dsp_params();
        let mgr = PresetManager::new().ok();
        if let Some(mgr) = mgr {
            let active_name = mgr.active_preset_name().to_string();
            let _ = crate::cli::apply_voice_internal(&active_name, false);
            self.active_preset_name = active_name;
        }
        self.output_gain_db = cfg.output_gain_db;
        self.dpdfnet_attenuation = cfg.dpdfnet_attenuation_db;
        self.theme = ThemeChoice::from_str_lenient(&cfg.theme);
        self.unsaved_changes = false;
        self.set_status("Discarded unsaved tweaks; restored saved settings.".to_string());
    }

    pub fn reset_selected_preset(&mut self) -> Result<()> {
        let name = self.selected_preset().name.clone();
        if name.eq_ignore_ascii_case("Natural Clean") {
            self.dpdfnet_attenuation = 45.0;
            self.output_gain_db = 4.5;
            self.quiet_boost_db = 4.0;
            self.quiet_boost_enabled = true;
            self.effect_mix_pct = 100;
            let mut cfg = CleanMicSettings::load();
            cfg.dpdfnet_attenuation_db = 45.0;
            cfg.output_gain_db = 4.5;
            cfg.quiet_voice_boost_enabled = true;
            cfg.quiet_voice_boost_db = 4.0;
            cfg.apply_dsp_params()?;
            cfg.save()?;
            let mut mgr = PresetManager::new()?;
            let _ = mgr.reset_preset("Natural Clean");
            self.presets = mgr.all_presets().to_vec();
            if self.active_preset_name.eq_ignore_ascii_case("Natural Clean") {
                let _ = crate::cli::apply_voice_internal("Natural Clean", false);
            }
            self.unsaved_changes = false;
            self.set_status(
                "Reset 'Natural Clean' to factory baseline (DPDFNet 45dB, Output Gain +4.5dB, Quiet Boost ON +4dB)."
                    .to_string(),
            );
            return Ok(());
        }
        let mut mgr = PresetManager::new()?;
        if mgr.reset_preset(&name)? {
            self.presets = mgr.all_presets().to_vec();
            if let Some(reset_p) = self
                .presets
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(&name))
                .cloned()
            {
                if self.active_preset_name.eq_ignore_ascii_case(&name) {
                    self.dpdfnet_attenuation = reset_p.dpdfnet_attenuation_db;
                    let wet_dry = reset_p.wet_dry_mix;
                    self.effect_mix_pct =
                        ((1.0 - wet_dry).clamp(0.0, 1.0) * 100.0).round() as u32;
                    let _ = self.apply_selected();
                }
                self.unsaved_changes = false;
                self.set_status(format!(
                    "Reset '{}' to factory defaults (Boost: {:+.1}dB, DPDFNet: {:.0}dB)",
                    name, reset_p.model_gain_db, reset_p.dpdfnet_attenuation_db
                ));
            }
        } else {
            self.set_status(format!("Cannot reset '{}': not a factory preset.", name));
        }
        Ok(())
    }

    pub fn reset_all_to_factory(&mut self) -> Result<()> {
        let mut cfg = CleanMicSettings::default();
        cfg.quiet_voice_boost_enabled = true;
        cfg.quiet_voice_boost_db = 4.0;
        cfg.dpdfnet_attenuation_db = 45.0;
        cfg.output_gain_db = 4.5;
        cfg.apply_dsp_params()?;
        cfg.save()?;

        let mut mgr = PresetManager::new()?;
        mgr.reset_all_presets()?;
        self.presets = mgr.all_presets().to_vec();

        self.dpdfnet_attenuation = 45.0;
        self.output_gain_db = 4.5;
        self.quiet_boost_db = 4.0;
        self.quiet_boost_enabled = true;
        self.effect_mix_pct = 100;
        self.unsaved_changes = false;

        let _ = crate::cli::apply_voice_internal(&self.active_preset_name, false);
        self.set_status(
            "Reset ALL 27 presets & clean mic to factory defaults (+4.5 dB baseline).".to_string(),
        );
        Ok(())
    }

    pub fn save_current_tweaks(&mut self) -> Result<()> {
        let active_name = self.active_preset_name.clone();
        let mut cfg = CleanMicSettings::load();
        cfg.dpdfnet_attenuation_db = self.dpdfnet_attenuation;
        cfg.output_gain_db = self.output_gain_db;
        cfg.quiet_voice_boost_enabled = self.quiet_boost_enabled;
        cfg.quiet_voice_boost_db = self.quiet_boost_db;
        cfg.theme = self.theme.as_str().to_string();
        cfg.save()?;

        let mut mgr = PresetManager::new()?;
        mgr.set_active_preset(&active_name)?;

        if !active_name.eq_ignore_ascii_case("Natural Clean") {
            if let Some(mut preset) = mgr.find_preset(&active_name) {
                preset.dpdfnet_attenuation_db = self.dpdfnet_attenuation;
                let wet_dry = 1.0 - (self.effect_mix_pct as f32 / 100.0);
                preset.wet_dry_mix = wet_dry;
                if let Some(cur) = self
                    .presets
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(&active_name))
                {
                    preset.model_gain_db = cur.model_gain_db;
                }
                mgr.update_preset_parameters(&preset)?;
                self.presets = mgr.all_presets().to_vec();
            }
        }
        self.unsaved_changes = false;
        self.set_status(format!(
            "Saved '{}' & settings permanently to disk [settings.toml].",
            active_name
        ));
        Ok(())
    }

    pub fn reload_presets(&mut self) -> Result<()> {
        let mut mgr = PresetManager::new()?;
        mgr.reload_from_disk()?;
        self.presets = mgr.all_presets().to_vec();
        let active_name = mgr.active_preset_name().to_string();
        self.active_preset_name = active_name.clone();
        let indices = self.filtered_indices();
        for (i, &idx) in indices.iter().enumerate() {
            if self.presets[idx].name.eq_ignore_ascii_case(&active_name) {
                self.cursor = i;
                break;
            }
        }
        let _ = self.apply_selected();
        self.set_status(format!(
            "Reloaded {} presets from disk.",
            self.presets.len()
        ));
        Ok(())
    }

    pub fn trigger_audition(&mut self) {
        if self.audition_running.load(Ordering::SeqCst) {
            self.set_status("Audition already running...".to_string());
            return;
        }

        let is_running = self.audition_running.clone();
        let status_arc = self.audition_status.clone();
        let source_node = "ghelper-audio";

        is_running.store(true, Ordering::SeqCst);
        if let Ok(mut g) = status_arc.lock() {
            *g = Some(("Audition: Recording 5s...".to_string(), Instant::now()));
        }

        let node = source_node.to_string();
        std::thread::spawn(move || {
            let wav_path = "/tmp/micstudio_audition.wav";
            let mut child = match Command::new("pw-record")
                .args(["--target", &node, wav_path])
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    if let Ok(mut g) = status_arc.lock() {
                        *g = Some((format!("Audition error: {}", e), Instant::now()));
                    }
                    is_running.store(false, Ordering::SeqCst);
                    return;
                }
            };

            std::thread::sleep(std::time::Duration::from_secs(5));
            let _ = child.kill();
            let _ = child.wait();

            if let Ok(mut g) = status_arc.lock() {
                *g = Some(("Audition: Playing back...".to_string(), Instant::now()));
            }

            // Loudness normalization & gain boost for clear laptop speaker playback
            boost_and_normalize_audition(wav_path);

            let _ = Command::new("pw-play").arg(wav_path).output();

            if let Ok(mut g) = status_arc.lock() {
                *g = Some(("Audition complete.".to_string(), Instant::now()));
            }
            is_running.store(false, Ordering::SeqCst);
        });
    }

    pub fn set_status(&mut self, text: String) {
        self.status_message = Some((text, Instant::now()));
    }

    pub fn active_status(&self) -> Option<String> {
        if let Ok(guard) = self.audition_status.lock() {
            if let Some((msg, _)) = guard.as_ref().filter(|(_, c)| c.elapsed().as_secs() < 4) {
                return Some(msg.clone());
            }
        }
        if let Some((msg, _)) = self
            .status_message
            .as_ref()
            .filter(|(_, c)| c.elapsed().as_secs() < 3)
        {
            return Some(msg.clone());
        }
        None
    }
}

fn boost_and_normalize_audition(path: &str) {
    if let Ok(mut reader) = hound::WavReader::open(path) {
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
            hound::SampleFormat::Int => {
                let max_val = (1i64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
                reader
                    .samples::<i32>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / max_val)
                    .collect()
            }
        };
        if !samples.is_empty() {
            let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            // Target loud & clear peak at 0.92 (-0.7 dBFS). Scale up to 16.0x (+24 dB) for quiet speech.
            let gain = if peak > 0.0001 {
                (0.92 / peak).clamp(1.5, 16.0)
            } else {
                2.0
            };
            let norm_path = format!("{}.boosted.wav", path);
            if let Ok(mut writer) = hound::WavWriter::create(&norm_path, spec) {
                match spec.sample_format {
                    hound::SampleFormat::Float => {
                        for s in samples {
                            let _ = writer.write_sample((s * gain).clamp(-0.99, 0.99));
                        }
                    }
                    hound::SampleFormat::Int => {
                        let max_val = (1i64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
                        for s in samples {
                            let val = ((s * gain).clamp(-0.99, 0.99) * max_val) as i32;
                            let _ = writer.write_sample(val);
                        }
                    }
                }
                let _ = writer.finalize();
                let _ = std::fs::rename(&norm_path, path);
            }
        }
    }
}
