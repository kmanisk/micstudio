//! Automated layout and rendering verification for MicStudio TUI using TestBackend.

use micstudio::ui::app::AppState;
use micstudio::ui::render::{fit_string, render_ui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn test_fit_string_utf8_truncation() {
    assert_eq!(fit_string("Natural Clean", 20), "Natural Clean");
    assert_eq!(fit_string("Natural Clean", 13), "Natural Clean");
    assert_eq!(fit_string("Natural Clean", 7), "Natura…");
    assert_eq!(fit_string("Natural Clean", 1), "…");
    assert_eq!(fit_string("Natural Clean", 0), "");
    assert_eq!(fit_string("日本語音声", 6), "日本…");
}

#[test]
fn test_render_80x24_minimal_resolution() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("Failed to create TestBackend");
    let mut state = AppState::new().expect("Failed to initialize AppState");
    state.active_preset_name = "Natural Clean".to_string();
    state.cursor = 0;

    let res = terminal.draw(|f| render_ui(f, &state));
    assert!(res.is_ok(), "Rendering at 80x24 must never panic");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer.area.width, 80);
    assert_eq!(buffer.area.height, 24);

    let mut full_rendered = String::new();
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            let cell = buffer.cell((x, y)).unwrap();
            line.push_str(cell.symbol());
        }
        full_rendered.push_str(&line);
        full_rendered.push('\n');
    }

    assert!(full_rendered.contains("MICSTUDIO"), "Top bar title missing");
    assert!(full_rendered.contains("Meter:"), "VU meter missing");
    assert!(
        !full_rendered.contains("State: Clean"),
        "State: Clean must be removed to avoid top bar clutter"
    );
    assert!(full_rendered.contains("Presets"), "Presets panel missing");
    assert!(
        full_rendered.contains("Natural Clean"),
        "Natural Clean preset missing"
    );
    assert!(
        full_rendered.contains("Live Controls"),
        "Live Controls panel missing"
    );
    assert!(
        full_rendered.contains("DPDFNet Suppression"),
        "DPDFNet live control missing"
    );
    assert!(
        full_rendered.contains("Quiet Voice Boost"),
        "Quiet Voice Boost live control missing"
    );
    assert!(
        full_rendered.contains("Clean Output Gain") || full_rendered.contains("Model Voice Boost"),
        "Output Gain live control missing"
    );
    assert!(
        full_rendered.contains("Effect Wet Mix"),
        "Effect Wet Mix live control missing"
    );
    assert!(
        full_rendered.contains("Hear Voice (Monitor)"),
        "Hear Voice live control missing"
    );
    assert!(full_rendered.contains("j/k"), "Footer hotkeys missing");
    assert!(full_rendered.contains("Space"), "Space hotkey missing");
    assert!(full_rendered.contains("clean"), "Clean hotkey missing");
}

#[test]
fn test_natural_clean_quiet_boost_vs_fx_wet_mix() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().unwrap();

    // Natural clean active -> shows Quiet Voice Boost + Clean Output Gain + Direct Broadcast
    state.active_preset_name = "Natural Clean".to_string();
    terminal.draw(|f| render_ui(f, &state)).unwrap();
    let buffer = terminal.backend().buffer();
    let clean_rendered: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(
        clean_rendered.contains("Quiet Voice Boost"),
        "Quiet Voice Boost must be visible on Natural Clean"
    );
    assert!(
        clean_rendered.contains("Clean Output Gain"),
        "Clean Output Gain must be shown on Natural Clean"
    );
    assert!(
        clean_rendered.contains("Direct Clean"),
        "Direct Clean indicator must be shown on Natural Clean"
    );

    // Deep Voice active -> shows Quiet Voice Boost (universal) + Model Voice Boost (+2.0 dB) + Effect Wet Mix
    state.active_preset_name = "Deep Voice".to_string();
    terminal.draw(|f| render_ui(f, &state)).unwrap();
    let buffer2 = terminal.backend().buffer();
    let fx_rendered: String = buffer2.content().iter().map(|c| c.symbol()).collect();
    assert!(
        fx_rendered.contains("Quiet Voice Boost"),
        "Quiet Voice Boost must be available as universal option across all FX models"
    );
    assert!(
        fx_rendered.contains("Model Voice Boost"),
        "Model Voice Boost (+2.0 dB default) must be shown on Deep Voice"
    );
    assert!(
        fx_rendered.contains("Effect Wet Mix"),
        "Effect Wet Mix must be visible on Deep Voice"
    );
}

