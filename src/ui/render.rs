//! Pure Ratatui rendering function for MicStudio.

use crate::ui::app::AppState;
use crate::ui::theme::ColorPalette;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Truncate string to `max_width` display columns using an ellipsis if necessary.
pub fn fit_string(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let w = UnicodeWidthStr::width(s);
    if w <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let target = max_width.saturating_sub(1);
    let mut current_width = 0;
    let mut out = String::new();
    for ch in s.chars() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_w > target {
            break;
        }
        current_width += ch_w;
        out.push(ch);
    }
    out.push('…');
    out
}

pub fn render_ui(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let palette = state.theme.palette();

    // Enforce 80x24 minimum resolution without panicking
    if area.width < 80 || area.height < 24 {
        let msg = format!(
            "terminal too small (need 80x24, current: {}x{})",
            area.width, area.height
        );
        let p = Paragraph::new(msg).alignment(Alignment::Center).style(
            Style::default()
                .fg(palette.accent_warning)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_widget(p, area);
        return;
    }

    // Main 3-way vertical split: Top Bar (1), Middle (Min 20), Bottom Bar (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(20),
            Constraint::Length(1),
        ])
        .split(area);

    render_top_bar(frame, chunks[0], state, &palette);
    render_middle(frame, chunks[1], state, &palette);
    render_bottom_bar(frame, chunks[2], state, &palette);

    if state.show_help {
        render_help_dialog(frame, area, state, &palette);
    }
    if state.show_details_modal {
        render_details_modal(frame, area, state, &palette);
    }
    if let Some(ref action) = state.confirm_action {
        render_confirm_dialog(frame, area, action, &palette);
    }
}

fn render_top_bar(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let meter_spans = if !state.meter_enabled {
        vec![Span::styled(
            "Meter: off",
            Style::default().fg(palette.text_muted),
        )]
    } else {
        let norm = ((state.input_rms_db + 60.0) / 6.0).clamp(0.0, 10.0).round() as usize;
        let filled = "━".repeat(norm);
        let empty = "─".repeat(10 - norm);
        let bar = format!("[{}{}]", filled, empty);
        let peak_db = if state.spectrum.peak_level > 1e-5 {
            20.0 * state.spectrum.peak_level.log10()
        } else {
            -96.0
        };
        let is_hw_clip = state.spectrum.hw_adc_clipped;
        let is_digital_limiting = state.spectrum.digital_limiting || (!is_hw_clip && peak_db >= -2.0);

        let mut spans = vec![
            Span::styled("Meter: ", Style::default().fg(palette.text)),
            Span::styled(
                bar,
                Style::default().fg(if is_hw_clip || norm > 8 {
                    palette.accent_danger
                } else if norm > 5 {
                    palette.accent_warning
                } else {
                    palette.accent_secondary
                }),
            ),
        ];
        if state.sidetone_enabled {
            spans.push(Span::styled(
                format!(" {:+5.1} dBFS", state.input_rms_db),
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" (Pk: {:+5.1})", peak_db),
                Style::default().fg(palette.text_muted),
            ));
        }
        if is_hw_clip {
            spans.push(Span::styled(
                " [Input clipping suspected - check gain]",
                Style::default().fg(palette.accent_danger).add_modifier(Modifier::BOLD),
            ));
        } else if is_digital_limiting {
            spans.push(Span::styled(
                " [LIMITING]",
                Style::default().fg(palette.accent_warning).add_modifier(Modifier::BOLD),
            ));
        }
        spans
    };

    let sep = Span::styled(" │ ", Style::default().fg(palette.border));

    let mut spans = vec![
        Span::styled(
            " MICSTUDIO ",
            Style::default()
                .fg(palette.badge_fg)
                .bg(palette.badge_bg)
                .add_modifier(Modifier::BOLD),
        ),
        sep.clone(),
    ];
    spans.extend(meter_spans);

    // Right side theme indicator
    spans.push(sep);
    spans.push(Span::styled("Theme: ", Style::default().fg(palette.text_muted)));
    spans.push(Span::styled(
        state.theme.short_name(),
        Style::default()
            .fg(palette.accent_purple)
            .add_modifier(Modifier::BOLD),
    ));

    let line = Line::from(spans);
    let p = Paragraph::new(line);
    frame.render_widget(p, area);
}

fn render_middle(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    if state.spectrograph_fullscreen {
        render_fullscreen_spectrograph(frame, area, state, palette);
        return;
    }

    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(50)])
        .split(area);

    render_left_pane(frame, horizontal_chunks[0], state, palette);
    render_right_pane(frame, horizontal_chunks[1], state, palette);
}

enum VisualRow {
    CategoryHeader(String),
    PresetItem {
        filtered_index: usize,
        preset_idx: usize,
    },
}

