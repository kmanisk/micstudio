//! Pure Ratatui rendering function for MicStudio.

use crate::ui::app::AppState;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
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

    // Enforce 80x24 minimum resolution without panicking
    if area.width < 80 || area.height < 24 {
        let msg = format!(
            "terminal too small (need 80x24, current: {}x{})",
            area.width, area.height
        );
        let p = Paragraph::new(msg).alignment(Alignment::Center).style(
            Style::default()
                .fg(Color::Yellow)
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

    render_top_bar(frame, chunks[0], state);
    render_middle(frame, chunks[1], state);
    render_bottom_bar(frame, chunks[2], state);

    if state.show_help {
        render_help_dialog(frame, area, state);
    }
}

fn render_top_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let is_clean = state
        .active_preset_name
        .eq_ignore_ascii_case("Natural Clean");

    let engine_state_spans = if is_clean {
        vec![
            Span::raw("State: "),
            Span::styled(
                "Clean",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]
    } else {
        vec![
            Span::raw("State: "),
            Span::styled(
                format!("FX ({})", state.active_preset_name),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]
    };

    let meter_spans = if !state.meter_enabled {
        vec![Span::styled(
            "Meter: off",
            Style::default().fg(Color::DarkGray),
        )]
    } else {
        let norm = ((state.input_rms_db + 60.0) / 6.0).clamp(0.0, 10.0).round() as usize;
        let filled = "█".repeat(norm);
        let empty = "░".repeat(10 - norm);
        let bar = format!("[{}{}]", filled, empty);
        let peak_db = if state.spectrum.peak_level > 1e-5 {
            20.0 * state.spectrum.peak_level.log10()
        } else {
            -96.0
        };
        let is_hw_clip = state.spectrum.hw_adc_clipped;
        let is_digital_limiting = state.spectrum.digital_limiting || (!is_hw_clip && peak_db >= -2.0);

        let mut spans = vec![
            Span::styled("Meter: ", Style::default().fg(Color::White)),
            Span::styled(
                bar,
                Style::default().fg(if is_hw_clip {
                    Color::Red
                } else if norm > 8 {
                    Color::Red
                } else if norm > 5 {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            ),
            Span::raw(format!(" {:+5.1} dBFS", state.input_rms_db)),
            Span::styled(
                format!(" (Pk: {:+5.1})", peak_db),
                Style::default().fg(Color::DarkGray),
            ),
        ];
        if is_hw_clip {
            spans.push(Span::styled(
                " [HW ADC CLIP: Lower physical mic volume]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        } else if is_digital_limiting {
            spans.push(Span::styled(
                " [DIGITAL LIMITING]",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
        spans
    };

    let mut spans = vec![
        Span::styled(
            " MICSTUDIO ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
    ];
    spans.extend(engine_state_spans);
    spans.push(Span::raw(" │ "));
    spans.extend(meter_spans);

    let line = Line::from(spans);
    let p = Paragraph::new(line);
    frame.render_widget(p, area);
}

fn render_middle(frame: &mut Frame, area: Rect, state: &AppState) {
    if state.spectrograph_fullscreen {
        render_fullscreen_spectrograph(frame, area, state);
        return;
    }

    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(50)])
        .split(area);

    render_left_pane(frame, horizontal_chunks[0], state);
    render_right_pane(frame, horizontal_chunks[1], state);
}

enum VisualRow {
    CategoryHeader(String),
    PresetItem {
        filtered_index: usize,
        preset_idx: usize,
    },
}

fn render_left_pane(frame: &mut Frame, area: Rect, state: &AppState) {
    let title = if state.search_mode {
        format!(" Presets [/{}_] ", state.search_query)
    } else if !state.search_query.is_empty() {
        format!(" Presets [/{}] ", state.search_query)
    } else {
        " Presets ".to_string()
    };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let filtered = state.filtered_indices();
    if filtered.is_empty() {
        let p = Paragraph::new(" No matching presets").style(Style::default().fg(Color::DarkGray));
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
    // Calculate scrolling window
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
                        .fg(Color::DarkGray)
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

                // Available width for preset name: inner_w - (1+1 + 3+1 + 3+1) = inner_w - 10
                let name_max = inner_w.saturating_sub(10);
                let fitted_name = fit_string(&p.name, name_max);

                let row_style = if is_cursor {
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let verified_style = if p.verified {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let active_style = if is_active {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
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

fn render_right_pane(frame: &mut Frame, area: Rect, state: &AppState) {
    if state.sidetone_enabled {
        let detail_height = if area.height <= 22 { 7 } else { 8 };
        let controls_height = if area.height <= 22 { 9 } else { 10 };
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(detail_height),   // Preset details
                Constraint::Min(5), // Live Spectrograph module (only when using 'v')
                Constraint::Length(controls_height), // Live Controls (7 items)
            ])
            .split(area);

        render_preset_details(frame, vertical_chunks[0], state);
        render_spectrograph_module(frame, vertical_chunks[1], state);
        render_live_controls(frame, vertical_chunks[2], state);
    } else {
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),     // Preset details & Signal chain overview
                Constraint::Length(10), // Live Controls (7 items)
            ])
            .split(area);

        render_preset_details(frame, vertical_chunks[0], state);
        render_live_controls(frame, vertical_chunks[1], state);
    }
}

fn render_preset_details(frame: &mut Frame, area: Rect, state: &AppState) {
    let selected = state.selected_preset();
    let is_selected_active = selected
        .name
        .eq_ignore_ascii_case(&state.active_preset_name);
    let title = format!(" Selected Preset: {} ", fit_string(&selected.name, 28));

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let mut lines = Vec::new();
    let inner_w = inner.width as usize;

    // Line 1: Identity badges & routing status
    let mut header_spans = vec![Span::styled(
        format!("[{}] ", selected.category),
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )];
    if selected.verified {
        header_spans.push(Span::styled(
            "[VERIFIED] ",
            Style::default().fg(Color::Green),
        ));
    }
    if is_selected_active {
        header_spans.push(Span::styled(
            "[CURRENTLY ACTIVE] ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
        let src_node = if selected.name.eq_ignore_ascii_case("Natural Clean") {
            "ghelper-audio (Clean Broadcast)"
        } else {
            "ghelper-audio (Live FX Hot-Reload)"
        };
        header_spans.push(Span::styled(
            format!("Node: {}", src_node),
            Style::default().fg(Color::Cyan),
        ));
    } else {
        header_spans.push(Span::styled(
            "[PREVIEW ONLY] ",
            Style::default().fg(Color::Yellow),
        ));
        header_spans.push(Span::styled(
            "(Press Enter to Apply)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines.push(Line::from(header_spans));

    // Line 2: Description
    let desc_fit = fit_string(&selected.description, inner_w.saturating_sub(13));
    lines.push(Line::from(vec![
        Span::styled(
            "Description: ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc_fit, Style::default().fg(Color::Gray)),
    ]));

    // Line 3: Stage Pipeline graph
    let stages = selected.stage_pipeline_summary();
    let stages_joined = stages.join(" ➔ ");
    lines.push(Line::from(vec![
        Span::styled(
            "Pipeline   : ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            fit_string(&stages_joined, inner_w.saturating_sub(13)),
            Style::default().fg(Color::LightCyan),
        ),
    ]));

    // Line 4: DSP Parameters breakdown
    let nonzero = selected.nonzero_params();
    if nonzero.is_empty() {
        let clean_param_text = if state.quiet_boost_enabled {
            format!(
                "Direct bypass (DPDFNet {:.0}dB) │ Quiet Voice Boost {:+.0}dB [ON] (Lowered threshold & leveling)",
                state.dpdfnet_attenuation, state.quiet_boost_db
            )
        } else {
            format!(
                "Direct bypass — pristine clean speech (DPDFNet {:.0}dB, Quiet Boost: OFF)",
                state.dpdfnet_attenuation
            )
        };
        lines.push(Line::from(vec![
            Span::styled(
                "Parameters : ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(clean_param_text, Style::default().fg(Color::Green)),
        ]));
    } else {
        let mut param_strs = Vec::new();
        for (param, val) in nonzero {
            param_strs.push(format!("{}: {}", param, val));
        }
        lines.push(Line::from(vec![
            Span::styled(
                "Parameters : ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                fit_string(&param_strs.join(" │ "), inner_w.saturating_sub(13)),
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }

    // Line 5: Tech limitations note
    if inner.height >= 5 {
        if let Some(note) = selected.limitations_note() {
            lines.push(Line::from(vec![
                Span::styled(
                    "Tech Note  : ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    fit_string(&note, inner_w.saturating_sub(13)),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    // Line 6+: Sidetone prompt if sidetone is off and space permits
    if !state.sidetone_enabled && inner.height >= 7 {
        lines.push(Line::from(vec![
            Span::styled("Monitoring : ", Style::default().fg(Color::White)),
            Span::styled(
                "Inactive (Press 'v' to monitor mic & view live real-time spectrograph)",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_spectrograph_module(frame: &mut Frame, area: Rect, state: &AppState) {
    let mode_str = match state.spectrograph_mode {
        crate::ui::app::SpectrographMode::RtaBars => "RTA Spectrum",
        crate::ui::app::SpectrographMode::Waterfall => "Waterfall Heatmap",
    };
    let title = format!(
        " Live Spectrograph [{}] (m: mode, s: zoom) │ Peak: {:>+5.1} dB │ RMS: {:>+5.1} dB ",
        mode_str,
        if state.spectrum.peak_level > 1e-5 {
            20.0 * state.spectrum.peak_level.log10()
        } else {
            -96.0
        },
        state.spectrum.rms_db
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 10 || inner.height < 2 {
        return;
    }

    match state.spectrograph_mode {
        crate::ui::app::SpectrographMode::RtaBars => {
            render_rta_bars(frame, inner, state);
        }
        crate::ui::app::SpectrographMode::Waterfall => {
            render_waterfall_heatmap(frame, inner, state);
        }
    }
}

fn render_rta_bars(frame: &mut Frame, inner: Rect, state: &AppState) {
    let w = inner.width as usize;
    let h = inner.height as usize;
    if w == 0 || h == 0 {
        return;
    }

    let bar_h = h.saturating_sub(1).max(1);
    let bands = &state.spectrum.bands;
    let peaks = &state.spectrum.peaks;
    let n_bands = bands.len().max(1);

    // Map each column to corresponding frequency band
    let mut cols: Vec<usize> = Vec::with_capacity(w);
    for col in 0..w {
        let b = (col * n_bands) / w;
        cols.push(b.min(n_bands - 1));
    }

    let mut lines: Vec<Line> = Vec::with_capacity(h);

    // Render bar rows from top (r=0, 0 dBFS) to bottom (r=bar_h-1, -80 dBFS)
    for r in 0..bar_h {
        let mut spans: Vec<Span> = Vec::with_capacity(w);
        let row_from_bot = bar_h.saturating_sub(1 + r);
        let step_bot = row_from_bot * 8;
        let step_top = step_bot + 8;

        // Dynamic spectral color gradient: Red (clipping/hot) -> Yellow -> Green -> Cyan (noise floor)
        let row_style = if r == 0 {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else if r < bar_h / 4 {
            Style::default().fg(Color::Red)
        } else if r < bar_h / 2 {
            Style::default().fg(Color::Yellow)
        } else if r < (bar_h * 3) / 4 {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Cyan)
        };

        for col in 0..w {
            let b = cols[col];
            let level = if b < bands.len() { bands[b] } else { 0.0 };
            let peak = if b < peaks.len() { peaks[b] } else { 0.0 };

            let total_steps = (level * (bar_h * 8) as f32).round() as usize;
            let peak_row = ((1.0 - peak) * (bar_h.saturating_sub(1)) as f32).round() as usize;

            if total_steps >= step_top {
                spans.push(Span::styled("█", row_style));
            } else if total_steps > step_bot {
                let frac = total_steps - step_bot;
                let ch = match frac {
                    1 => ' ',
                    2 => '▂',
                    3 => '▃',
                    4 => '▄',
                    5 => '▅',
                    6 => '▆',
                    7 => '▇',
                    _ => '█',
                };
                spans.push(Span::styled(ch.to_string(), row_style));
            } else if r == peak_row && peak > 0.05 {
                spans.push(Span::styled(
                    "─",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(" "));
            }
        }
        lines.push(Line::from(spans));
    }

    // Bottom line: logarithmic frequency axis
    if h > 1 {
        let axis_str = build_frequency_axis(w);
        lines.push(Line::from(vec![Span::styled(
            axis_str,
            Style::default().fg(Color::DarkGray),
        )]));
    }

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_waterfall_heatmap(frame: &mut Frame, inner: Rect, state: &AppState) {
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
                    spans.push(Span::styled("·", Style::default().fg(Color::DarkGray)));
                } else if val < 0.28 {
                    spans.push(Span::styled("░", Style::default().fg(Color::Blue)));
                } else if val < 0.48 {
                    spans.push(Span::styled("▒", Style::default().fg(Color::Cyan)));
                } else if val < 0.68 {
                    spans.push(Span::styled("▓", Style::default().fg(Color::Green)));
                } else if val < 0.85 {
                    spans.push(Span::styled("█", Style::default().fg(Color::Yellow)));
                } else {
                    spans.push(Span::styled(
                        "█",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
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

fn render_fullscreen_spectrograph(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(58), // Top 58%: High-res RTA Spectrum
            Constraint::Percentage(42), // Bottom 42%: Waterfall Spectrogram
        ])
        .split(area);

    let title_rta = format!(
        " Fullscreen RTA Spectrum Analyzer (50 Hz – 16 kHz) │ Peak: {:>+5.1} dBFS │ RMS: {:>+5.1} dBFS ",
        if state.spectrum.peak_level > 1e-5 {
            20.0 * state.spectrum.peak_level.log10()
        } else {
            -96.0
        },
        state.spectrum.rms_db
    );
    let block_rta = Block::default()
        .borders(Borders::ALL)
        .title(title_rta)
        .border_style(Style::default().fg(Color::Cyan));
    let inner_rta = block_rta.inner(chunks[0]);
    frame.render_widget(block_rta, chunks[0]);
    render_rta_bars(frame, inner_rta, state);

    let block_wf = Block::default()
        .borders(Borders::ALL)
        .title(" Spectrogram Waterfall (Time History) │ 's': return to normal view │ 'm': toggle mode │ Esc: exit ")
        .border_style(Style::default().fg(Color::Green));
    let inner_wf = block_wf.inner(chunks[1]);
    frame.render_widget(block_wf, chunks[1]);
    render_waterfall_heatmap(frame, inner_wf, state);
}

fn build_frequency_axis(width: usize) -> String {
    if width < 15 {
        return " 50Hz … 16kHz".to_string();
    }
    let markers: &[(&str, f32)] = &[
        ("50", 0.02),
        ("100", 0.12),
        ("250", 0.25),
        ("500", 0.38),
        ("1k", 0.51),
        ("2k", 0.64),
        ("4k", 0.76),
        ("8k", 0.88),
        ("16k", 0.98),
    ];

    let mut buf = vec![' '; width];
    for &(label, pos_ratio) in markers {
        let center = (pos_ratio * width as f32).round() as usize;
        let start = center.saturating_sub(label.len() / 2);
        if start + label.len() <= width {
            let mut overlaps = false;
            for i in 0..label.len() {
                if buf[start + i] != ' ' {
                    overlaps = true;
                    break;
                }
            }
            if !overlaps {
                for (i, c) in label.chars().enumerate() {
                    buf[start + i] = c;
                }
            }
        }
    }
    buf.into_iter().collect()
}

fn render_live_controls(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Live Controls (Shift+j/k or Tab to cycle, h/l to adjust) ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    // 0: DPDFNet Suppression Strength (0..100 dB)
    let is_sel_0 = state.selected_control == 0;
    let mark_0 = if is_sel_0 { "> " } else { "  " };
    let norm_atten = (state.dpdfnet_attenuation / 10.0).clamp(0.0, 10.0).round() as usize;
    let bar_atten = format!(
        "[{}{}]",
        "█".repeat(norm_atten),
        "░".repeat(10 - norm_atten)
    );
    let line_0_style = if is_sel_0 {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    lines.push(Line::from(vec![
        Span::styled(mark_0, line_0_style),
        Span::styled(
            format!(
                "[1] DPDFNet Suppression : {:>3.0} dB  {} ",
                state.dpdfnet_attenuation, bar_atten
            ),
            line_0_style,
        ),
    ]));

    // 1: Quiet Voice Boost (0..10 dB) - Universal across all models
    let is_sel_1 = state.selected_control == 1;
    let mark_1 = if is_sel_1 { "> " } else { "  " };
    let norm_boost = (state.quiet_boost_db).clamp(0.0, 10.0).round() as usize;
    let bar_boost = format!(
        "[{}{}]",
        "█".repeat(norm_boost),
        "░".repeat(10 - norm_boost)
    );
    let line_1_style = if is_sel_1 {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let boost_state = if state.quiet_boost_enabled {
        Span::styled(
            format!(" [{:+.0} dB] [ON] ", state.quiet_boost_db),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" [OFF] +0 dB ", Style::default().fg(Color::DarkGray))
    };
    lines.push(Line::from(vec![
        Span::styled(mark_1, line_1_style),
        Span::styled("[2] Quiet Voice Boost   : ", line_1_style),
        boost_state,
        Span::styled(format!("{} ", bar_boost), line_1_style),
    ]));

    // 2: Clean Output Gain (-24..+12 dB) OR Model Voice Boost (-12..+12 dB, default +2.0 dB)
    let is_sel_2 = state.selected_control == 2;
    let mark_2 = if is_sel_2 { "> " } else { "  " };
    let line_2_style = if is_sel_2 {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let is_active_clean = state
        .active_preset_name
        .eq_ignore_ascii_case("Natural Clean");

    if is_active_clean {
        let norm_gain = ((state.output_gain_db + 24.0) / 3.6)
            .clamp(0.0, 10.0)
            .round() as usize;
        let bar_gain = format!("[{}{}]", "█".repeat(norm_gain), "░".repeat(10 - norm_gain));
        lines.push(Line::from(vec![
            Span::styled(mark_2, line_2_style),
            Span::styled(
                format!(
                    "[3] Clean Output Gain   : {:>+4.1} dB {} ",
                    state.output_gain_db, bar_gain
                ),
                line_2_style,
            ),
        ]));
    } else {
        let active_model_gain = state
            .presets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(&state.active_preset_name))
            .map(|p| p.model_gain_db)
            .unwrap_or(2.0);
        let norm_gain = ((active_model_gain + 12.0) / 2.4).clamp(0.0, 10.0).round() as usize;
        let bar_gain = format!("[{}{}]", "█".repeat(norm_gain), "░".repeat(10 - norm_gain));
        lines.push(Line::from(vec![
            Span::styled(mark_2, line_2_style),
            Span::styled(
                format!(
                    "[3] Model Voice Boost   : {:>+4.1} dB {} ",
                    active_model_gain, bar_gain
                ),
                line_2_style,
            ),
        ]));
    }

    // 3: Effect Wet Mix (0..100%) OR Direct Passthrough on Natural Clean
    let is_sel_3 = state.selected_control == 3;
    let mark_3 = if is_sel_3 { "> " } else { "  " };
    let line_3_style = if is_sel_3 {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    if is_active_clean {
        lines.push(Line::from(vec![
            Span::styled(mark_3, line_3_style),
            Span::styled("[4] Effect Wet Mix      : [Direct Clean]", line_3_style),
        ]));
    } else {
        let norm_mix = (state.effect_mix_pct as f32 / 10.0)
            .clamp(0.0, 10.0)
            .round() as usize;
        let bar_mix = format!("[{}{}]", "█".repeat(norm_mix), "░".repeat(10 - norm_mix));
        lines.push(Line::from(vec![
            Span::styled(mark_3, line_3_style),
            Span::styled(
                format!(
                    "[4] Effect Wet Mix      : {:>3}%     {} ",
                    state.effect_mix_pct, bar_mix
                ),
                line_3_style,
            ),
        ]));
    }

    // 4: Sidetone (On/Off)
    let is_sel_4 = state.selected_control == 4;
    let mark_4 = if is_sel_4 { "> " } else { "  " };
    let sidetone_state = if state.sidetone_enabled {
        Span::styled(
            "[ON]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("[OFF]", Style::default().fg(Color::DarkGray))
    };
    let line_4_style = if is_sel_4 {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    lines.push(Line::from(vec![
        Span::styled(mark_4, line_4_style),
        Span::styled("[5] Sidetone (Hear Mic) : ", line_4_style),
        sidetone_state,
    ]));

    // 5: VU Meter (On/Off)
    let is_sel_5 = state.selected_control == 5;
    let mark_5 = if is_sel_5 { "> " } else { "  " };
    let meter_state = if state.meter_enabled {
        Span::styled(
            "[ON]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("[OFF]", Style::default().fg(Color::DarkGray))
    };
    let line_5_style = if is_sel_5 {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    lines.push(Line::from(vec![
        Span::styled(mark_5, line_5_style),
        Span::styled("[6] Level VU Meter      : ", line_5_style),
        meter_state,
    ]));

    // 6: Spectrograph Mode (RTA / Waterfall)
    let is_sel_6 = state.selected_control == 6;
    let mark_6 = if is_sel_6 { "> " } else { "  " };
    let spec_mode_str = match state.spectrograph_mode {
        crate::ui::app::SpectrographMode::RtaBars => "[RTA SPECTRUM]",
        crate::ui::app::SpectrographMode::Waterfall => "[WATERFALL HEATMAP]",
    };
    let line_6_style = if is_sel_6 {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    lines.push(Line::from(vec![
        Span::styled(mark_6, line_6_style),
        Span::styled("[7] Spectrograph Mode   : ", line_6_style),
        Span::styled(
            spec_mode_str,
            if is_sel_6 {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            },
        ),
    ]));

    if inner.height >= 8 {
        lines.push(Line::from(vec![Span::styled(
            " [r] Reset preset to default │ [w] Save live tweaks",
            Style::default().fg(Color::DarkGray),
        )]));
    }

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_bottom_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let key_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let action_style = Style::default().fg(Color::DarkGray);

    let mut spans = if state.search_mode {
        vec![
            Span::styled(
                "Search: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}_", state.search_query),
                Style::default().fg(Color::White),
            ),
            Span::styled(" (Enter done, Esc clear)", action_style),
        ]
    } else {
        vec![
            Span::styled("j/k", key_style),
            Span::styled(" presets ", action_style),
            Span::styled("Shift+j/k", key_style),
            Span::styled(" controls ", action_style),
            Span::styled("h/l", key_style),
            Span::styled(" adjust ", action_style),
            Span::styled("Enter", key_style),
            Span::styled(" apply ", action_style),
            Span::styled("Space", key_style),
            Span::styled(" clean ", action_style),
            Span::styled("r", key_style),
            Span::styled(" reset ", action_style),
            Span::styled("w", key_style),
            Span::styled(" save ", action_style),
            Span::styled("s", key_style),
            Span::styled(" spec ", action_style),
            Span::styled("m", key_style),
            Span::styled(" mode ", action_style),
            Span::styled("v", key_style),
            Span::styled(" hear ", action_style),
            Span::styled("?", key_style),
            Span::styled(" help ", action_style),
            Span::styled("q", key_style),
            Span::styled(" quit", action_style),
        ]
    };

    if let Some(msg) = state.active_status() {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(
            fit_string(&msg, 24),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let p = Paragraph::new(Line::from(spans));
    frame.render_widget(p, area);
}

fn render_help_dialog(frame: &mut Frame, area: Rect, state: &AppState) {
    let popup_w = 72.min(area.width.saturating_sub(4));
    let popup_h = 24.min(area.height.saturating_sub(2));

    let popup_x = (area.width.saturating_sub(popup_w)) / 2;
    let popup_y = (area.height.saturating_sub(popup_h)) / 2;

    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

    frame.render_widget(Clear, popup_rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" MicStudio Help & DSP Stack ");
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    let inner_w = inner.width as usize;
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::White);

    let stages_line = state.dynamic_stages.join(" → ");

    let lines = vec![
        Line::from(vec![
            Span::styled("j / k / ↑ / ↓  ", key_style),
            Span::styled(": Navigate presets list (skips headers)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("Shift+j / k    ", key_style),
            Span::styled(": Cycle Live Controls menu down / up", desc_style),
        ]),
        Line::from(vec![
            Span::styled("h / l / ← / →  ", key_style),
            Span::styled(": Adjust active control value (- / +)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("s / m          ", key_style),
            Span::styled(
                ": Fullscreen Spectrograph / Toggle RTA vs Waterfall",
                desc_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("Enter          ", key_style),
            Span::styled(": Apply highlighted voice preset live", desc_style),
        ]),
        Line::from(vec![
            Span::styled("Space          ", key_style),
            Span::styled(": Immediately revert to Natural Clean", desc_style),
        ]),
        Line::from(vec![
            Span::styled("r              ", key_style),
            Span::styled(
                ": Reset selected preset to factory default parameters",
                desc_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("w              ", key_style),
            Span::styled(": Save current live tweaks to active preset", desc_style),
        ]),
        Line::from(vec![
            Span::styled("F5 / Ctrl+R    ", key_style),
            Span::styled(": Hot-reload presets.toml definitions from disk", desc_style),
        ]),
        Line::from(vec![
            Span::styled("/              ", key_style),
            Span::styled(": Filter presets by name or category", desc_style),
        ]),
        Line::from(vec![
            Span::styled("1 – 9          ", key_style),
            Span::styled(": Direct number jump to preset", desc_style),
        ]),
        Line::from(vec![
            Span::styled("p              ", key_style),
            Span::styled(": Audition active preset (record 5s & play)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("Tab / S-Tab    ", key_style),
            Span::styled(
                ": Cycle 7 controls (DPDFNet / Quiet Boost / Gain / Wet Mix / Sidetone / VU / Spec)",
                desc_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("v / t          ", key_style),
            Span::styled(": Toggle sidetone / Toggle VU meter", desc_style),
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
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            fit_string(&stages_line, inner_w),
            Style::default().fg(Color::Gray),
        )]),
    ];

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}