#[test]
fn test_render_100x30_and_200x50_clean_scaling() {
    // 100x30
    {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = AppState::new().unwrap();
        assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());
    }

    // 200x50
    {
        let backend = TestBackend::new(200, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = AppState::new().unwrap();
        assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());
    }
}

#[test]
fn test_render_too_small_70x20_displays_guard_message() {
    let backend = TestBackend::new(70, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = AppState::new().unwrap();

    assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());

    let buffer = terminal.backend().buffer();
    let mut content = String::new();
    for y in 0..20 {
        for x in 0..70 {
            if let Some(cell) = buffer.cell((x, y)) {
                content.push_str(cell.symbol());
            }
        }
    }

    assert!(
        content.contains("terminal too small (need 80x24"),
        "Buffer should contain terminal too small warning, got: {}",
        content
    );
}

#[test]
fn test_scrolling_over_27_presets_and_category_headers() {
    let mut state = AppState::new().unwrap();
    let total = state.presets.len();
    assert!(total >= 20, "Expected at least 20 presets, got {}", total);

    for idx in 0..total {
        state.cursor = idx;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let res = terminal.draw(|f| render_ui(f, &state));
        assert!(
            res.is_ok(),
            "Rendering preset index {} ('{}') at 80x24 must not panic",
            idx,
            state.presets[idx].name
        );

        let buffer = terminal.backend().buffer();
        for y in 0..24 {
            for x in 0..80 {
                assert!(buffer.cell((x, y)).is_some());
            }
        }
    }
}

#[test]
fn test_search_filtering_and_navigation() {
    let mut state = AppState::new().unwrap();
    state.enter_search();
    assert!(state.search_mode);

    // Search for "robot"
    for c in "robot".chars() {
        state.handle_search_char(c);
    }
    assert_eq!(state.search_query, "robot");

    let filtered = state.filtered_indices();
    assert!(!filtered.is_empty(), "Expected matches for 'robot'");
    for &idx in &filtered {
        let p = &state.presets[idx];
        assert!(
            p.name.to_lowercase().contains("robot") || p.category.to_lowercase().contains("robot"),
            "Preset '{}' in '{}' did not match search query 'robot'",
            p.name,
            p.category
        );
    }

    // Render filtered state
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let res = terminal.draw(|f| render_ui(f, &state));
    assert!(res.is_ok(), "Rendering in search mode must not panic");

    // Exit search
    state.exit_search(true);
    assert!(!state.search_mode);
    assert!(state.search_query.is_empty());
    assert_eq!(state.filtered_indices().len(), state.presets.len());
}

#[test]
fn test_help_popup_renders_cleanly_at_80x24() {
    let mut state = AppState::new().unwrap();
    state.show_help = true;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());

    let buffer = terminal.backend().buffer();
    let mut content = String::new();
    for y in 0..24 {
        for x in 0..80 {
            content.push_str(buffer.cell((x, y)).unwrap().symbol());
        }
    }

    assert!(content.contains("MicStudio Help & DSP Stack"));
    assert!(content.contains("Active 40-mic.conf Stages"));
}

#[test]
fn test_print_screen_dumps_80x24() {
    let mut state = AppState::new().unwrap();

    // 1. Natural Clean screen dump
    state.active_preset_name = "Natural Clean".to_string();
    state.cursor = 0;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render_ui(f, &state)).unwrap();
    let buffer = terminal.backend().buffer();

    println!("\n=== SCREEN DUMP 1: NATURAL CLEAN (80x24) ===");
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer.cell((x, y)).unwrap().symbol());
        }
        println!("{}", line);
    }

    // 2. Deep Voice screen dump
    state.active_preset_name = "Deep Voice".to_string();
    state.cursor = 5; // In Character category
    let backend2 = TestBackend::new(80, 24);
    let mut terminal2 = Terminal::new(backend2).unwrap();
    terminal2.draw(|f| render_ui(f, &state)).unwrap();
    let buffer2 = terminal2.backend().buffer();

    println!("\n=== SCREEN DUMP 2: DEEP VOICE (80x24) ===");
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer2.cell((x, y)).unwrap().symbol());
        }
        println!("{}", line);
    }

    // 3. Modal details screen dump
    state.show_details_modal = true;
    terminal2.draw(|f| render_ui(f, &state)).unwrap();
    let buffer3 = terminal2.backend().buffer();
    println!("\n=== SCREEN DUMP 3: DETAILS MODAL (80x24) ===");
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer3.cell((x, y)).unwrap().symbol());
        }
        println!("{}", line);
    }
}