fn render_left_pane(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let title = if state.search_mode {
        format!(" Presets [/{}_] ", state.search_query)
    } else if !state.search_query.is_empty() {
        format!(" Presets [/{}] ", state.search_query)
    } else {
        " Presets ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(palette.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let filtered = state.filtered_indices();
    if filtered.is_empty() {
        let p = Paragraph::new(" No matching presets").style(Style::default().fg(palette.text_muted));
        frame.render_widget(p, inner);
        return;
    }

    // Build visual list with category headers
    let mut visual_rows: Vec<VisualRow> = Vec::new();
    let mut last_category = String::new();
    let mut selected_visual_idx = 0;

    for (f_idx, &p_idx) in filtered.iter().enumerate() {
        let p = &state.presets[p_idx];
        if p.category != last_category {
            visual_rows.push(VisualRow::CategoryHeader(p.category.clone()));
            last_category = p.category.clone();
        }
        if f_idx == state.cursor {
            selected_visual_idx = visual_rows.len();
        }
        visual_rows.push(VisualRow::PresetItem {
            filtered_index: f_idx,
            preset_idx: p_idx,
        });
    }

    let pane_height = inner.height as usize;
    let mut scroll_top = 0;
    if selected_visual_idx >= pane_height {
        scroll_top = selected_visual_idx + 1 - pane_height;
    }
    if scroll_top > visual_rows.len().saturating_sub(pane_height) {
        scroll_top = visual_rows.len().saturating_sub(pane_height);
    }

    let inner_w = inner.width as usize;
    let mut rendered_lines = Vec::new();

    for row in visual_rows.iter().skip(scroll_top).take(pane_height) {
        match row {
            VisualRow::CategoryHeader(cat) => {
                let text = format!("── {} ──", cat);
                let line = Line::from(vec![Span::styled(
                    fit_string(&text, inner_w),
                    Style::default()
                        .fg(palette.accent_purple)
                        .add_modifier(Modifier::BOLD),
                )]);
                rendered_lines.push(line);
            }
            VisualRow::PresetItem {
                filtered_index,
                preset_idx,
            } => {
                let p = &state.presets[*preset_idx];
                let is_cursor = *filtered_index == state.cursor;
                let is_active = p.name.eq_ignore_ascii_case(&state.active_preset_name);

                let cursor_char = if is_cursor { ">" } else { " " };
                let verified_badge = if p.verified { "[V]" } else { "   " };
                let active_mark = if is_active { "[*]" } else { "[ ]" };

                let name_max = inner_w.saturating_sub(10);
                let fitted_name = fit_string(&p.name, name_max);

                let row_style = if is_cursor {
                    Style::default()
                        .bg(palette.select_bg)
                        .fg(palette.select_fg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(palette.text)
                };

                let verified_style = if p.verified {
                    Style::default().fg(palette.accent_secondary)
                } else {
                    Style::default().fg(palette.text_muted)
                };

                let active_style = if is_active {
                    Style::default()
                        .fg(palette.accent_secondary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(palette.text_muted)
                };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", cursor_char), row_style),
                    Span::styled(
                        format!("{} ", verified_badge),
                        if is_cursor { row_style } else { verified_style },
                    ),
                    Span::styled(
                        format!("{} ", active_mark),
                        if is_cursor { row_style } else { active_style },
                    ),
                    Span::styled(fitted_name, row_style),
                ]);
                rendered_lines.push(line);
            }
        }
    }

    let p = Paragraph::new(rendered_lines);
    frame.render_widget(p, inner);
}

fn render_right_pane(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    if state.sidetone_enabled {
        let detail_height = 4;
        let controls_height = if area.height <= 22 { 9 } else { 10 };
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(detail_height),
                Constraint::Min(6),
                Constraint::Length(controls_height),
            ])
            .split(area);

        render_preset_details(frame, vertical_chunks[0], state, palette);
        render_spectrograph_module(frame, vertical_chunks[1], state, palette);
        render_live_controls(frame, vertical_chunks[2], state, palette);
    } else {
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),
                Constraint::Length(10),
            ])
            .split(area);

        render_preset_details(frame, vertical_chunks[0], state, palette);
        render_live_controls(frame, vertical_chunks[1], state, palette);
    }
}

struct SpecRow {
    key: &'static str,
    val: String,
    style: Style,
}

