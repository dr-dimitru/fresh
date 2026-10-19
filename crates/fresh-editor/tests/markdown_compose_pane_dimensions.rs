//! Compose must measure the pane that draws it, including its resolved gutter.
//! These regressions drive the real plugin and observe terminal output only.
#![cfg(feature = "plugins")]

use crate::common::harness::{copy_plugin, copy_plugin_lib, EditorTestHarness, HarnessOptions};
use crossterm::event::{KeyCode, KeyModifiers};
use fresh::config::Config;

const DOCUMENT: &str = "# Geometry\n\nIntro.\n\n```rust\nlet greeting = \"Hello 世界 café\";\n```\n\n| Name | Description |\n| --- | --- |\n| Unicode | 世界 café and words in a cell, extended far enough that both composing panes must independently wrap this description to their own available measure |\n\nA paragraph with **emphasis** and enough words to wrap across a narrow pane without splitting the frame belonging to the code above.\n\nFinal sentinel.\n";

fn compose_harness(
    md: &str,
    config: Config,
    width: u16,
    height: u16,
) -> (EditorTestHarness, tempfile::TempDir) {
    let temp = tempfile::TempDir::new().unwrap();
    let plugins = temp.path().join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    copy_plugin(&plugins, "markdown_compose");
    copy_plugin_lib(&plugins);
    std::fs::write(plugins.join("compose_test_control.ts"), r#"
const editor = getEditor();
registerHandler("composeTestReload", () => editor.reloadConfig());
editor.registerCommand("Compose test reload", "Reload isolated test config", "composeTestReload", null);
editor.on("config_changed", () => editor.setStatus("compose test config reloaded"));
"#).unwrap();
    let path = temp.path().join("geometry.md");
    std::fs::write(&path, md).unwrap();
    let mut harness = EditorTestHarness::create(
        width,
        height,
        HarnessOptions::new()
            .with_config(config)
            .with_working_dir(temp.path().to_path_buf())
            .without_empty_plugins_dir()
            .with_full_grammar_registry(),
    )
    .unwrap();
    harness.editor_mut().set_clipboard_for_test(String::new());
    harness.open_file(&path).unwrap();
    harness.render().unwrap();
    (harness, temp)
}

fn command(h: &mut EditorTestHarness, name: &str) {
    h.run_palette_command(name).unwrap();
    h.wait_for_prompt_closed().unwrap();
}

fn settle_frame(h: &mut EditorTestHarness) {
    h.wait_until_stable(|h| h.screen_to_string().contains('┌'))
        .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
}

fn assert_closed_code_frame(h: &EditorTestHarness) {
    let screen = h.screen_to_string();
    let top = screen
        .lines()
        .find(|row| row.contains('┌') && !row.contains('┬'))
        .unwrap_or_else(|| panic!("no code frame on screen:\n{screen}"));
    assert!(
        top.contains('┐'),
        "code top wraps its right corner:\n{screen}"
    );
    let bottom = screen
        .lines()
        .find(|row| row.contains('└') && !row.contains('┴'))
        .unwrap_or_else(|| panic!("no code bottom on screen:\n{screen}"));
    assert!(
        bottom.contains('┘'),
        "code bottom wraps its right corner:\n{screen}"
    );
    let corners = |row: &str, left: char, right: char| {
        let chars: Vec<char> = row.chars().collect();
        (
            chars.iter().position(|c| *c == left),
            chars.iter().position(|c| *c == right),
        )
    };
    assert_eq!(
        corners(top, '┌', '┐'),
        corners(bottom, '└', '┘'),
        "{screen}"
    );
    let (Some(left), Some(right)) = corners(top, '┌', '┐') else {
        unreachable!()
    };
    let top_row = screen.lines().position(|row| row == top).unwrap();
    let bottom_row = screen.lines().position(|row| row == bottom).unwrap();
    for row in top_row + 1..bottom_row {
        assert_eq!(
            h.get_cell(left as u16, row as u16).as_deref(),
            Some("│"),
            "left rail row {row}:\n{screen}"
        );
        assert_eq!(
            h.get_cell(right as u16, row as u16).as_deref(),
            Some("│"),
            "right rail row {row}:\n{screen}"
        );
    }
}

fn assert_closed_table_frame(h: &EditorTestHarness) {
    let screen = h.screen_to_string();
    let top = screen
        .lines()
        .find(|row| row.contains('┬'))
        .unwrap_or_else(|| panic!("no table top:\n{screen}"));
    let bottom = screen
        .lines()
        .find(|row| row.contains('┴'))
        .unwrap_or_else(|| panic!("no table bottom:\n{screen}"));
    let corners = |row: &str, left, right| {
        (
            row.chars().position(|c| c == left),
            row.chars().position(|c| c == right),
        )
    };
    let top_corners = corners(top, '┌', '┐');
    assert!(
        top_corners.0.is_some() && top_corners.1.is_some(),
        "table corners must share a row:\n{screen}"
    );
    assert_eq!(
        top_corners,
        corners(bottom, '└', '┘'),
        "table edges must align:\n{screen}"
    );
}

#[test]
fn numbered_narrow_compose_keeps_code_corners_and_rails_aligned() {
    for width in [44, 140] {
        let (mut h, _tmp) = compose_harness(DOCUMENT, Config::default(), width, 40);
        command(&mut h, "Toggle Compose");
        settle_frame(&mut h);
        command(&mut h, "Toggle Line Numbers (Current Buffer)");
        settle_frame(&mut h);
        let screen = h.screen_to_string();
        assert!(
            screen.contains("│ # Geometry"),
            "explicit pin not shown:\n{screen}"
        );
        assert_closed_code_frame(&h);
        assert_closed_table_frame(&h);
    }
}

#[test]
fn dual_preview_uses_its_own_width_while_source_is_focused() {
    let (mut h, _tmp) = compose_harness(DOCUMENT, Config::default(), 100, 40);
    command(&mut h, "Markdown: Source/Compose Dual Mode");
    settle_frame(&mut h);
    assert!(
        h.screen_to_string().contains("```rust"),
        "Source remains literal"
    );
    assert_closed_code_frame(&h);
    h.resize(76, 40).unwrap();
    settle_frame(&mut h);
    assert_closed_code_frame(&h);
}

#[test]
fn unequal_compose_panes_do_not_replace_each_others_layout() {
    let (mut h, _tmp) = compose_harness(DOCUMENT, Config::default(), 140, 40);
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    command(&mut h, "Split Vertical");
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    // The ordinary split starts halfway across the screen. Drag its separator,
    // not a buffer cell, so the two panes now have visibly different widths.
    h.mouse_drag(70, 20, 48, 20).unwrap();
    h.mouse_click(80, 3).unwrap();
    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
    command(&mut h, "Toggle Line Numbers (Current Buffer)");
    settle_frame(&mut h);
    let screen = h.screen_to_string();
    let top = screen.lines().find(|row| row.contains('┌')).unwrap();
    assert_eq!(
        top.matches('┌').count(),
        2,
        "both panes draw a frame:\n{screen}"
    );
    assert_eq!(
        top.matches('┐').count(),
        2,
        "neither frame wraps its corner:\n{screen}"
    );
    let table_widths = |screen: &str| {
        let top = screen.lines().find(|row| row.contains('┬')).unwrap();
        let lefts: Vec<_> = top
            .chars()
            .enumerate()
            .filter_map(|(i, c)| (c == '┌').then_some(i))
            .collect();
        let rights: Vec<_> = top
            .chars()
            .enumerate()
            .filter_map(|(i, c)| (c == '┐').then_some(i))
            .collect();
        assert_eq!(
            (lefts.len(), rights.len()),
            (2, 2),
            "both tables must fit their pane:\n{screen}"
        );
        lefts
            .into_iter()
            .zip(rights)
            .map(|(left, right)| right - left)
            .collect::<Vec<_>>()
    };
    let widths = table_widths(&screen);
    assert!(
        widths[1] > widths[0],
        "the wider pane must allocate its own table width:\n{screen}"
    );
    h.mouse_click(10, 2).unwrap();
    settle_frame(&mut h);
    let screen = h.screen_to_string();
    let top = screen.lines().find(|row| row.contains('┌')).unwrap();
    assert_eq!(
        top.matches('┐').count(),
        2,
        "focus must not replace sibling layout:\n{screen}"
    );
    assert_eq!(
        table_widths(&screen),
        widths,
        "focus must not replace a sibling table's allocation"
    );
}

#[test]
fn compose_line_number_policy_respects_global_and_explicit_pins() {
    use fresh::config::PluginConfig;
    for global in [true, false] {
        for policy in [None, Some("hide"), Some("inherit")] {
            let mut config = Config::default();
            config.editor.line_numbers = global;
            if let Some(policy) = policy {
                config.plugins.insert(
                    "markdown_compose".into(),
                    PluginConfig {
                        enabled: true,
                        path: None,
                        settings: serde_json::json!({"lineNumbers": policy}),
                    },
                );
            }
            let (mut h, _tmp) = compose_harness(DOCUMENT, config, 140, 40);
            command(&mut h, "Toggle Compose");
            settle_frame(&mut h);
            let visible = policy == Some("inherit") && global;
            let screen = h.screen_to_string();
            assert_eq!(
                screen.contains("│ # Geometry"),
                visible,
                "global={global} policy={policy:?}\n{screen}"
            );
            command(&mut h, "Toggle Line Numbers (Current Buffer)");
            settle_frame(&mut h);
            assert_eq!(h.screen_to_string().contains("│ # Geometry"), !visible);
            command(&mut h, "Toggle Compose");
            h.wait_for_async_quiescence(4).unwrap();
            assert_eq!(
                h.screen_to_string().contains("│ # Geometry"),
                !visible,
                "user pin survives exit"
            );
            command(&mut h, "Toggle Compose");
            settle_frame(&mut h);
            assert_eq!(
                h.screen_to_string().contains("│ # Geometry"),
                !visible,
                "user pin beats mode default on re-entry"
            );
        }
    }
}

#[test]
fn gutter_digit_growth_reflows_with_unchanged_pane_width() {
    for boundary in [99, 999] {
        let mut lines: Vec<_> = DOCUMENT.lines().map(str::to_string).collect();
        lines.resize(boundary, "filler".into());
        let md = lines.join("\n");
        let (mut h, _tmp) = compose_harness(&md, Config::default(), 44, 40);
        command(&mut h, "Toggle Compose");
        settle_frame(&mut h);
        command(&mut h, "Toggle Line Numbers (Current Buffer)");
        settle_frame(&mut h);
        let left_corner = |h: &EditorTestHarness| {
            h.screen_to_string()
                .lines()
                .find(|r| r.contains('┌') && !r.contains('┬'))
                .unwrap()
                .chars()
                .position(|c| c == '┌')
                .unwrap()
        };
        let before = left_corner(&h);
        h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
        h.send_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
        h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
        settle_frame(&mut h);
        assert_closed_code_frame(&h);
        assert_closed_table_frame(&h);
        assert_eq!(
            left_corner(&h),
            before + 1,
            "{boundary}→{} grows gutter:\n{}",
            boundary + 1,
            h.screen_to_string()
        );
        h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
        h.send_key(KeyCode::Backspace, KeyModifiers::NONE).unwrap();
        h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
        settle_frame(&mut h);
        assert_closed_code_frame(&h);
        assert_closed_table_frame(&h);
        assert_eq!(
            left_corner(&h),
            before,
            "deleting the extra line restores the gutter"
        );
    }
}

#[test]
fn resized_compose_reaches_eof_and_types_at_visible_cursor() {
    for numbers in [false, true] {
        let md = format!(
            "{}\n{}\nFinal sentinel.",
            DOCUMENT,
            "Long **paragraph** with words that wrap and Unicode 世界 café.\n\n".repeat(80)
        );
        let (mut h, _tmp) = compose_harness(&md, Config::default(), 100, 30);
        command(&mut h, "Toggle Compose");
        settle_frame(&mut h);
        if numbers {
            command(&mut h, "Toggle Line Numbers (Current Buffer)");
        }
        for width in [44, 140, 60] {
            h.resize(width, 30).unwrap();
            h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
            h.wait_until_stable(|h| h.screen_to_string().contains("Final sentinel."))
                .unwrap();
            h.wait_for_async_quiescence(4).unwrap();
            let cursor = h
                .render_observing_cursor()
                .unwrap()
                .expect("EOF caret is visible");
            let row = h.get_screen_row(cursor.1 as usize);
            assert!(
                row.contains("Final sentinel."),
                "caret {:?} must be on the final row:\n{}",
                cursor,
                h.screen_to_string()
            );
            let prefix = row.split("Final sentinel.").next().unwrap();
            assert_eq!(
                cursor.0 as usize,
                prefix.chars().count() + "Final sentinel.".len(),
                "EOF caret must be just after the final character, not merely on its row"
            );
            h.type_text("X").unwrap();
            h.wait_until_stable(|h| h.screen_to_string().contains("sentinel.X"))
                .unwrap();
            h.send_key(KeyCode::Backspace, KeyModifiers::NONE).unwrap();
        }
    }
}

#[test]
fn settled_pane_batches_do_not_scale_with_document_length() {
    use fresh::view::pane_decorations::offer_stats;
    for count in [150, 1500] {
        let md = format!(
            "{}\n{}",
            DOCUMENT,
            "ordinary paragraph with **markup**\n\n".repeat(count)
        );
        let before = offer_stats();
        let (mut h, _tmp) = compose_harness(&md, Config::default(), 44, 30);
        command(&mut h, "Toggle Compose");
        settle_frame(&mut h);
        let settled = offer_stats();
        assert!(settled.batches > before.batches);
        assert!(
            settled.bytes - before.bytes
                <= 2 * (44 * 30 * 4 + 1024) * (settled.batches - before.batches)
        );
        for _ in 0..12 {
            h.render().unwrap();
        }
        assert_eq!(
            offer_stats().batches,
            settled.batches,
            "unchanged frames must not re-offer the document"
        );
    }
}

#[test]
fn policy_reload_updates_unfocused_preview_without_moving_focus() {
    let (mut h, temp) = compose_harness(DOCUMENT, Config::default(), 140, 40);
    command(&mut h, "Markdown: Source/Compose Dual Mode");
    settle_frame(&mut h);
    let heading_numbers = |h: &EditorTestHarness| {
        h.screen_to_string()
            .lines()
            .find(|row| row.contains("# Geometry"))
            .unwrap()
            .matches("│ # Geometry")
            .count()
    };
    assert_eq!(
        heading_numbers(&h),
        1,
        "Source has numbers, default preview does not"
    );
    let config_dir = temp.path().join(".fresh");
    std::fs::create_dir(&config_dir).unwrap();
    let write_policy = |policy: &str| {
        std::fs::write(config_dir.join("config.json"), serde_json::json!({
        "plugins": { "markdown_compose": { "enabled": true, "settings": {"lineNumbers": policy} } }
    }).to_string()).unwrap()
    };
    write_policy("inherit");
    command(&mut h, "Compose test reload");
    h.wait_for_screen_contains("compose test config reloaded")
        .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert_eq!(
        heading_numbers(&h),
        2,
        "unfocused preview should inherit numbers:\n{}",
        h.screen_to_string()
    );
    assert!(
        h.render_observing_cursor().unwrap().unwrap().0 < 70,
        "Source must keep focus"
    );
    // Pin the preview off, return focus to Source, then reload both policies.
    h.mouse_click(90, 3).unwrap();
    command(&mut h, "Toggle Line Numbers (Current Buffer)");
    h.mouse_click(15, 3).unwrap();
    for policy in ["hide", "inherit"] {
        write_policy(policy);
        command(&mut h, "Compose test reload");
        h.wait_for_screen_contains("compose test config reloaded")
            .unwrap();
        h.wait_for_async_quiescence(4).unwrap();
        assert_eq!(
            heading_numbers(&h),
            1,
            "reload must not overwrite the preview's pin:\n{}",
            h.screen_to_string()
        );
        assert!(h.render_observing_cursor().unwrap().unwrap().0 < 70);
    }
}

#[test]
fn two_compose_panes_replay_seen_scroll_without_new_batches() {
    use fresh::view::pane_decorations::offer_stats;
    let md = format!("{}\n{}", DOCUMENT, "A paragraph with **markup** and words long enough to wrap across several rows of either composing pane, but without whole-document layout work.\n\n".repeat(400));
    let (mut h, _temp) = compose_harness(&md, Config::default(), 100, 18);
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    command(&mut h, "Split Vertical");
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    command(&mut h, "Toggle Line Numbers (Current Buffer)");
    h.wait_for_async_quiescence(4).unwrap();
    let before = offer_stats();
    for _ in 0..20 {
        h.mouse_scroll_down(20, 10).unwrap();
        h.wait_for_async_quiescence(4).unwrap();
    }
    let first = offer_stats();
    assert!(
        first.batches > before.batches,
        "first scroll must exercise newly offered content"
    );
    assert!(
        first.bytes - before.bytes <= 2 * (100 * 18 * 4 + 1024) * (first.batches - before.batches)
    );
    h.mouse_click(15, 3).unwrap();
    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
    h.wait_until_stable(|h| h.screen_to_string().contains("# Geometry"))
        .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let replay = offer_stats();
    for _ in 0..20 {
        h.mouse_scroll_down(20, 10).unwrap();
        h.wait_for_async_quiescence(4).unwrap();
    }
    assert_eq!(
        offer_stats().batches,
        replay.batches,
        "replaying seen content must not re-offer either pane"
    );
}
