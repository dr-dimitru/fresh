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

const FOLD_DOCUMENT: &str = "# Folding\n\nOpening paragraph with **emphasis** for the cursor.\n\n- Parent item with **bold** words\n  - Child alpha\n  - Child beta\n  - Child gamma\n- Sibling item\n\n```rust\nlet folded = \"frame\";\n```\n\nClosing paragraph with **strong** markup.\n\nFinal sentinel.\n";

/// Column of the vertical split's separator, read from the tab-bar row, which
/// holds no buffer text that could contain a frame rail.
fn separator_col(screen: &str) -> usize {
    screen
        .lines()
        .nth(1)
        .unwrap()
        .chars()
        .position(|c| c == '│')
        .unwrap()
}

/// Text of one side of a vertical split, row by row, so a pane's rows can be
/// compared without the sibling's text leaking into the assertion.
fn pane_rows(screen: &str, left: bool) -> Vec<String> {
    let sep = separator_col(screen);
    screen
        .lines()
        .map(|row| {
            if left {
                row.chars().take(sep).collect()
            } else {
                row.chars().skip(sep + 1).collect()
            }
        })
        .collect()
}

fn row_of(rows: &[String], needle: &str) -> Option<usize> {
    rows.iter().position(|row| row.contains(needle))
}

#[test]
fn folded_compose_pane_keeps_its_own_layout_cursor_and_scroll() {
    let md = format!(
        "{FOLD_DOCUMENT}\n{}",
        (1..=30)
            .map(|n| format!("Tail paragraph {n:02} with **markup**.\n\n"))
            .collect::<String>()
    );
    let (mut h, _tmp) = compose_harness(&md, Config::default(), 140, 30);
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    command(&mut h, "Split Vertical");
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);

    // Fold the nested list in the left pane only, from its parent item.
    let rows = pane_rows(&h.screen_to_string(), true);
    let parent = row_of(&rows, "Parent item").unwrap();
    h.mouse_click(10, parent as u16).unwrap();
    command(&mut h, "Toggle Fold");
    h.wait_until_stable(|h| {
        !pane_rows(&h.screen_to_string(), true)
            .iter()
            .any(|r| r.contains("Child beta"))
    })
    .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let screen = h.screen_to_string();
    let (left, right) = (pane_rows(&screen, true), pane_rows(&screen, false));
    for child in ["Child alpha", "Child beta", "Child gamma"] {
        assert!(
            row_of(&left, child).is_none(),
            "left pane folds {child}:\n{screen}"
        );
        assert!(
            row_of(&right, child).is_some(),
            "right pane keeps {child}:\n{screen}"
        );
    }
    assert!(
        left[parent].contains("- Parent item"),
        "the left cursor reveals its own list marker:\n{screen}"
    );
    assert!(
        right[parent].contains("• Parent item"),
        "the right pane keeps the composed marker it had:\n{screen}"
    );
    assert!(
        row_of(&right, "# Folding").is_some() && row_of(&left, "# Folding").is_none(),
        "each pane reveals markup only on its own cursor line:\n{screen}"
    );
    let left_frame = row_of(&left, "┌").unwrap();
    let right_frame = row_of(&right, "┌").unwrap();
    assert_eq!(
        right_frame - left_frame,
        6,
        "the fold moves only the left pane's frame up by the three hidden items:\n{screen}"
    );
    for rows in [&left, &right] {
        let top = row_of(rows, "┌").unwrap();
        assert!(
            rows[top].contains('┐') && rows[top + 2].contains('┘'),
            "frames stay closed:\n{screen}"
        );
    }

    // Down from the folded parent skips the hidden children in this pane.
    h.send_key(KeyCode::Down, KeyModifiers::NONE).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let cursor = h.render_observing_cursor().unwrap().expect("left caret");
    let left = pane_rows(&h.screen_to_string(), true);
    assert!(
        left[cursor.1 as usize].contains("- Sibling item"),
        "the caret lands on the sibling after the fold:\n{}",
        h.screen_to_string()
    );

    // Scroll the unfolded right pane; the folded left pane must not move.
    let right_x = 80;
    for _ in 0..4 {
        h.mouse_scroll_down(right_x, 10).unwrap();
    }
    h.wait_until_stable(|h| {
        !pane_rows(&h.screen_to_string(), false)
            .iter()
            .any(|r| r.contains("Opening paragraph"))
    })
    .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let screen = h.screen_to_string();
    let (left, right) = (pane_rows(&screen, true), pane_rows(&screen, false));
    assert!(
        row_of(&left, "Opening paragraph").is_some(),
        "left pane keeps its scroll:\n{screen}"
    );
    assert!(
        row_of(&left, "Child beta").is_none(),
        "left fold survives sibling scroll:\n{screen}"
    );

    // Click into the scrolled right pane and type where the click landed.
    let pane_left = separator_col(&screen) + 1;
    let target = row_of(&right, "Tail paragraph 03").expect("scrolled pane shows the tail");
    let byte = right[target].find("paragraph").unwrap();
    let col = right[target][..byte].chars().count();
    h.mouse_click((pane_left + col) as u16, target as u16)
        .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    h.type_text("X").unwrap();
    h.wait_until_stable(|h| h.screen_to_string().contains("Tail Xparagraph 03"))
        .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let cursor = h.render_observing_cursor().unwrap().expect("right caret");
    let screen = h.screen_to_string();
    let right = pane_rows(&screen, false);
    let row = &right[cursor.1 as usize];
    assert!(
        row.contains("Tail Xparagraph 03"),
        "caret stays on the edited row:\n{screen}"
    );
    let typed = row[..row.find("paragraph 03").unwrap()].chars().count();
    assert_eq!(
        cursor.0 as usize,
        pane_left + typed,
        "caret sits just after the typed character:\n{screen}"
    );
    let left = pane_rows(&screen, true);
    assert!(
        row_of(&left, "Child beta").is_none(),
        "left fold survives an edit in the sibling:\n{screen}"
    );
    assert!(
        row_of(&left, "Opening paragraph").is_some(),
        "left scroll survives an edit in the sibling:\n{screen}"
    );
}

