//! Minimal Voice-Only Controller TUI for MicStudio.

pub mod app;
pub mod render;

pub use app::AppState;
pub use render::render_ui;

use crate::monitor::MicMonitor;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::stdout;
use std::time::Duration;

pub fn run_tui() -> Result<()> {
    let state = AppState::new()?;
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let res = run_event_loop_with_state(&mut terminal, state);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

pub fn run_tui_spectrograph(waterfall: bool) -> Result<()> {
    let mut state = AppState::new()?;
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    state.spectrograph_fullscreen = true;
    if waterfall {
        state.spectrograph_mode = app::SpectrographMode::Waterfall;
    }
    let res = run_event_loop_with_state(&mut terminal, state);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn run_event_loop_with_state(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    mut state: AppState,
) -> Result<()> {
    // If sidetone was persisted as enabled, establish PipeWire monitor links immediately on launch
    if state.sidetone_enabled {
        crate::config::CleanMicSettings::sync_hear_voice(true);
        state.sidetone_guarded = crate::config::CleanMicSettings::is_sidetone_guarded();
    }

    let should_monitor =
        state.meter_enabled || state.spectrograph_enabled || state.spectrograph_fullscreen;
    let mut monitor = MicMonitor::new(should_monitor);
    let mut tick_counter: usize = 0;

    loop {
        // Redraw current screen state
        terminal.draw(|frame| render_ui(frame, &state))?;

        // Periodically refresh output sink guard state (approx every 1s)
        tick_counter = (tick_counter + 1) % 30;
        if tick_counter == 0 {
            state.sidetone_guarded = crate::config::CleanMicSettings::is_sidetone_guarded();
        }

        // 30Hz when meter or spectrograph is on (smooth 33ms animation), 500ms slow tick when idle
        let active_monitoring =
            state.meter_enabled || state.spectrograph_enabled || state.spectrograph_fullscreen;
        let timeout = if active_monitoring {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(500)
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                // Ignore key release events on Windows/terminals that report them
                if key.kind == crossterm::event::KeyEventKind::Release {
                    continue;
                }

                if state.show_inputs {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => state.show_inputs = false,
                        KeyCode::Char('j') | KeyCode::Down => {
                            state.input_cursor = (state.input_cursor + 1).min(state.input_devices.len());
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            state.input_cursor = state.input_cursor.saturating_sub(1);
                        }
                        KeyCode::Enter => {
                            let name = if state.input_cursor == 0 { "" } else {
                                &state.input_devices[state.input_cursor - 1].name
                            };
                            let result = crate::input::select(name);
                            state.set_status(match result {
                                Ok(()) => "Input selection saved; automatic fallback remains enabled.".into(),
                                Err(error) => format!("Input selection failed: {error}"),
                            });
                            state.show_inputs = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                if state.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter => {
                            state.close_help();
                        }
                        _ => {}
                    }
                    continue;
                }

                if let Some(action) = state.confirm_action.clone() {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Enter => {
                            match action {
                                crate::ui::app::ConfirmAction::ResetSelected(_) => {
                                    let _ = state.reset_selected_preset();
                                }
                                crate::ui::app::ConfirmAction::ResetAll => {
                                    let _ = state.reset_all_to_factory();
                                }
                                crate::ui::app::ConfirmAction::QuitUnsaved => {
                                    let _ = state.save_current_tweaks();
                                    state.should_quit = true;
                                }
                            }
                            state.confirm_action = None;
                        }
                        KeyCode::Char('w') if matches!(action, crate::ui::app::ConfirmAction::QuitUnsaved) => {
                            let _ = state.save_current_tweaks();
                            state.confirm_action = None;
                            state.should_quit = true;
                        }
                        KeyCode::Char('q') | KeyCode::Char('n') if matches!(action, crate::ui::app::ConfirmAction::QuitUnsaved) => {
                            state.discard_and_restore_saved();
                            state.confirm_action = None;
                            state.should_quit = true;
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            state.confirm_action = None;
                            state.set_status("Cancelled.".to_string());
                        }
                        _ => {}
                    }
                    continue;
                }

                // If in search input mode
                if state.search_mode {
                    match key.code {
                        KeyCode::Esc => {
                            state.exit_search(true);
                        }
                        KeyCode::Enter => {
                            state.exit_search(false);
                        }
                        KeyCode::Backspace => {
                            state.handle_search_backspace();
                        }
                        KeyCode::Char(c) => {
                            state.handle_search_char(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                match (key.code, key.modifiers) {
                    (KeyCode::Char('i'), _) => {
                        match crate::input::devices() {
                            Ok(devices) => {
                                let selected = crate::input::selected().unwrap_or_default();
                                state.input_cursor = devices.iter().position(|d| d.name == selected)
                                    .map(|index| index + 1).unwrap_or(0);
                                state.input_devices = devices;
                                state.show_inputs = true;
                            }
                            Err(error) => state.set_status(format!("Input discovery failed: {error}")),
                        }
                    }
                    (KeyCode::Char('q'), _) => {
                        state.request_quit();
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        state.request_quit();
                    }
                    // Shift+j / J: cycle down through Live Controls
                    (KeyCode::Char('J'), _) => {
                        state.cycle_control_next();
                    }
                    (KeyCode::Char('j'), m) if m.contains(KeyModifiers::SHIFT) => {
                        state.cycle_control_next();
                    }
                    // Shift+k / K: cycle up through Live Controls
                    (KeyCode::Char('K'), _) => {
                        state.cycle_control_prev();
                    }
                    (KeyCode::Char('k'), m) if m.contains(KeyModifiers::SHIFT) => {
                        state.cycle_control_prev();
                    }
                    // j / Down: move down in presets list
                    (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                        state.move_cursor_down();
                    }
                    // k / Up: move up in presets list
                    (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                        state.move_cursor_up();
                    }
                    (KeyCode::Enter, _) => {
                        let _ = state.apply_selected();
                        monitor.restart();
                        if state.show_details_modal {
                            state.close_details_modal();
                        }
                    }
                    (KeyCode::Char(' '), _) => {
                        state.toggle_details_modal();
                    }
                    (KeyCode::Char('c'), _) => {
                        let _ = state.revert_clean();
                        monitor.restart();
                    }
                    (KeyCode::Char('/'), _) => {
                        state.enter_search();
                    }
                    (KeyCode::Char('p'), _) => {
                        state.trigger_audition();
                    }
                    (KeyCode::F(5), _) => {
                        let _ = state.reload_presets();
                        monitor.restart();
                    }
                    (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                        let _ = state.reload_presets();
                        monitor.restart();
                    }
                    (KeyCode::Char('R'), _) => {
                        state.request_reset_all();
                    }
                    (KeyCode::Char('r'), m) if m.contains(KeyModifiers::SHIFT) => {
                        state.request_reset_all();
                    }
                    (KeyCode::Char('r'), _) => {
                        state.request_reset_selected();
                    }
                    (KeyCode::Char('w'), _) => {
                        let _ = state.save_current_tweaks();
                    }
                    (KeyCode::Char(c), _) if c.is_ascii_digit() && c != '0' => {
                        let num = (c as u8 - b'0') as usize;
                        state.jump_to_preset(num);
                    }
                    (KeyCode::Tab, KeyModifiers::NONE) => {
                        state.cycle_control_next();
                    }
                    (KeyCode::BackTab, _) => {
                        state.cycle_control_prev();
                    }
                    (KeyCode::Char('h'), _) | (KeyCode::Left, _) => {
                        let _ = state.adjust_selected_control(-1);
                        monitor.set_enabled(
                            state.meter_enabled
                                || state.spectrograph_enabled
                                || state.sidetone_enabled,
                        );
                    }
                    (KeyCode::Char('l'), _) | (KeyCode::Right, _) => {
                        let _ = state.adjust_selected_control(1);
                        monitor.set_enabled(
                            state.meter_enabled
                                || state.spectrograph_enabled
                                || state.sidetone_enabled,
                        );
                    }
                    (KeyCode::Char('m'), _) | (KeyCode::Char('M'), _) | (KeyCode::Char('s'), _) => {
                        state.toggle_spectrograph_fullscreen();
                        monitor.set_enabled(
                            state.meter_enabled
                                || state.spectrograph_enabled
                                || state.spectrograph_fullscreen
                                || state.sidetone_enabled,
                        );
                        crate::config::ensure_meter_linked();
                    }
                    (KeyCode::Char('g'), _) | (KeyCode::Char('G'), _) => {
                        state.toggle_spectrograph_mode();
                    }
                    (KeyCode::Char('v'), _) => {
                        let _ = state.toggle_sidetone();
                        monitor.set_enabled(
                            state.meter_enabled
                                || state.spectrograph_enabled
                                || state.spectrograph_fullscreen
                                || state.sidetone_enabled,
                        );
                        crate::config::ensure_meter_linked();
                    }
                    (KeyCode::Char('t'), _) => {
                        state.toggle_meter();
                        monitor.set_enabled(
                            state.meter_enabled
                                || state.spectrograph_enabled
                                || state.spectrograph_fullscreen
                                || state.sidetone_enabled,
                        );
                        crate::config::ensure_meter_linked();
                    }
                    (KeyCode::Char('?'), _) => {
                        state.toggle_help();
                    }
                    (KeyCode::Esc, _) => {
                        if state.show_details_modal {
                            state.close_details_modal();
                        } else if state.spectrograph_fullscreen {
                            state.spectrograph_fullscreen = false;
                        } else if !state.search_query.is_empty() {
                            state.search_query.clear();
                            state.cursor = 0;
                        } else {
                            state.close_help();
                        }
                    }
                    _ => {}
                }
            }
        }

        if state.should_quit {
            break;
        }

        // Update live spectrum and meter telemetry
        if state.meter_enabled || state.spectrograph_enabled || state.spectrograph_fullscreen {
            let spec = monitor.get_spectrum();
            state.input_rms_db = spec.rms_db;
            state.spectrum = spec;
        }
    }

    // Ensure any sidetone monitor links are severed upon quitting MicStudio
    crate::config::CleanMicSettings::sync_hear_voice(false);

    Ok(())
}