#[test]
fn test_shift_j_k_cycle_live_controls() {
    let mut state = AppState::new().unwrap();
    assert_eq!(state.selected_control, 0);

    // Shift+j goes down
    state.cycle_control_next();
    assert_eq!(state.selected_control, 1); // QuietBoost
    state.cycle_control_next();
    assert_eq!(state.selected_control, 2); // OutputGain / ModelGain
    state.cycle_control_next();
    assert_eq!(state.selected_control, 3); // EffectMix
    state.cycle_control_next();
    assert_eq!(state.selected_control, 4); // Sidetone
    state.cycle_control_next();
    assert_eq!(state.selected_control, 5); // VU Meter
    state.cycle_control_next();
    assert_eq!(state.selected_control, 6); // Spectrograph mode
    state.cycle_control_next();
    assert_eq!(state.selected_control, 0); // Wraps around

    // Shift+k goes up
    state.cycle_control_prev();
    assert_eq!(state.selected_control, 6);
    state.cycle_control_prev();
    assert_eq!(state.selected_control, 5);
    state.cycle_control_prev();
    assert_eq!(state.selected_control, 4);
    state.cycle_control_prev();
    assert_eq!(state.selected_control, 3);
}

#[test]
fn test_spectrograph_rendering_and_modes() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().unwrap();
    state.sidetone_enabled = true;

    // 1. Embedded RTA Bars mode (when sidetone is active)
    assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());
    let buf = terminal.backend().buffer().clone();
    let mut rendered = String::new();
    for y in 0..30 {
        for x in 0..100 {
            rendered.push_str(buf.cell((x, y)).unwrap().symbol());
        }
    }
    assert!(rendered.contains("Spectrograph [Active]"));
    assert!(rendered.contains("Live Controls (Shift+j/k or Tab to cycle, h/l to adjust)"));

    // 2. Waterfall mode
    state.spectrograph_mode = micstudio::ui::app::SpectrographMode::Waterfall;
    assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());

    // 3. Fullscreen Spectrograph
    state.spectrograph_fullscreen = true;
    assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());
}

#[test]
fn test_tui_pipeline_graph_and_preset_details() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().unwrap();
    state.active_preset_name = "Deep Voice".to_string();
    if let Some(pos) = state
        .filtered_indices()
        .iter()
        .position(|&idx| state.presets[idx].name == "Deep Voice")
    {
        state.cursor = pos;
    }

    assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());

    let buffer = terminal.backend().buffer();
    let mut content = String::new();
    for y in 0..30 {
        for x in 0..100 {
            content.push_str(buffer.cell((x, y)).unwrap().symbol());
        }
        content.push('\n');
    }

    assert!(
        content.contains("Pitch:"),
        "Pitch spec row missing in details pane"
    );
    assert!(
        content.contains("Denoise:"),
        "Denoise spec row missing in details pane"
    );
    assert!(
        content.contains("Active"),
        "Active status missing for active preset"
    );
    assert!(
        content.contains("Model details"),
        "Model details modal hint missing"
    );
    assert!(
        content.contains("reset"),
        "Reset shortcut missing from bottom bar"
    );
    assert!(
        content.contains("save"),
        "Save shortcut missing from bottom bar"
    );

    // Modal popup test
    state.show_details_modal = true;
    assert!(terminal.draw(|f| render_ui(f, &state)).is_ok());
    let buf_modal = terminal.backend().buffer().clone();
    let mut modal_content = String::new();
    for y in 0..30 {
        for x in 0..100 {
            modal_content.push_str(buf_modal.cell((x, y)).unwrap().symbol());
        }
        modal_content.push('\n');
    }
    assert!(modal_content.contains("Model Details & Chain"));
    assert!(modal_content.contains("State:"));
    assert!(modal_content.contains("Chain:"));
}