/// Width of the first table frame drawn in one pane, corner to corner.
fn first_table_width(rows: &[String]) -> Option<usize> {
    let top = rows.iter().find(|row| row.contains('┬'))?;
    let left = top.chars().position(|c| c == '┌')?;
    let right = top.chars().position(|c| c == '┐')?;
    Some(right - left)
}

#[test]
fn table_width_growth_in_one_pane_reaches_the_sibling() {
    // Tall enough that the first bounded batch misses the wide row, so the
    // shared width memo grows while only one pane is scrolled to it.
    let mut md = String::from("# Table\n\n| Key | Value |\n| --- | --- |\n");
    for n in 1..=400 {
        if n == 300 {
            md.push_str("| a-much-longer-key-cell | v |\n");
        } else {
            md.push_str(&format!("| r{n:04} | v |\n"));
        }
    }
    md.push_str("\nFinal sentinel.\n");
    let (mut h, _tmp) = compose_harness(&md, Config::default(), 140, 16);
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    command(&mut h, "Split Vertical");
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    let narrow = first_table_width(&pane_rows(&h.screen_to_string(), true)).unwrap();

    // Scroll the left pane continuously down to the wide row, then back up.
    let left_shows = |h: &EditorTestHarness, text: &str| {
        row_of(&pane_rows(&h.screen_to_string(), true), text).is_some()
    };
    h.mouse_click(20, 8).unwrap();
    while !left_shows(&h, "a-much-longer-key-cell") {
        h.send_key(KeyCode::PageDown, KeyModifiers::NONE).unwrap();
        h.wait_for_async_quiescence(4).unwrap();
    }
    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
    h.wait_until_stable(|h| left_shows(h, "# Table")).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let screen = h.screen_to_string();
    let left = first_table_width(&pane_rows(&screen, true)).unwrap();
    let right = first_table_width(&pane_rows(&screen, false)).unwrap();
    assert!(left > narrow, "the wide row widens the table:\n{screen}");
    assert_eq!(
        right, left,
        "the sibling showing the same table adopts the widened columns:\n{screen}"
    );
}

#[test]
fn reopened_fence_restyles_every_compose_pane() {
    let md = "# Fence\n\n```rust\nlet a = 1;\n```\n\nParagraph below the fence.\n\nFinal sentinel.\n";
    let (mut h, _tmp) = compose_harness(md, Config::default(), 140, 24);
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    command(&mut h, "Split Vertical");
    command(&mut h, "Toggle Compose");
    settle_frame(&mut h);
    let paragraph_prefix = |rows: &[String]| {
        let row = &rows[row_of(rows, "Paragraph below").unwrap()];
        row[..row.find("Paragraph below").unwrap()].to_string()
    };
    let screen = h.screen_to_string();
    let before = paragraph_prefix(&pane_rows(&screen, true));
    assert_eq!(before, paragraph_prefix(&pane_rows(&screen, false)));

    // Typing a word after the closing fence (no backtick) stops it closing,
    // so the paragraph below becomes code in every pane showing the file.
    h.mouse_click(20, 6).unwrap();
    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
    h.send_key_repeat(KeyCode::Down, KeyModifiers::NONE, 4)
        .unwrap();
    h.send_key(KeyCode::End, KeyModifiers::NONE).unwrap();
    h.type_text(" x").unwrap();
    h.wait_until_stable(|h| paragraph_prefix(&pane_rows(&h.screen_to_string(), true)) != before)
        .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let screen = h.screen_to_string();
    assert_eq!(
        paragraph_prefix(&pane_rows(&screen, false)),
        paragraph_prefix(&pane_rows(&screen, true)),
        "the sibling pane must restyle the paragraph too:\n{screen}"
    );
}
