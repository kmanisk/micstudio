//! Clean, responsive Ratatui TUI with Neovim keyboard controls.
//! Streamlined layout, uncluttered telemetry, live voice monitor, and virtual device toggle.

use crate::config::CleanMicSettings;
use crate::ipc::{is_engine_running, send_command, IpcRequest, IpcResponse};
use crate::presets::PresetManager;
use crate::speaker::{SpeakerDsp, SpeakerVoicing};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io::{self, Stdout};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ActivePanel {
    Presets,
    NoiseAndLevel,
    SpeakerDsp,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum NoiseControlItem {
    RnnoiseToggle,
    DenoiseBlend,
    GateToggle,
    GateAggro,
    SilenceAtten,
    Hangover,
    OutputGain,
    HearVoice,
    VirtualDevice,
}

impl NoiseControlItem {
    pub const ALL: [NoiseControlItem; 9] = [
        NoiseControlItem::RnnoiseToggle,
        NoiseControlItem::DenoiseBlend,
        NoiseControlItem::GateToggle,
        NoiseControlItem::GateAggro,
        NoiseControlItem::SilenceAtten,
        NoiseControlItem::Hangover,
        NoiseControlItem::OutputGain,
        NoiseControlItem::HearVoice,
        NoiseControlItem::VirtualDevice,
    ];

    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            NoiseControlItem::DenoiseBlend
                | NoiseControlItem::GateAggro
                | NoiseControlItem::SilenceAtten
                | NoiseControlItem::Hangover
                | NoiseControlItem::OutputGain
        )
    }

    pub fn name(&self) -> &'static str {
        match self {
            NoiseControlItem::RnnoiseToggle => "RNNoise Neural Suppression",
            NoiseControlItem::DenoiseBlend => "Suppression Blend",
            NoiseControlItem::GateToggle => "Noise Gate",
            NoiseControlItem::GateAggro => "Gate Aggressiveness",
            NoiseControlItem::SilenceAtten => "Silence Attenuation",
            NoiseControlItem::Hangover => "Gate Hangover",
            NoiseControlItem::OutputGain => "Microphone Output Gain",
            NoiseControlItem::HearVoice => "Hear My Voice (Sidetone Monitor)",
            NoiseControlItem::VirtualDevice => "Separate Virtual Audio Device",
        }
    }

    pub fn unit(&self) -> &'static str {
        match self {
            NoiseControlItem::DenoiseBlend | NoiseControlItem::GateAggro => "%",
            NoiseControlItem::SilenceAtten | NoiseControlItem::OutputGain => "dB",
            NoiseControlItem::Hangover => "ms",
            _ => "",
        }
    }

    pub fn bounds_hint(&self) -> &'static str {
        match self {
            NoiseControlItem::DenoiseBlend => "Range: 0 to 100",
            NoiseControlItem::GateAggro => "Range: 0 to 100",
            NoiseControlItem::SilenceAtten => "Range: -60 to 0",
            NoiseControlItem::Hangover => "Range: 0 to 1000",
            NoiseControlItem::OutputGain => "Range: -24.0 to +12.0",
            _ => "",
        }
    }
}

pub fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = tui_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

