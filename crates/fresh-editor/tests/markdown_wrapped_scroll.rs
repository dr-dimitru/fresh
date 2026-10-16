//! Small Markdown documents must retain logical line labels while wrapping,
//! scrolling, resizing and editing, with both loading strategies.

use crate::common::harness::{EditorTestHarness, HarnessOptions};
use crossterm::event::{KeyCode, KeyModifiers};
use fresh::config::Config;

fn assert_row_labels(screen: &str, numbers: bool) {
    let mut rows = 0;
    for line in screen.lines() {
        let Some((prefix, text)) = line.split_once("Row ") else {
            continue;
        };
        let Some(number) = text.get(..3).and_then(|s| s.parse::<usize>().ok()) else {
            continue;
        };
        rows += 1;
        if numbers {
            assert!(
                prefix.ends_with(&format!("{number} │ ")),
                "logical line {number} has the wrong label: {line:?}\n{screen}"
            );
        } else {
            assert!(
                !prefix.chars().any(|c| c.is_ascii_digit()),
                "line numbers should be hidden without hiding gutter decorations: {line:?}"
            );
        }
    }
    assert!(rows > 0, "no source rows on screen:\n{screen}");
}

#[test]
fn keyboard_navigation_finishes_pending_wheel_scroll() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("navigation.md");
    std::fs::write(
        &path,
        (1..=150)
            .map(|i| {
                format!(
                    "Row {i:03} **bold** café 日本語 {}",
                    "enough prose to cross a screen row. ".repeat(4)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let mut config = Config::default();
    config.editor.smooth_scroll = true;
    config.editor.animations = true;
    config.editor.line_wrap = false;
    config.editor.line_numbers = false;
    let mut h = EditorTestHarness::create(
        90,
        28,
        HarnessOptions::new()
            .with_config(config)
            .with_working_dir(temp.path().to_path_buf())
            .with_full_grammar_registry(),
    )
    .unwrap();
    h.open_file(&path).unwrap();
    h.wait_for_screen_contains("Row 001").unwrap();
    for _ in 0..15 {
        h.mouse_scroll_down(60, 12).unwrap();
    }
    h.resize(72, 24).unwrap();
    // Do not settle the wheel before the key: the key must take over now,
    // not have its viewport overwritten by a later animation frame.
    h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
    h.send_key(KeyCode::Home, KeyModifiers::NONE).unwrap();
    h.wait_until_stable(|_| true).unwrap();
    h.assert_screen_contains("Row 150");
}

#[test]
fn small_markdown_scroll_preserves_wrap_and_line_number_settings() {
    for threshold in [10 * 1024 * 1024, 0] {
        for wrap in [false, true] {
            for numbers in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("small.md");
                let content = (1..=150)
                    .map(|i| {
                        format!(
                            "Row {i:03} **bold** and `code`, café 日本語 🦀, with {}",
                            "enough prose to cross a screen row. ".repeat(4),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                std::fs::write(&path, content).unwrap();
                let mut config = Config::default();
                config.editor.line_wrap = wrap;
                config.editor.line_numbers = numbers;
                config.editor.large_file_threshold_bytes = threshold;
                let mut h = EditorTestHarness::create(
                    90,
                    28,
                    HarnessOptions::new()
                        .with_config(config)
                        .with_working_dir(temp.path().to_path_buf())
                        .with_full_grammar_registry(),
                )
                .unwrap();
                h.open_file(&path).unwrap();
                h.wait_for_screen_contains(if numbers { "1 │ Row 001" } else { "Row 001" })
                    .unwrap();
                assert_row_labels(&h.screen_to_string(), numbers);
                for _ in 0..15 {
                    h.mouse_scroll_down(60, 12).unwrap();
                }
                assert!(!h.screen_to_string().contains("Row 001"));
                h.wait_until_stable(|_| true).unwrap();
                assert_row_labels(&h.screen_to_string(), numbers);
                h.resize(72, 24).unwrap();
                assert_row_labels(&h.screen_to_string(), numbers);
                h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
                // Home on an unwrapped line restores the horizontal origin.
                // In wrapped mode it moves only within the final visual row.
                if !wrap {
                    h.send_key(KeyCode::Home, KeyModifiers::NONE).unwrap();
                }
                h.wait_for_screen_contains("Row 150").unwrap();
                assert_row_labels(&h.screen_to_string(), numbers);
                h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
                h.type_text("# ").unwrap();
                h.wait_for_screen_contains("# Row 001").unwrap();
                h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
                h.send_key(KeyCode::Delete, KeyModifiers::NONE).unwrap();
                h.send_key(KeyCode::Delete, KeyModifiers::NONE).unwrap();
                h.wait_for_screen_contains("Row 001").unwrap();
                assert_row_labels(&h.screen_to_string(), numbers);
            }
        }
    }
}