fn build_compact_spec_rows(
    selected: &crate::presets::Preset,
    state: &AppState,
    _inner_w: usize,
    palette: &ColorPalette,
) -> Vec<SpecRow> {
    let mut rows = Vec::new();
    let is_active = selected
        .name
        .eq_ignore_ascii_case(&state.active_preset_name);

    let (state_text, state_style) = if is_active {
        (
            "● Active (Live on ghelper-audio)",
            Style::default()
                .fg(palette.accent_secondary)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "○ Preview (Enter to apply)",
            Style::default().fg(palette.accent_warning),
        )
    };
    rows.push(SpecRow {
        key: "State:",
        val: state_text.to_string(),
        style: state_style,
    });

    if selected.name.eq_ignore_ascii_case("Natural Clean") {
        rows.push(SpecRow {
            key: "Denoise:",
            val: format!("{:.0} dB (DPDFNet Neural AI)", state.dpdfnet_attenuation),
            style: Style::default().fg(palette.accent_secondary),
        });
        rows.push(SpecRow {
            key: "Quiet Boost:",
            val: if state.quiet_boost_enabled {
                format!("{:+.1} dB [ON] (Speech Leveler)", state.quiet_boost_db)
            } else {
                "OFF (+0 dB)".to_string()
            },
            style: if state.quiet_boost_enabled {
                Style::default()
                    .fg(palette.accent_secondary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.text_muted)
            },
        });
        rows.push(SpecRow {
            key: "Output Gain:",
            val: format!("{:+.1} dB", state.output_gain_db),
            style: Style::default().fg(palette.text),
        });
        rows.push(SpecRow {
            key: "Effect FX:",
            val: "Direct Passthrough (100% Clean)".to_string(),
            style: Style::default().fg(palette.accent_primary),
        });
        rows.push(SpecRow {
            key: "Anti-Shout:",
            val: "Active (-2.0 dBFS Limiter Guard)".to_string(),
            style: Style::default().fg(palette.accent_secondary),
        });
    } else {
        if selected.pitch_semitones.abs() > 0.01 {
            let p_val = if selected.formant_preserving {
                format!("{:+.1} st (Formants Preserved)", selected.pitch_semitones)
            } else {
                format!("{:+.1} st", selected.pitch_semitones)
            };
            rows.push(SpecRow {
                key: "Pitch:",
                val: p_val,
                style: Style::default().fg(palette.accent_primary),
            });
        }

        if selected.distortion_level > 0.01 {
            rows.push(SpecRow {
                key: "Tube Drive:",
                val: format!("{:.0}% Saturation", selected.distortion_level * 100.0),
                style: Style::default().fg(palette.accent_warning),
            });
        }

        if selected.ringmod_depth > 0.01 {
            let mod_val = if selected.ringmod_square > 0.5 {
                format!("{:.0} Hz Chopper Stutter", selected.ringmod_freq)
            } else {
                format!("{:.0} Hz Ring Modulator", selected.ringmod_freq)
            };
            rows.push(SpecRow {
                key: "Modulation:",
                val: mod_val,
                style: Style::default().fg(palette.accent_purple),
            });
        }

        if selected.bitcrush_bits < 24 || selected.downsample_rate < 48000 {
            rows.push(SpecRow {
                key: "Decimator:",
                val: format!(
                    "{}-bit / {} Hz Lo-Fi",
                    selected.bitcrush_bits, selected.downsample_rate
                ),
                style: Style::default().fg(palette.accent_warning),
            });
        }

        if selected.delay_mix > 0.01 {
            rows.push(SpecRow {
                key: "Echo/Delay:",
                val: format!(
                    "{:.0} ms ({:.0}% wet)",
                    selected.delay_time * 1000.0,
                    selected.delay_mix * 100.0
                ),
                style: Style::default().fg(palette.accent_primary),
            });
        }

        if selected.eq_gain.abs() > 0.1 {
            rows.push(SpecRow {
                key: "Tone EQ:",
                val: format!("{:+.1} dB @ {:.0} Hz", selected.eq_gain, selected.eq_freq),
                style: Style::default().fg(palette.accent_secondary),
            });
        }

        if selected.hpf_freq > 30.0 && selected.lpf_freq >= 19000.0 {
            rows.push(SpecRow {
                key: "Filter:",
                val: format!("{:.0} Hz HPF (Rumble Cut)", selected.hpf_freq),
                style: Style::default().fg(palette.text_muted),
            });
        } else if selected.lpf_freq < 19000.0 && selected.hpf_freq <= 30.0 {
            rows.push(SpecRow {
                key: "Filter:",
                val: format!("{:.0} Hz LPF (Bandwidth Cut)", selected.lpf_freq),
                style: Style::default().fg(palette.text_muted),
            });
        } else if selected.hpf_freq > 30.0 || selected.lpf_freq < 19000.0 {
            rows.push(SpecRow {
                key: "Bandpass:",
                val: format!("{:.0} Hz - {:.0} Hz", selected.hpf_freq, selected.lpf_freq),
                style: Style::default().fg(palette.text_muted),
            });
        }

        if selected.flanger_enabled {
            rows.push(SpecRow {
                key: "Flanger:",
                val: format!("{:.2} Hz Space LFO", selected.flanger_rate),
                style: Style::default().fg(palette.accent_primary),
            });
        }

        rows.push(SpecRow {
            key: "Denoise:",
            val: format!("{:.0} dB (DPDFNet AI)", state.dpdfnet_attenuation),
            style: Style::default().fg(palette.accent_secondary),
        });

        let boost_val = if is_active {
            state
                .presets
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(&state.active_preset_name))
                .map(|p| p.model_gain_db)
                .unwrap_or(2.0)
        } else {
            selected.model_gain_db
        };
        rows.push(SpecRow {
            key: "Voice Boost:",
            val: format!("{:+.1} dB", boost_val),
            style: Style::default().fg(palette.accent_warning),
        });

        let mix_pct = if is_active {
            state.effect_mix_pct
        } else {
            ((1.0 - selected.wet_dry_mix).clamp(0.0, 1.0) * 100.0).round() as u32
        };
        rows.push(SpecRow {
            key: "Effect Mix:",
            val: format!("{}% Wet", mix_pct),
            style: Style::default().fg(palette.accent_primary),
        });

        rows.push(SpecRow {
            key: "Anti-Shout:",
            val: "Active (-2.0 dBFS Limiter Guard)".to_string(),
            style: Style::default().fg(palette.accent_secondary),
        });
    }

    rows
}