fn tui_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut mgr = PresetManager::new()?;
    let presets = mgr.all_presets().to_vec();
    let mut list_state = ListState::default();
    list_state.select(Some(0));

    let mut speaker_state = ListState::default();
    speaker_state.select(Some(0));

    let mut active_panel = ActivePanel::NoiseAndLevel;
    let mut selected_noise_idx: usize = 0;
    let mut settings = CleanMicSettings::load();
    let _ = settings.apply_to_pipewire();

    let mut active_preset_str = mgr.active_preset_name().to_string();
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(50);

    // Debouncing state for slider changes
    let mut dsp_dirty = false;
    let mut last_dirty_time = Instant::now();

    // Cache system status queries (wpctl & speaker dsp) so we don't spawn subprocesses every 50-100ms
    let mut last_status_check = Instant::now() - Duration::from_secs(10);
    let mut cached_default_source = "ghelper-audio".to_string();
    let mut cached_speaker_status = (SpeakerVoicing::Dynamic, true, "Internal Speakers".to_string());

    // Status message line to display non-intrusive feedback inside the UI
    let mut status_msg = String::new();
    let mut status_msg_time = Instant::now();

    // Numeric input popup state
    let mut input_modal_active = false;
    let mut input_buffer = String::new();
    let mut input_target_control = NoiseControlItem::DenoiseBlend;

    // Live mic monitor for real-time telemetry (input level, output level, peak, VAD)
    let mut mic_monitor = crate::monitor::MicMonitor::new(settings.telemetry_enabled);

    let mut in_rms_db: f32 = -90.0;
    let mut out_rms_db: f32 = -90.0;
    let mut peak_lvl: f32 = 0.0;
    let mut vad_pct: f32 = 0.0;

    loop {
        // Poll status every 1.5 seconds instead of every frame
        if last_status_check.elapsed() > Duration::from_millis(1500) {
            cached_default_source = get_current_default_source();
            cached_speaker_status = SpeakerDsp::get_status().unwrap_or((SpeakerVoicing::Dynamic, true, "Internal Speakers".to_string()));
            last_status_check = Instant::now();
        }

        // Flush debounced DSP updates after 60ms of inactivity
        if dsp_dirty && last_dirty_time.elapsed() >= Duration::from_millis(60) {
            let _ = settings.apply_dsp_params();
            let _ = settings.save();
            dsp_dirty = false;
        }

        // Telemetry handling
        if settings.telemetry_enabled {
            let t = mic_monitor.get_telemetry();
            in_rms_db = t.input_rms_db;
            out_rms_db = t.output_rms_db;
            peak_lvl = t.peak_level;
            vad_pct = t.vad_probability * 100.0;

            if is_engine_running() {
                if let Ok(IpcResponse::Status { telemetry, .. }) = send_command(&IpcRequest::GetStatus) {
                    in_rms_db = telemetry.input_rms_db;
                    out_rms_db = telemetry.output_rms_db;
                    peak_lvl = telemetry.peak_level;
                    vad_pct = telemetry.vad_probability * 100.0;
                }
            }
        }

        let default_source = &cached_default_source;
        let (current_voicing, dsp_enabled, route_desc) = &cached_speaker_status;

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Length(3), // Top Summary Bar
                    Constraint::Min(12),   // Main Panel Split
                    Constraint::Length(3), // Footer
                ])
                .split(f.area());

            // 1. Header with Clean Tab Styling
            let tab_presets = if active_panel == ActivePanel::Presets {
                Span::styled(" [1] Voice Presets ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(" [1] Voice Presets ", Style::default().fg(Color::DarkGray))
            };

            let tab_noise = if active_panel == ActivePanel::NoiseAndLevel {
                Span::styled(" [2] Noise & Level ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(" [2] Noise & Level ", Style::default().fg(Color::DarkGray))
            };

            let tab_speaker = if active_panel == ActivePanel::SpeakerDsp {
                Span::styled(" [3] Dolby Atmos ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(" [3] Dolby Atmos ", Style::default().fg(Color::DarkGray))
            };

            let title_line = Line::from(vec![
                Span::styled(" MICSTUDIO ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw(" Native Audio Studio   "),
                tab_presets,
                Span::raw(" "),
                tab_noise,
                Span::raw(" "),
                tab_speaker,
            ]);
            let header = Paragraph::new(title_line)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
            f.render_widget(header, chunks[0]);

            // 2. Streamlined Top Status Bar
            let mut status_spans = vec![
                Span::styled("  Default Input: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<15}", default_source), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" │ Output: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<20}", route_desc), Style::default().fg(Color::White)),
                Span::styled(" │ Monitor [v]: ", Style::default().fg(Color::DarkGray)),
                Span::styled(if settings.hear_voice { "ON" } else { "OFF" }, Style::default().fg(if settings.hear_voice { Color::Cyan } else { Color::DarkGray }).add_modifier(Modifier::BOLD)),
                Span::styled(" │ Telemetry [t]: ", Style::default().fg(Color::DarkGray)),
                Span::styled(if settings.telemetry_enabled { "ON" } else { "OFF" }, Style::default().fg(if settings.telemetry_enabled { Color::Green } else { Color::DarkGray }).add_modifier(Modifier::BOLD)),
            ];

            if !status_msg.is_empty() && status_msg_time.elapsed() < Duration::from_secs(3) {
                status_spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                status_spans.push(Span::styled(&status_msg, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
            }

            let status_block = Paragraph::new(Line::from(status_spans))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
            f.render_widget(status_block, chunks[1]);

            // 3. Main Center Split: 60% Left Controls, 40% Right Meters & Flow
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[2]);

            match active_panel {
                ActivePanel::NoiseAndLevel => {
                    let mut lines = Vec::new();

                    lines.push(Line::from(Span::styled("NOISE REDUCTION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                    // RNNoise Toggle
                    let sel_0 = selected_noise_idx == 0;
                    lines.push(Line::from(vec![
                        Span::styled(if sel_0 { " ▶ RNNoise Suppression      " } else { "   RNNoise Suppression      " }, if sel_0 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(if settings.rnnoise_enabled { "[ON]" } else { "[BYPASS (Dry Passthrough)]" }, if settings.rnnoise_enabled { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Red).add_modifier(Modifier::BOLD) }),
                    ]));
                    // Denoise Blend
                    let sel_1 = selected_noise_idx == 1;
                    let blend_val_str = if settings.rnnoise_enabled {
                        format!("[{:.0}%]", settings.denoise_blend)
                    } else {
                        format!("[{:.0}% (Bypassed: 0%)]", settings.denoise_blend)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(if sel_1 { " ▶ Suppression Blend        " } else { "   Suppression Blend        " }, if sel_1 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(blend_val_str, if settings.rnnoise_enabled { Style::default().fg(Color::White) } else { Style::default().fg(Color::DarkGray) }),
                    ]));
                    lines.push(Line::from(""));

                    lines.push(Line::from(Span::styled("NOISE GATE", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                    // Gate Toggle
                    let sel_2 = selected_noise_idx == 2;
                    lines.push(Line::from(vec![
                        Span::styled(if sel_2 { " ▶ Gate                     " } else { "   Gate                     " }, if sel_2 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(if settings.gate_enabled { "[ON]" } else { "[BYPASS (Open Gate)]" }, if settings.gate_enabled { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Red).add_modifier(Modifier::BOLD) }),
                    ]));
                    // Gate Aggressiveness
                    let sel_3 = selected_noise_idx == 3;
                    let aggro_val_str = if settings.gate_enabled {
                        format!("[{:.0}%]", settings.gate_aggressiveness)
                    } else {
                        format!("[{:.0}% (Bypassed: 0%)]", settings.gate_aggressiveness)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(if sel_3 { " ▶ Aggressiveness           " } else { "   Aggressiveness           " }, if sel_3 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(aggro_val_str, if settings.gate_enabled { Style::default().fg(Color::White) } else { Style::default().fg(Color::DarkGray) }),
                    ]));
                    // Silence Attenuation
                    let sel_4 = selected_noise_idx == 4;
                    let atten_val_str = if settings.gate_enabled {
                        format!("[{:.0} dB]", settings.silence_attenuation)
                    } else {
                        format!("[{:.0} dB (Bypassed: 0 dB)]", settings.silence_attenuation)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(if sel_4 { " ▶ Silence Attenuation      " } else { "   Silence Attenuation      " }, if sel_4 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(atten_val_str, if settings.gate_enabled { Style::default().fg(Color::White) } else { Style::default().fg(Color::DarkGray) }),
                    ]));
                    // Hangover
                    let sel_5 = selected_noise_idx == 5;
                    let hangover_val_str = if settings.gate_enabled {
                        format!("[{} ms]", settings.hangover_ms)
                    } else {
                        format!("[{} ms (Bypassed)]", settings.hangover_ms)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(if sel_5 { " ▶ Gate Hangover            " } else { "   Gate Hangover            " }, if sel_5 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(hangover_val_str, if settings.gate_enabled { Style::default().fg(Color::White) } else { Style::default().fg(Color::DarkGray) }),
                    ]));
                    lines.push(Line::from(""));

                    lines.push(Line::from(Span::styled("LEVEL & OUTPUT OPTIONS", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                    // Output Gain
                    let sel_6 = selected_noise_idx == 6;
                    lines.push(Line::from(vec![
                        Span::styled(if sel_6 { " ▶ Output Gain              " } else { "   Output Gain              " }, if sel_6 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(format!("[{:+.1} dB]", settings.output_gain_db), Style::default().fg(Color::White)),
                    ]));
                    // Hear My Voice
                    let sel_7 = selected_noise_idx == 7;
                    lines.push(Line::from(vec![
                        Span::styled(if sel_7 { " ▶ Hear My Voice (Monitor)  " } else { "   Hear My Voice (Monitor)  " }, if sel_7 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(if settings.hear_voice { "[ON (Press 'v')]" } else { "[OFF (Press 'v')]" }, if settings.hear_voice { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) }),
                    ]));
                    // Virtual Device Toggle
                    let sel_8 = selected_noise_idx == 8;
                    lines.push(Line::from(vec![
                        Span::styled(if sel_8 { " ▶ Create Virtual Device    " } else { "   Create Virtual Device    " }, if sel_8 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                        Span::styled(if settings.create_virtual_device { "[ENABLED]" } else { "[DISABLED (Default)]" }, if settings.create_virtual_device { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Green) }),
                    ]));

                    let p = Paragraph::new(lines)
                        .block(Block::default().borders(Borders::ALL).title(" Microphone Controls (j/k: select, h/l: ±1, Enter: edit/toggle) "));
                    f.render_widget(p, main_chunks[0]);
                }
                ActivePanel::Presets => {
                    let selected_idx = list_state.selected().unwrap_or(0);
                    let preset_items: Vec<ListItem> = presets
                        .iter()
                        .enumerate()
                        .map(|(idx, p)| {
                            let is_active = p.name.eq_ignore_ascii_case(&active_preset_str) && is_engine_running();
                            let prefix = if is_active { " ▶ " } else { "   " };
                            let style = if is_active {
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                            } else if idx == selected_idx {
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(format!("{}{:<18} {}", prefix, p.name, p.description)).style(style)
                        })
                        .collect();

                    let list = List::new(preset_items)
                        .block(Block::default().borders(Borders::ALL).title(" Voice Presets (j/k: select, Enter: activate) "))
                        .highlight_symbol(">> ");
                    f.render_stateful_widget(list, main_chunks[0], &mut list_state);
                }
                ActivePanel::SpeakerDsp => {
                    let selected_idx = speaker_state.selected().unwrap_or(0);
                    let mut spk_items = Vec::new();

                    for (idx, v) in SpeakerVoicing::ALL.iter().enumerate() {
                        let is_active = *v == *current_voicing && *dsp_enabled;
                        let prefix = if is_active { " ● " } else { " ○ " };
                        let style = if is_active {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else if idx == selected_idx {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        spk_items.push(ListItem::new(format!("{}{:<24} {}", prefix, v.display_name(), v.description())).style(style));
                    }

                    let on_idx = 4;
                    let off_idx = 5;
                    spk_items.push(ListItem::new(format!(" {} DSP On (Active on internal speakers)", if *dsp_enabled { "✔" } else { " " })).style(
                        if selected_idx == on_idx { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }
                    ));
                    spk_items.push(ListItem::new(format!(" {} DSP Off (Bypassed to flat speakers)", if !*dsp_enabled { "✔" } else { " " })).style(
                        if selected_idx == off_idx { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }
                    ));

                    let list = List::new(spk_items)
                        .block(Block::default().borders(Borders::ALL).title(" Dolby Atmos / DAX3 Profiles (j/k: select, Enter: apply) "))
                        .highlight_symbol(">> ");
                    f.render_stateful_widget(list, main_chunks[0], &mut speaker_state);
                }
            }

            // Right Panel: Clean Meters & Signal Telemetry
            let noise_filter_status = if settings.rnnoise_enabled {
                Span::styled(format!("RNNoise ({:.0}% Blend)", settings.denoise_blend), Style::default().fg(Color::White))
            } else {
                Span::styled("BYPASS (Raw Passthrough)", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            };

            let noise_gate_status = if settings.gate_enabled {
                Span::styled(format!("Speech Gate ({:.0}% Aggro)", settings.gate_aggressiveness), Style::default().fg(Color::White))
            } else {
                Span::styled("BYPASS (Gate Open)", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            };

            let gain_str = format!("{:+.1} dB Output Gain", settings.output_gain_db);

            if settings.telemetry_enabled {
                let clipping = peak_lvl >= 0.99;
                let tele_lines = vec![
                    Line::from(Span::styled("LIVE TELEMETRY [ACTIVE]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                    Line::from(vec![
                        Span::styled("  Input Level   : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{:.1} dBFS", in_rms_db), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Output Level  : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{:.1} dBFS", out_rms_db), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  True Peak     : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{:.2} FS", peak_lvl), Style::default().fg(if clipping { Color::Red } else { Color::Green })),
                    ]),
                    Line::from(vec![
                        Span::styled("  Clipping      : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(if clipping { "[YES]" } else { "[NO]" }, Style::default().fg(if clipping { Color::Red } else { Color::Green }).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Voice VAD     : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{:.0}%", vad_pct), Style::default().fg(Color::Cyan)),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled("SIGNAL PIPELINE", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                    Line::from(vec![
                        Span::styled("  Source Node   : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("ghelper-audio (Clean Mic)", Style::default().fg(Color::Green)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Noise Filter  : ", Style::default().fg(Color::DarkGray)),
                        noise_filter_status,
                    ]),
                    Line::from(vec![
                        Span::styled("  Noise Gate    : ", Style::default().fg(Color::DarkGray)),
                        noise_gate_status,
                    ]),
                    Line::from(vec![
                        Span::styled("  Pre-HPF       : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("70 Hz Sub-Rumble Cut", Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Broadcast EQ  : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("9-Band Parametric", Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Compressor    : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("3:1 Leveling ({})", gain_str), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  True Limiter  : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("-1 dBFS Brickwall", Style::default().fg(Color::White)),
                    ]),
                ];

                let tele_widget = Paragraph::new(tele_lines)
                    .block(Block::default().borders(Borders::ALL).title(" Signal Telemetry (Press 't' to pause) "));
                f.render_widget(tele_widget, main_chunks[1]);
            } else {
                let static_lines = vec![
                    Line::from(Span::styled("LIVE TELEMETRY [OFF]", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))),
                    Line::from(""),
                    Line::from(Span::styled("Telemetry paused to conserve CPU/RAM.", Style::default().fg(Color::DarkGray))),
                    Line::from(Span::styled("Press 't' to enable live RMS meters.", Style::default().fg(Color::Cyan))),
                    Line::from(""),
                    Line::from(Span::styled("SIGNAL PIPELINE", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                    Line::from(vec![
                        Span::styled("  Source Node   : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("ghelper-audio (Clean Mic)", Style::default().fg(Color::Green)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Noise Filter  : ", Style::default().fg(Color::DarkGray)),
                        noise_filter_status,
                    ]),
                    Line::from(vec![
                        Span::styled("  Noise Gate    : ", Style::default().fg(Color::DarkGray)),
                        noise_gate_status,
                    ]),
                    Line::from(vec![
                        Span::styled("  Pre-HPF       : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("70 Hz Sub-Rumble Cut", Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Broadcast EQ  : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("9-Band Parametric", Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  Compressor    : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("3:1 Leveling ({})", gain_str), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("  True Limiter  : ", Style::default().fg(Color::DarkGray)),
                        Span::styled("-1 dBFS Brickwall", Style::default().fg(Color::White)),
                    ]),
                ];

                let tele_widget = Paragraph::new(static_lines)
                    .block(Block::default().borders(Borders::ALL).title(" Signal Pipeline (Press 't' to view live meters) "));
                f.render_widget(tele_widget, main_chunks[1]);
            }

            // 4. Footer Keybindings
            let footer = Paragraph::new(Line::from(vec![
                Span::styled(" [1/2/3/Tab] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Tab   "),
                Span::styled(" [j/k] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Select   "),
                Span::styled(" [h/l] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" ±1 Adjust   "),
                Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Set   "),
                Span::styled(" [v] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Monitor   "),
                Span::styled(" [t] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Telemetry   "),
                Span::styled(" [r] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Reset   "),
                Span::styled(" [Space] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Clean Mic   "),
                Span::styled(" [q/Esc] ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::raw(" Quit "),
            ]))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
            f.render_widget(footer, chunks[3]);

            // 5. Centered Numeric Input Modal
            if input_modal_active {
                let area = centered_rect(44, 20, f.area());
                f.render_widget(Clear, area);

                let modal_lines = vec![
                    Line::from(Span::styled(format!("Set {}", input_target_control.name()), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                    Line::from(Span::styled(format!("Unit: {}  |  {}", input_target_control.unit(), input_target_control.bounds_hint()), Style::default().fg(Color::DarkGray))),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("Value: "),
                        Span::styled(&input_buffer, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(Color::White)),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled("Press [Enter] to apply, [Esc] to cancel", Style::default().fg(Color::DarkGray))),
                ];

                let modal = Paragraph::new(modal_lines)
                    .block(Block::default().borders(Borders::ALL).title(" Custom Value ").border_style(Style::default().fg(Color::Cyan)));
                f.render_widget(modal, area);
            }
        })?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if input_modal_active {
                        match key.code {
                            KeyCode::Esc => {
                                input_modal_active = false;
                                input_buffer.clear();
                            }
                            KeyCode::Enter => {
                                if let Ok(val) = input_buffer.trim().parse::<f32>() {
                                    match input_target_control {
                                        NoiseControlItem::DenoiseBlend => {
                                            settings.denoise_blend = val.clamp(0.0, 100.0);
                                        }
                                        NoiseControlItem::GateAggro => {
                                            settings.gate_aggressiveness = val.clamp(0.0, 100.0);
                                        }
                                        NoiseControlItem::SilenceAtten => {
                                            settings.silence_attenuation = val.clamp(-60.0, 0.0);
                                        }
                                        NoiseControlItem::Hangover => {
                                            settings.hangover_ms = (val.round() as u32).clamp(0, 1000);
                                        }
                                        NoiseControlItem::OutputGain => {
                                            settings.output_gain_db = val.clamp(-24.0, 12.0);
                                        }
                                        _ => {}
                                    }
                                    let _ = settings.apply_dsp_params();
                                    let _ = settings.save();
                                }
                                input_modal_active = false;
                                input_buffer.clear();
                            }
                            KeyCode::Backspace => {
                                input_buffer.pop();
                            }
                            KeyCode::Char(c) => {
                                if c.is_ascii_digit() || c == '.' || c == '-' {
                                    input_buffer.push(c);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                // Flush any pending dirty state before exit
                                if dsp_dirty {
                                    let _ = settings.apply_dsp_params();
                                    let _ = settings.save();
                                }
                                break;
                            }
                            KeyCode::Tab => {
                                active_panel = match active_panel {
                                    ActivePanel::Presets => ActivePanel::NoiseAndLevel,
                                    ActivePanel::NoiseAndLevel => ActivePanel::SpeakerDsp,
                                    ActivePanel::SpeakerDsp => ActivePanel::Presets,
                                };
                            }
                            KeyCode::Char('1') => active_panel = ActivePanel::Presets,
                            KeyCode::Char('2') => active_panel = ActivePanel::NoiseAndLevel,
                            KeyCode::Char('3') => active_panel = ActivePanel::SpeakerDsp,
                            KeyCode::Char('v') | KeyCode::Char('V') => {
                                settings.hear_voice = !settings.hear_voice;
                                CleanMicSettings::sync_hear_voice(settings.hear_voice);
                                let _ = settings.save();
                                status_msg = if settings.hear_voice {
                                    "Hear My Voice: ON (Sidetone Connected)".to_string()
                                } else {
                                    "Hear My Voice: OFF".to_string()
                                };
                                status_msg_time = Instant::now();
                            }
                            KeyCode::Char('t') | KeyCode::Char('T') => {
                                settings.telemetry_enabled = !settings.telemetry_enabled;
                                mic_monitor.set_enabled(settings.telemetry_enabled);
                                let _ = settings.save();
                                status_msg = if settings.telemetry_enabled {
                                    "Live Telemetry: ENABLED".to_string()
                                } else {
                                    "Live Telemetry: DISABLED (CPU Saver)".to_string()
                                };
                                status_msg_time = Instant::now();
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                match active_panel {
                                    ActivePanel::NoiseAndLevel => {
                                        if selected_noise_idx + 1 < NoiseControlItem::ALL.len() {
                                            selected_noise_idx += 1;
                                        }
                                    }
                                    ActivePanel::Presets => {
                                        let curr = list_state.selected().unwrap_or(0);
                                        let next = (curr + 1).min(presets.len().saturating_sub(1));
                                        list_state.select(Some(next));
                                    }
                                    ActivePanel::SpeakerDsp => {
                                        let curr = speaker_state.selected().unwrap_or(0);
                                        let next = (curr + 1).min(5);
                                        speaker_state.select(Some(next));
                                    }
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                match active_panel {
                                    ActivePanel::NoiseAndLevel => {
                                        if selected_noise_idx > 0 {
                                            selected_noise_idx -= 1;
                                        }
                                    }
                                    ActivePanel::Presets => {
                                        let curr = list_state.selected().unwrap_or(0);
                                        let prev = curr.saturating_sub(1);
                                        list_state.select(Some(prev));
                                    }
                                    ActivePanel::SpeakerDsp => {
                                        let curr = speaker_state.selected().unwrap_or(0);
                                        let prev = curr.saturating_sub(1);
                                        speaker_state.select(Some(prev));
                                    }
                                }
                            }
                            KeyCode::Char('h') | KeyCode::Left => {
                                if active_panel == ActivePanel::NoiseAndLevel {
                                    match NoiseControlItem::ALL[selected_noise_idx] {
                                        NoiseControlItem::RnnoiseToggle => {
                                            settings.rnnoise_enabled = !settings.rnnoise_enabled;
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::DenoiseBlend => {
                                            settings.denoise_blend = (settings.denoise_blend - 1.0).clamp(0.0, 100.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::GateToggle => {
                                            settings.gate_enabled = !settings.gate_enabled;
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::GateAggro => {
                                            settings.gate_aggressiveness = (settings.gate_aggressiveness - 1.0).clamp(0.0, 100.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::SilenceAtten => {
                                            settings.silence_attenuation = (settings.silence_attenuation - 1.0).clamp(-60.0, 0.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::Hangover => {
                                            settings.hangover_ms = settings.hangover_ms.saturating_sub(10).clamp(0, 1000);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::OutputGain => {
                                            settings.output_gain_db = (settings.output_gain_db - 0.5).clamp(-24.0, 12.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::HearVoice => {
                                            settings.hear_voice = !settings.hear_voice;
                                            CleanMicSettings::sync_hear_voice(settings.hear_voice);
                                            let _ = settings.save();
                                        }
                                        NoiseControlItem::VirtualDevice => {
                                            settings.create_virtual_device = !settings.create_virtual_device;
                                            let _ = settings.save();
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('l') | KeyCode::Right => {
                                if active_panel == ActivePanel::NoiseAndLevel {
                                    match NoiseControlItem::ALL[selected_noise_idx] {
                                        NoiseControlItem::RnnoiseToggle => {
                                            settings.rnnoise_enabled = !settings.rnnoise_enabled;
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::DenoiseBlend => {
                                            settings.denoise_blend = (settings.denoise_blend + 1.0).clamp(0.0, 100.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::GateToggle => {
                                            settings.gate_enabled = !settings.gate_enabled;
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::GateAggro => {
                                            settings.gate_aggressiveness = (settings.gate_aggressiveness + 1.0).clamp(0.0, 100.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::SilenceAtten => {
                                            settings.silence_attenuation = (settings.silence_attenuation + 1.0).clamp(-60.0, 0.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::Hangover => {
                                            settings.hangover_ms = (settings.hangover_ms + 10).clamp(0, 1000);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::OutputGain => {
                                            settings.output_gain_db = (settings.output_gain_db + 0.5).clamp(-24.0, 12.0);
                                            dsp_dirty = true;
                                            last_dirty_time = Instant::now();
                                        }
                                        NoiseControlItem::HearVoice => {
                                            settings.hear_voice = !settings.hear_voice;
                                            CleanMicSettings::sync_hear_voice(settings.hear_voice);
                                            let _ = settings.save();
                                        }
                                        NoiseControlItem::VirtualDevice => {
                                            settings.create_virtual_device = !settings.create_virtual_device;
                                            let _ = settings.save();
                                        }
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                match active_panel {
                                    ActivePanel::NoiseAndLevel => {
                                        let ctrl = NoiseControlItem::ALL[selected_noise_idx];
                                        if ctrl.is_numeric() {
                                            input_modal_active = true;
                                            input_target_control = ctrl;
                                            input_buffer = match ctrl {
                                                NoiseControlItem::DenoiseBlend => format!("{:.0}", settings.denoise_blend),
                                                NoiseControlItem::GateAggro => format!("{:.0}", settings.gate_aggressiveness),
                                                NoiseControlItem::SilenceAtten => format!("{:.0}", settings.silence_attenuation),
                                                NoiseControlItem::Hangover => format!("{}", settings.hangover_ms),
                                                NoiseControlItem::OutputGain => format!("{:.1}", settings.output_gain_db),
                                                _ => String::new(),
                                            };
                                        } else {
                                            match ctrl {
                                                NoiseControlItem::RnnoiseToggle => {
                                                    settings.rnnoise_enabled = !settings.rnnoise_enabled;
                                                    let _ = settings.apply_dsp_params();
                                                    let _ = settings.save();
                                                }
                                                NoiseControlItem::GateToggle => {
                                                    settings.gate_enabled = !settings.gate_enabled;
                                                    let _ = settings.apply_dsp_params();
                                                    let _ = settings.save();
                                                }
                                                NoiseControlItem::HearVoice => {
                                                    settings.hear_voice = !settings.hear_voice;
                                                    CleanMicSettings::sync_hear_voice(settings.hear_voice);
                                                    let _ = settings.save();
                                                }
                                                NoiseControlItem::VirtualDevice => {
                                                    settings.create_virtual_device = !settings.create_virtual_device;
                                                    let _ = settings.save();
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    ActivePanel::Presets => {
                                        let idx = list_state.selected().unwrap_or(0);
                                        let preset = &presets[idx];
                                        if let Ok(msg) = crate::cli::apply_voice_silent(&preset.name) {
                                            status_msg = msg;
                                        }
                                        status_msg_time = Instant::now();
                                        active_preset_str = preset.name.clone();
                                        let _ = mgr.set_active_preset(&preset.name);
                                        last_status_check = Instant::now() - Duration::from_secs(10);
                                    }
                                    ActivePanel::SpeakerDsp => {
                                        let idx = speaker_state.selected().unwrap_or(0);
                                        match idx {
                                            0 => { let _ = SpeakerDsp::apply_voicing(SpeakerVoicing::Custom); }
                                            1 => { let _ = SpeakerDsp::apply_voicing(SpeakerVoicing::Dynamic); }
                                            2 => { let _ = SpeakerDsp::apply_voicing(SpeakerVoicing::Music); }
                                            3 => { let _ = SpeakerDsp::apply_voicing(SpeakerVoicing::Balanced); }
                                            4 => { let _ = SpeakerDsp::set_enabled(true); }
                                            5 => { let _ = SpeakerDsp::set_enabled(false); }
                                            _ => {}
                                        }
                                        last_status_check = Instant::now() - Duration::from_secs(10);
                                    }
                                }
                            }
                            KeyCode::Char('r') => {
                                if active_panel == ActivePanel::NoiseAndLevel {
                                    settings.rnnoise_enabled = true;
                                    settings.denoise_blend = 100.0;
                                    settings.gate_enabled = true;
                                    settings.gate_aggressiveness = 85.0;
                                    settings.silence_attenuation = -36.0;
                                    settings.hangover_ms = 180;
                                    settings.output_gain_db = 2.0;
                                    let _ = settings.apply_dsp_params();
                                    let _ = settings.save();
                                    status_msg = "Restored optimal microphone defaults".to_string();
                                    status_msg_time = Instant::now();
                                }
                            }
                            KeyCode::Char(' ') => {
                                if let Ok(msg) = crate::cli::set_clean_silent() {
                                    status_msg = msg;
                                }
                                status_msg_time = Instant::now();
                                active_preset_str = "Natural Clean".to_string();
                                last_status_check = Instant::now() - Duration::from_secs(10);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn get_current_default_source() -> String {
    let output = Command::new("wpctl")
        .args(["inspect", "@DEFAULT_AUDIO_SOURCE@"])
        .output();
    if let Ok(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if line.contains("node.description =") {
                if let Some((_, val)) = line.split_once('=') {
                    let desc = val.trim().trim_matches('"').to_string();
                    if desc.eq_ignore_ascii_case("Dusky Mic") {
                        return "MicStudio Clean".to_string();
                    }
                    return desc;
                }
            }
        }
    }
    "MicStudio Clean".to_string()
}
