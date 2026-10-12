//! MicStudio Application State & Actions.

use crate::config::{parse_dynamic_stages, CleanMicSettings};
use crate::dsp::spectrum::LiveSpectrumData;
use crate::presets::{Preset, PresetManager};
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
    pub fn from_index(idx: usize) -> Self {
        match idx % 6 {
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

pub struct AppState {
    pub presets: Vec<Preset>,
    pub cursor: usize, // Index in filtered_indices
    pub active_preset_name: String,
    pub selected_control: usize,
    pub dpdfnet_attenuation: f32,
    pub output_gain_db: f32,
    pub effect_mix_pct: u32,
    pub sidetone_enabled: bool,
    pub meter_enabled: bool,
    pub input_rms_db: f32,
    pub spectrograph_enabled: bool,
    pub spectrograph_fullscreen: bool,
    pub spectrograph_mode: SpectrographMode,
    pub spectrum: LiveSpectrumData,
    pub show_help: bool,
    pub dynamic_stages: Vec<String>,
    pub status_message: Option<(String, Instant)>,
    pub should_quit: bool,

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
            effect_mix_pct: 100,
            sidetone_enabled: cfg.hear_voice,
            meter_enabled: cfg.meter_enabled,
            input_rms_db: -96.0,
            spectrograph_enabled: cfg.hear_voice,
            spectrograph_fullscreen: false,
            spectrograph_mode: SpectrographMode::RtaBars,
            spectrum: LiveSpectrumData::default(),
            show_help: false,
            dynamic_stages,
            status_message: None,
            should_quit: false,
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
                    p.name.to_lowercase().contains(&q)
                        || p.category.to_lowercase().contains(&q)
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
        self.selected_control = (self.selected_control + 1) % 6;
    }

    pub fn cycle_control_prev(&mut self) {
        self.selected_control = if self.selected_control > 0 {
            self.selected_control - 1
        } else {
            5
        };
    }

    pub fn adjust_selected_control(&mut self, direction: i32) -> Result<()> {
        match LiveControl::from_index(self.selected_control) {
            LiveControl::Dpdfnet => {
                let delta = direction as f32 * 5.0;
                let new_val = (self.dpdfnet_attenuation + delta).clamp(0.0, 100.0);
                self.dpdfnet_attenuation = new_val;
                let mut cfg = CleanMicSettings::load();
                cfg.dpdfnet_attenuation_db = new_val;
                cfg.apply_dsp_params()?;
                cfg.save()?;
                self.set_status(format!("DPDFNet Attenuation: {:.0} dB", new_val));
            }
            LiveControl::OutputGain => {
                let delta = direction as f32 * 1.0;
                let new_val = (self.output_gain_db + delta).clamp(-24.0, 12.0);
                self.output_gain_db = new_val;
                let mut cfg = CleanMicSettings::load();
                cfg.output_gain_db = new_val;
                cfg.apply_dsp_params()?;
                cfg.save()?;
                self.set_status(format!("Output Gain: {:+.1} dB", new_val));
            }
            LiveControl::EffectMix => {
                let delta = direction * 5;
                let new_val = (self.effect_mix_pct as i32 + delta).clamp(0, 100) as u32;
                self.effect_mix_pct = new_val;
                let wet_dry = 1.0 - (new_val as f32 / 100.0);
                if let Some(p) = self
                    .presets
                    .iter_mut()
                    .find(|p| p.name.eq_ignore_ascii_case(&self.active_preset_name))
                {
                    p.wet_dry_mix = wet_dry;
                    let _ = crate::cli::apply_voice_silent(&p.name);
                }
                self.set_status(format!("Effect Mix: {}% Wet", new_val));
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
            if self.spectrograph_enabled { "ON" } else { "OFF" }
        ));
    }

    pub fn toggle_spectrograph_fullscreen(&mut self) {
        self.spectrograph_fullscreen = !self.spectrograph_fullscreen;
        self.set_status(format!(
            "Spectrograph View: {}",
            if self.spectrograph_fullscreen { "Fullscreen" } else { "Embedded" }
        ));
    }

    pub fn toggle_spectrograph_mode(&mut self) {
        self.spectrograph_mode = match self.spectrograph_mode {
            SpectrographMode::RtaBars => SpectrographMode::Waterfall,
            SpectrographMode::Waterfall => SpectrographMode::RtaBars,
        };
        self.set_status(format!(
            "Spectrograph Mode: {}",
            match self.spectrograph_mode {
                SpectrographMode::RtaBars => "RTA Spectrum Bars",
                SpectrographMode::Waterfall => "Waterfall Spectrogram",
            }
        ));
    }

    pub fn toggle_sidetone(&mut self) -> Result<()> {
        self.sidetone_enabled = !self.sidetone_enabled;
        let mut cfg = CleanMicSettings::load();
        cfg.hear_voice = self.sidetone_enabled;
        CleanMicSettings::sync_hear_voice(self.sidetone_enabled);
        cfg.save()?;
        self.spectrograph_enabled = self.sidetone_enabled;
        if self.sidetone_enabled {
            self.meter_enabled = true;
        }
        self.set_status(format!(
            "Sidetone: {}",
            if self.sidetone_enabled { "ON (Live Spectrograph Active)" } else { "OFF" }
        ));
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
        self.set_status(format!(
            "Meter: {}",
            if self.meter_enabled { "ON" } else { "OFF" }
        ));
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
        let msg = crate::cli::apply_voice_internal(&name, false)?;
        self.active_preset_name = name;
        self.dpdfnet_attenuation = atten;
        self.set_status(msg);
        Ok(())
    }

    pub fn revert_clean(&mut self) -> Result<()> {
        let msg = crate::cli::apply_voice_internal("Natural Clean", false)?;
        self.active_preset_name = "Natural Clean".to_string();
        self.dpdfnet_attenuation = 45.0;
        let indices = self.filtered_indices();
        for (i, &idx) in indices.iter().enumerate() {
            if self.presets[idx].name.eq_ignore_ascii_case("Natural Clean") {
                self.cursor = i;
                break;
            }
        }
        self.set_status(msg);
        Ok(())
    }

    pub fn trigger_audition(&mut self) {
        if self.audition_running.load(Ordering::SeqCst) {
            self.set_status("Audition already running...".to_string());
            return;
        }

        let is_running = self.audition_running.clone();
        let status_arc = self.audition_status.clone();
        let active_is_clean = self.active_preset_name.eq_ignore_ascii_case("Natural Clean");
        let source_node = if active_is_clean {
            "ghelper-audio"
        } else {
            "ghelper-audio-fx"
        };

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
            if let Some((msg, created)) = guard.as_ref() {
                if created.elapsed().as_secs() < 4 {
                    return Some(msg.clone());
                }
            }
        }
        if let Some((msg, created)) = &self.status_message {
            if created.elapsed().as_secs() < 3 {
                return Some(msg.clone());
            }
        }
        None
    }
}