fn render_preset_details(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let selected = state.selected_preset();
    let is_selected_active = selected
        .name
        .eq_ignore_ascii_case(&state.active_preset_name);

    let title = if is_selected_active {
        " Description [Active] ".to_string()
    } else {
        " Description ".to_string()
    };

    let border_style = if is_selected_active {
        Style::default().fg(palette.accent_secondary)
    } else {
        Style::default().fg(palette.border)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let p = Paragraph::new(selected.description.as_str())
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(palette.text));
    frame.render_widget(p, inner);
}

fn render_details_modal(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let popup_w = 68.min(area.width.saturating_sub(4));
    let popup_h = 17.min(area.height.saturating_sub(2));

    let popup_x = (area.width.saturating_sub(popup_w)) / 2;
    let popup_y = (area.height.saturating_sub(popup_h)) / 2;

    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
    frame.render_widget(Clear, popup_rect);

    let selected = state.selected_preset();
    let is_selected_active = selected
        .name
        .eq_ignore_ascii_case(&state.active_preset_name);

    let title = format!(" {} (Model Details & Chain) ", selected.name);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(palette.accent_primary));
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    let inner_w = inner.width as usize;
    let mut lines = Vec::new();

    let state_span = if is_selected_active {
        Span::styled(
            "● Active (Live on ghelper-audio)",
            Style::default()
                .fg(palette.accent_secondary)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "○ Inactive (Press Enter to Apply)",
            Style::default().fg(palette.text_muted),
        )
    };
    lines.push(Line::from(vec![
        Span::styled("State:        ", Style::default().fg(palette.accent_primary)),
        state_span,
    ]));
    lines.push(Line::from(vec![
        Span::styled("Category:     ", Style::default().fg(palette.accent_primary)),
        Span::styled(&selected.category, Style::default().fg(palette.text)),
    ]));

    let spec_rows = build_compact_spec_rows(selected, state, inner_w, palette);
    for row in spec_rows.iter().skip(1).take(7) {
        let key_str = format!("{:<14}", row.key);
        lines.push(Line::from(vec![
            Span::styled(key_str, Style::default().fg(palette.accent_primary)),
            Span::styled(&row.val, row.style),
        ]));
    }

    let chain_str = "DPDFNet AI -> LSP Parametric EQ -> LADSPA FX -> Leveler Comp -> Limiter";
    lines.push(Line::from(vec![
        Span::styled("Chain:        ", Style::default().fg(palette.accent_primary)),
        Span::styled(
            fit_string(chain_str, inner_w.saturating_sub(15)),
            Style::default().fg(palette.accent_warning),
        ),
    ]));

    lines.push(Line::raw(""));
    let hints = Line::from(vec![
        Span::styled(
            "[Enter]",
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Apply   ", Style::default().fg(palette.text_muted)),
        Span::styled(
            "[r]",
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Reset   ", Style::default().fg(palette.text_muted)),
        Span::styled(
            "[Space/Esc]",
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Close", Style::default().fg(palette.text_muted)),
    ]);
    lines.push(hints);

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_spectrograph_module(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let title = " Spectrograph [Active] (m: full graph) ".to_string();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(palette.border_active));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 10 || inner.height < 2 {
        return;
    }

    match state.spectrograph_mode {
        crate::ui::app::SpectrographMode::RtaBars => {
            render_rta_bars(frame, inner, state, palette);
        }
        crate::ui::app::SpectrographMode::Waterfall => {
            render_waterfall_heatmap(frame, inner, state, palette);
        }
    }
}

fn render_rta_bars(frame: &mut Frame, inner: Rect, state: &AppState, palette: &ColorPalette) {
    let w = inner.width as usize;
    let h = inner.height as usize;
    if w == 0 || h == 0 {
        return;
    }

    let bar_h = h.saturating_sub(1).max(1);
    let bands = &state.spectrum.bands;
    let peaks = &state.spectrum.peaks;
    let n_bands = bands.len().max(1);

    let mut cols: Vec<usize> = Vec::with_capacity(w);
    for col in 0..w {
        let b = (col * n_bands) / w;
        cols.push(b.min(n_bands - 1));
    }

    let mut lines: Vec<Line> = Vec::with_capacity(h);

    for r in 0..bar_h {
        let mut spans: Vec<Span> = Vec::with_capacity(w);
        let row_from_bot = bar_h.saturating_sub(1 + r);
        let step_bot = row_from_bot * 8;
        let step_top = step_bot + 8;

        let row_style = if r == 0 {
            Style::default().fg(palette.accent_danger).add_modifier(Modifier::BOLD)
        } else if r < bar_h / 4 {
            Style::default().fg(palette.accent_danger)
        } else if r < bar_h / 2 {
            Style::default().fg(palette.accent_warning)
        } else if r < (bar_h * 3) / 4 {
            Style::default().fg(palette.accent_secondary)
        } else {
            Style::default().fg(palette.accent_primary)
        };

        if state.spectrum.rms_db <= -85.0 && r == bar_h / 2 {
            let idle_text = "- - Waiting for voice signal (-96 dBFS baseline) - -";
            let text_len = idle_text.chars().count();
            if w > text_len {
                let start = (w - text_len) / 2;
                let mut idle_spans = Vec::new();
                if start > 0 {
                    idle_spans.push(Span::raw(" ".repeat(start)));
                }
                idle_spans.push(Span::styled(idle_text, Style::default().fg(palette.text_muted)));
                let rem = w.saturating_sub(start + text_len);
                if rem > 0 {
                    idle_spans.push(Span::raw(" ".repeat(rem)));
                }
                lines.push(Line::from(idle_spans));
                continue;
            }
        }

        for &b in &cols {
            let val = bands[b] as usize;
            let peak_val = peaks[b] as usize;
            let has_peak = peak_val >= step_bot && peak_val < step_top && peak_val > 0;

            if val >= step_top {
                spans.push(Span::styled("█", row_style));
            } else if val > step_bot {
                let sub = val - step_bot;
                let ch = match sub {
                    1 => " ",
                    2 => "▂",
                    3 => "▃",
                    4 => "▄",
                    5 => "▅",
                    6 => "▆",
                    _ => "▇",
                };
                spans.push(Span::styled(ch, row_style));
            } else if has_peak {
                spans.push(Span::styled(
                    "▔",
                    Style::default()
                        .fg(palette.text)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(" "));
            }
        }
        lines.push(Line::from(spans));
    }

    // Bottom scale axis
    let axis_str = if w >= 45 {
        "  40Hz    120Hz     400Hz     1.2kHz    4kHz     12kHz"
    } else {
        "  60Hz      500Hz      2kHz      8kHz"
    };
    let fitted_axis = fit_string(axis_str, w);
    lines.push(Line::from(vec![Span::styled(
        fitted_axis,
        Style::default().fg(palette.text_muted),
    )]));

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_waterfall_heatmap(frame: &mut Frame, inner: Rect, state: &AppState, palette: &ColorPalette) {
    let w = inner.width as usize;
    let h = inner.height as usize;
    if w == 0 || h == 0 {
        return;
    }

    let history = &state.spectrum.waterfall;
    let mut lines: Vec<Line> = Vec::with_capacity(h);

    for y in 0..h {
        let slice_idx = if y < h && history.len() >= h - y {
            history.len() - (h - y)
        } else if !history.is_empty() {
            0
        } else {
            9999
        };

        let mut spans: Vec<Span> = Vec::with_capacity(w);
        if slice_idx < history.len() {
            let slice = &history[slice_idx];
            let n_bands = slice.len().max(1);
            for col in 0..w {
                let b = (col * n_bands) / w;
                let val = if b < slice.len() { slice[b] } else { 0.0 };

                if val < 0.12 {
                    spans.push(Span::raw(" "));
                } else if val < 0.28 {
                    spans.push(Span::styled("░", Style::default().fg(palette.accent_purple)));
                } else if val < 0.48 {
                    spans.push(Span::styled("▒", Style::default().fg(palette.accent_primary)));
                } else if val < 0.68 {
                    spans.push(Span::styled("▓", Style::default().fg(palette.accent_secondary)));
                } else if val < 0.85 {
                    spans.push(Span::styled("█", Style::default().fg(palette.accent_warning)));
                } else {
                    spans.push(Span::styled(
                        "█",
                        Style::default().fg(palette.accent_danger).add_modifier(Modifier::BOLD),
                    ));
                }
            }
        } else {
            spans.push(Span::raw(" ".repeat(w)));
        }
        lines.push(Line::from(spans));
    }

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_fullscreen_spectrograph(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let title_rta = " Fullscreen RTA Spectrum (m/Esc: exit graph) ".to_string();
    let block_rta = Block::default()
        .borders(Borders::ALL)
        .title(title_rta)
        .border_style(Style::default().fg(palette.border_active));
    let inner_rta = block_rta.inner(chunks[0]);
    frame.render_widget(block_rta, chunks[0]);
    render_rta_bars(frame, inner_rta, state, palette);

    let title_wf = " Live Spectrogram Waterfall (Time / Frequency Heatmap) ".to_string();
    let block_wf = Block::default()
        .borders(Borders::ALL)
        .title(title_wf)
        .border_style(Style::default().fg(palette.accent_secondary));
    let inner_wf = block_wf.inner(chunks[1]);
    frame.render_widget(block_wf, chunks[1]);
    render_waterfall_heatmap(frame, inner_wf, state, palette);
}

fn render_live_controls(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let title = if area.width >= 60 {
        " Live Controls (Shift+j/k or Tab to cycle, h/l to adjust) "
    } else {
        " Live Controls (Shift+j/k, h/l) "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(palette.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    // Responsive gauge width calculation: safe 8 or 10 columns to prevent border collisions
    let gauge_len = if inner.width >= 50 { 10 } else { 8 };

    // 0: DPDFNet Suppression Strength (0..100 dB)
    let is_sel_0 = state.selected_control == 0;
    let mark_0 = if is_sel_0 { "> " } else { "  " };
    let norm_atten = ((state.dpdfnet_attenuation / 100.0) * gauge_len as f32)
        .clamp(0.0, gauge_len as f32)
        .round() as usize;
    let line_0_style = if is_sel_0 {
        Style::default()
            .bg(palette.select_bg)
            .fg(palette.select_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text)
    };
    let mut spans_0 = vec![
        Span::styled(mark_0, line_0_style),
        Span::styled(
            format!(
                "[1] DPDFNet Suppression : {:>3.0} dB ",
                state.dpdfnet_attenuation
            ),
            line_0_style,
        ),
    ];
    spans_0.extend(render_gauge_bar(norm_atten, gauge_len, is_sel_0, palette));
    lines.push(Line::from(spans_0));

    // 1: Output Gain (-24..+12 dB) - Universal across all models
    let is_sel_1 = state.selected_control == 1;
    let mark_1 = if is_sel_1 { "> " } else { "  " };
    let norm_gain = (((state.output_gain_db + 24.0) / 36.0) * gauge_len as f32)
        .clamp(0.0, gauge_len as f32)
        .round() as usize;
    let line_1_style = if is_sel_1 {
        Style::default()
            .bg(palette.select_bg)
            .fg(palette.select_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text)
    };
    let mut spans_1 = vec![
        Span::styled(mark_1, line_1_style),
        Span::styled(
            format!(
                "[2] Output Gain         : {:>+4.1} dB ",
                state.output_gain_db
            ),
            line_1_style,
        ),
    ];
    spans_1.extend(render_gauge_bar(norm_gain, gauge_len, is_sel_1, palette));
    lines.push(Line::from(spans_1));

    // 2: Effect Wet Mix (0..100%) OR Direct Passthrough on Natural Clean
    let is_sel_2 = state.selected_control == 2;
    let mark_2 = if is_sel_2 { "> " } else { "  " };
    let line_2_style = if is_sel_2 {
        Style::default()
            .bg(palette.select_bg)
            .fg(palette.select_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text)
    };

    let is_active_clean = state
        .active_preset_name
        .eq_ignore_ascii_case("Natural Clean");

    if is_active_clean {
        lines.push(Line::from(vec![
            Span::styled(mark_2, line_2_style),
            Span::styled("[3] Effect Wet Mix      : 0% [Direct Clean]", line_2_style),
        ]));
    } else {
        let norm_mix = (((state.effect_mix_pct as f32) / 100.0) * gauge_len as f32)
            .clamp(0.0, gauge_len as f32)
            .round() as usize;
        let mut spans_2 = vec![
            Span::styled(mark_2, line_2_style),
            Span::styled(
                format!(
                    "[3] Effect Wet Mix      : {:>3}%     ",
                    state.effect_mix_pct
                ),
                line_2_style,
            ),
        ];
        spans_2.extend(render_gauge_bar(norm_mix, gauge_len, is_sel_2, palette));
        lines.push(Line::from(spans_2));
    }

    // 3: Sidetone (On/Off)
    let is_sel_3 = state.selected_control == 3;
    let mark_3 = if is_sel_3 { "> " } else { "  " };
    let sidetone_state = if state.sidetone_enabled {
        if state.force_speaker_sidetone {
            Span::styled(
                "[ON] (FORCED SPEAKERS)",
                Style::default()
                    .fg(palette.accent_warning)
                    .add_modifier(Modifier::BOLD),
            )
        } else if state.sidetone_guarded {
            Span::styled(
                "[ON] (Muted: Laptop Speakers Guard)",
                Style::default()
                    .fg(palette.accent_warning)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                "[ON]",
                Style::default()
                    .fg(palette.accent_secondary)
                    .add_modifier(Modifier::BOLD),
            )
        }
    } else {
        Span::styled("[OFF]", Style::default().fg(palette.text_muted))
    };
    let line_3_style = if is_sel_3 {
        Style::default()
            .bg(palette.select_bg)
            .fg(palette.select_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text)
    };
    lines.push(Line::from(vec![
        Span::styled(mark_3, line_3_style),
        Span::styled("[4] Hear Voice (Monitor): ", line_3_style),
        sidetone_state,
    ]));

    // 4: VU Meter (On/Off)
    let is_sel_4 = state.selected_control == 4;
    let mark_4 = if is_sel_4 { "> " } else { "  " };
    let meter_state = if state.meter_enabled {
        Span::styled(
            "[ON]",
            Style::default()
                .fg(palette.accent_secondary)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("[OFF]", Style::default().fg(palette.text_muted))
    };
    let line_4_style = if is_sel_4 {
        Style::default()
            .bg(palette.select_bg)
            .fg(palette.select_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text)
    };
    lines.push(Line::from(vec![
        Span::styled(mark_4, line_4_style),
        Span::styled("[5] Level VU Meter      : ", line_4_style),
        meter_state,
    ]));

    // 5: Spectrograph Mode (RTA / Waterfall)
    let is_sel_5 = state.selected_control == 5;
    let mark_5 = if is_sel_5 { "> " } else { "  " };
    let spec_mode_str = match state.spectrograph_mode {
        crate::ui::app::SpectrographMode::RtaBars => "[RTA SPECTRUM]",
        crate::ui::app::SpectrographMode::Waterfall => "[WATERFALL HEATMAP]",
    };
    let line_5_style = if is_sel_5 {
        Style::default()
            .bg(palette.select_bg)
            .fg(palette.select_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text)
    };
    lines.push(Line::from(vec![
        Span::styled(mark_5, line_5_style),
        Span::styled("[6] Spectrograph Mode   : ", line_5_style),
        Span::styled(
            spec_mode_str,
            if is_sel_5 {
                Style::default()
                    .fg(palette.accent_warning)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.accent_primary)
            },
        ),
    ]));

    if inner.height >= 8 {
        let (hint_text, hint_style) = if state.unsaved_changes {
            if inner.width >= 56 {
                (
                    " [r] Reset model │ [Shift+R] Reset all │ [w] Save tweaks (Unsaved)",
                    Style::default().fg(palette.accent_warning).add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    " [r] Reset │ [Shift+R] All │ [w] Save (Unsaved)",
                    Style::default().fg(palette.accent_warning).add_modifier(Modifier::BOLD),
                )
            }
        } else {
            if inner.width >= 56 {
                (
                    " [r] Reset model │ [Shift+R] Reset all │ [w] Save tweaks",
                    Style::default().fg(palette.text_muted),
                )
            } else {
                (
                    " [r] Reset │ [Shift+R] All │ [w] Save tweaks",
                    Style::default().fg(palette.text_muted),
                )
            }
        };
        lines.push(Line::from(vec![Span::styled(hint_text, hint_style)]));
    }

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_bottom_bar(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let key_style = Style::default()
        .fg(palette.text)
        .add_modifier(Modifier::BOLD);
    let action_style = Style::default().fg(palette.text_muted);

    let spans = if state.search_mode {
        vec![
            Span::styled(
                "Search: ",
                Style::default()
                    .fg(palette.accent_warning)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}_", state.search_query),
                Style::default().fg(palette.text),
            ),
            Span::styled(" (Enter done, Esc clear)", action_style),
        ]
    } else if state.spectrograph_fullscreen {
        vec![
            Span::styled("m", key_style),
            Span::styled(": exit graph │ ", action_style),
            Span::styled("Esc", key_style),
            Span::styled(": return", action_style),
        ]
    } else {
        let (save_k_style, save_label, save_a_style) = if state.unsaved_changes {
            (
                Style::default().fg(palette.accent_warning).add_modifier(Modifier::BOLD),
                ": save [unsaved]",
                Style::default().fg(palette.accent_warning).add_modifier(Modifier::BOLD),
            )
        } else {
            (key_style, ": save", action_style)
        };

        let sep = if area.width < 105 { "│ " } else { " │ " };
        let mut items = vec![
            Span::styled("j/k", key_style),
            Span::styled(format!(": move{}", sep), action_style),
            Span::styled("Enter", key_style),
            Span::styled(format!(": apply{}", sep), action_style),
            Span::styled("Space", key_style),
            Span::styled(format!(": details{}", sep), action_style),
            Span::styled("p", key_style),
            Span::styled(format!(": audition{}", sep), action_style),
            Span::styled("m", key_style),
            Span::styled(format!(": graph{}", sep), action_style),
            Span::styled("c", key_style),
            Span::styled(format!(": clean{}", sep), action_style),
            Span::styled("r", key_style),
            Span::styled(format!(": reset{}", sep), action_style),
        ];
        if area.width >= 105 {
            items.push(Span::styled("F2", Style::default().fg(palette.accent_purple).add_modifier(Modifier::BOLD)));
            items.push(Span::styled(format!(": theme{}", sep), action_style));
        }
        items.push(Span::styled("w", save_k_style));
        items.push(Span::styled(save_label, save_a_style));
        items
    };

    let p = Paragraph::new(Line::from(spans));
    frame.render_widget(p, area);
}

fn render_help_dialog(frame: &mut Frame, area: Rect, state: &AppState, palette: &ColorPalette) {
    let popup_w = 72.min(area.width.saturating_sub(4));
    let popup_h = 24.min(area.height.saturating_sub(2));

    let popup_x = (area.width.saturating_sub(popup_w)) / 2;
    let popup_y = (area.height.saturating_sub(popup_h)) / 2;

    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
    frame.render_widget(Clear, popup_rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" MicStudio Help & DSP Stack ")
        .border_style(Style::default().fg(palette.border_active));
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    let inner_w = inner.width as usize;
    let key_style = Style::default()
        .fg(palette.accent_warning)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(palette.text);

    let stages_line = state.dynamic_stages.join(" -> ");

    let lines = vec![
        Line::from(vec![
            Span::styled("j / k or Down / Up ", key_style),
            Span::styled(": Navigate presets list", desc_style),
        ]),
        Line::from(vec![
            Span::styled("Enter          ", key_style),
            Span::styled(": Apply voice preset live to microphone", desc_style),
        ]),
        Line::from(vec![
            Span::styled("Space          ", key_style),
            Span::styled(": View full model DSP specifications & signal chain", desc_style),
        ]),
        Line::from(vec![
            Span::styled("p              ", key_style),
            Span::styled(": Audition: record 5s & replay with normalized boost", desc_style),
        ]),
        Line::from(vec![
            Span::styled("c              ", key_style),
            Span::styled(": Instantly restore clean studio broadcast microphone", desc_style),
        ]),
        Line::from(vec![
            Span::styled("m              ", key_style),
            Span::styled(": Toggle fullscreen live spectrograph / spectrogram", desc_style),
        ]),
        Line::from(vec![
            Span::styled("g              ", key_style),
            Span::styled(": Switch spectrograph mode (RTA Spectrum / Waterfall Heatmap)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("Shift+j / Shift+k", key_style),
            Span::styled(": Cycle active live control row", desc_style),
        ]),
        Line::from(vec![
            Span::styled("h / l or Left/Right", key_style),
            Span::styled(": Adjust DPDFNet (5dB steps), Output Gain, or FX mix", desc_style),
        ]),
        Line::from(vec![
            Span::styled("w              ", key_style),
            Span::styled(": Save live tweaks & theme permanently to disk", desc_style),
        ]),
        Line::from(vec![
            Span::styled("F2             ", key_style),
            Span::styled(": Switch color theme (NvChad, Catppuccin, TokyoNight, Nord)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("r / Shift+R    ", key_style),
            Span::styled(": Reset model / Reset all to factory defaults", desc_style),
        ]),
        Line::from(vec![
            Span::styled("/              ", key_style),
            Span::styled(": Filter presets with fuzzy real-time search", desc_style),
        ]),
        Line::from(vec![
            Span::styled("v / t          ", key_style),
            Span::styled(": Toggle Hear Voice (monitor) / Toggle VU meter", desc_style),
        ]),
        Line::from(vec![
            Span::styled("? / Esc        ", key_style),
            Span::styled(": Close this help dialog", desc_style),
        ]),
        Line::from(vec![
            Span::styled("q              ", key_style),
            Span::styled(": Quit MicStudio", desc_style),
        ]),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "Active 40-mic.conf Stages: ",
            Style::default()
                .fg(palette.accent_primary)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            fit_string(&stages_line, inner_w),
            Style::default().fg(palette.text_muted),
        )]),
    ];

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_gauge_bar<'a>(
    norm: usize,
    max: usize,
    is_selected: bool,
    palette: &ColorPalette,
) -> Vec<Span<'a>> {
    let filled = norm.min(max);
    let empty = max.saturating_sub(filled);
    let bg = if is_selected {
        palette.select_bg
    } else {
        Color::Reset
    };
    let fill_style = Style::default()
        .fg(palette.gauge_fill)
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let empty_style = Style::default().fg(palette.gauge_empty).bg(bg);
    let bracket_style = Style::default().fg(palette.border).bg(bg);

    vec![
        Span::styled("[", bracket_style),
        Span::styled("━".repeat(filled), fill_style),
        Span::styled("─".repeat(empty), empty_style),
        Span::styled("]", bracket_style),
    ]
}

pub fn render_confirm_dialog(
    frame: &mut Frame,
    area: Rect,
    action: &crate::ui::app::ConfirmAction,
    palette: &ColorPalette,
) {
    let popup_w = 64.min(area.width.saturating_sub(4));
    let popup_h = 9.min(area.height.saturating_sub(2));

    let popup_x = (area.width.saturating_sub(popup_w)) / 2;
    let popup_y = (area.height.saturating_sub(popup_h)) / 2;

    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
    frame.render_widget(Clear, popup_rect);

    let (title, line1, line2, line4) = match action {
        crate::ui::app::ConfirmAction::ResetSelected(name) => (
            " Confirm Reset Preset ",
            format!("Reset '{}' to factory baseline?", name),
            "(Restores DPDFNet 45dB, Output Gain +4.5dB, Quiet Boost ON +4dB)".to_string(),
            "[y / Enter] Confirm Reset   [n / Esc] Cancel".to_string(),
        ),
        crate::ui::app::ConfirmAction::ResetAll => (
            " Confirm Factory Reset All ",
            "Reset ALL 27 presets & clean mic to factory defaults?".to_string(),
            "This will overwrite all customized tweaks permanently.".to_string(),
            "[y / Enter] Confirm Factory Reset   [n / Esc] Cancel".to_string(),
        ),
        crate::ui::app::ConfirmAction::QuitUnsaved => (
            " Unsaved Changes ",
            "You have modified controls or voice models in this session.".to_string(),
            "Do you want to save changes permanently to disk before exiting?".to_string(),
            "[w / y] Save & Exit   [q / n] Discard & Exit   [Esc] Cancel".to_string(),
        ),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(
            Style::default()
                .fg(palette.accent_warning)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        line1,
        Style::default().fg(palette.text).add_modifier(Modifier::BOLD),
    )]));
    if !line2.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            line2,
            Style::default().fg(palette.text_muted),
        )]));
    }
    lines.push(Line::from(vec![]));
    lines.push(Line::from(vec![Span::styled(
        line4,
        Style::default()
            .fg(palette.accent_primary)
            .add_modifier(Modifier::BOLD),
    )]));

    let p = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(p, inner);
}
