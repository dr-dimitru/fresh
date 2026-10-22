//! Screen-level contract for the opt-in Markdown Compose prose budget.
#![cfg(feature = "plugins")]

use crate::common::harness::{copy_plugin, copy_plugin_lib, EditorTestHarness, HarnessOptions};
use crossterm::event::{KeyCode, KeyModifiers};
use fresh::config::{Config, PluginConfig};

fn composed(text: &str, limit: Option<usize>) -> (EditorTestHarness, tempfile::TempDir) {
    composed_at(text, limit, 100, 24)
}

fn composed_at(
    text: &str,
    limit: Option<usize>,
    width: u16,
    height: u16,
) -> (EditorTestHarness, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    copy_plugin(&plugins, "markdown_compose");
    copy_plugin_lib(&plugins);
    std::fs::write(plugins.join("budget_test_controls.ts"), r#"
const editor = getEditor();
registerHandler("budget_test_reload", () => { editor.reloadConfig(); });
editor.registerCommand("Budget Test Reload", "Reload isolated test config", "budget_test_reload", null);
"#).unwrap();
    let path = temp.path().join("budget.md");
    std::fs::write(&path, text).unwrap();
    let mut config = Config {
        lsp_enabled: false,
        ..Config::default()
    };
    config.editor.smooth_scroll = false;
    config.plugins.insert(
        "markdown_compose".into(),
        PluginConfig {
            enabled: true,
            path: None,
            settings: limit.map_or_else(
                || serde_json::json!({}),
                |value| serde_json::json!({"maxProseReflowCodeUnits": value}),
            ),
        },
    );
    let mut h = EditorTestHarness::create(
        width,
        height,
        HarnessOptions::new()
            .with_config(config)
            .with_working_dir(temp.path().to_path_buf())
            .without_empty_plugins_dir()
            .with_full_grammar_registry(),
    )
    .unwrap();
    h.editor_mut().set_clipboard_for_test(String::new());
    h.open_file(&path).unwrap();
    h.run_palette_command("Toggle Compose").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    (h, temp)
}

fn row(h: &EditorTestHarness, needle: &str) -> String {
    let screen = h.screen_to_string();
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("Missing {needle:?}:\n{screen}"))
        .to_owned()
}

const TWO_LINES: &str = "Home.\n\nAlpha\nBravo\n\n**done**.\n";

#[test]
fn finite_budget_preserves_source_line_boundaries() {
    let (h, _temp) = composed(TWO_LINES, Some(8));
    assert!(
        !row(&h, "Alpha").contains("Bravo"),
        "{}",
        h.screen_to_string()
    );
    assert!(row(&h, "Bravo").contains("Bravo"));
}

#[test]
fn omitted_and_zero_keep_unlimited_reflow() {
    for limit in [None, Some(0)] {
        let (h, _temp) = composed(TWO_LINES, limit);
        assert!(
            row(&h, "Alpha").contains("Alpha Bravo"),
            "{}",
            h.screen_to_string()
        );
    }
}

fn goto(h: &mut EditorTestHarness, line: usize) {
    h.send_key(KeyCode::Char('g'), KeyModifiers::CONTROL)
        .unwrap();
    h.wait_for_prompt().unwrap();
    h.type_text(&line.to_string()).unwrap();
    h.send_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_async_quiescence(4).unwrap();
}

#[test]
fn unicode_and_crlf_use_exact_source_code_unit_thresholds() {
    for ending in ["\n", "\r\n"] {
        for (first, second, units) in [("🦀", "é", 3), ("日本", "語", 3), ("e\u{301}", "x", 3)]
        {
            let text = ["Home.", "", first, second, "", "**done**."].join(ending);
            for (limit, joined) in [(units, true), (units - 1, false)] {
                let (h, _temp) = composed(&text, Some(limit));
                // TestBackend materializes a spacer cell after wide glyphs.
                // Row membership, not string adjacency, observes source joins.
                let first_glyph = if first == "日本" { "日" } else { first };
                assert_eq!(
                    row(&h, first_glyph).contains(second),
                    joined,
                    "limit={limit}\n{}",
                    h.screen_to_string()
                );
            }
        }
    }
}

#[test]
fn edits_grow_and_shrink_across_the_budget() {
    let (mut h, _temp) = composed(TWO_LINES, Some(10));
    assert!(row(&h, "Alpha").contains("Bravo"));
    goto(&mut h, 3);
    h.send_key(KeyCode::Home, KeyModifiers::NONE).unwrap();
    h.type_text("!").unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(
        !row(&h, "Alpha").contains("Bravo"),
        "{}",
        h.screen_to_string()
    );
    h.send_key(KeyCode::Backspace, KeyModifiers::NONE).unwrap();
    h.wait_until_stable(|h| row(h, "Alpha").contains("Bravo"))
        .unwrap();
}

#[test]
fn fallback_preserves_list_and_quote_source_rows() {
    let text = "Home.\n\n1. Alpha\n   Bravo\n\n> Quoted\n> continuation\n\n> > Nested\n> > quotation\n\n**done**.\n";
    let (h, _temp) = composed(text, Some(1));
    assert!(!row(&h, "Alpha").contains("Bravo"));
    assert!(!row(&h, "Quoted").contains("continuation"));
    assert!(row(&h, "Quoted").contains('▌'));
    assert!(row(&h, "continuation").contains('▌'));
    assert_eq!(row(&h, "quotation").matches('▌').count(), 2);
}

#[test]
fn tiny_budget_does_not_bypass_code_frames_or_table_layout() {
    let text = format!("Home.\n\n```js\n{}\n```\n\n| Header | Other |\n| --- | --- |\n| {} | cell |\n\n**done**.\n", "word ".repeat(25), "table ".repeat(18));
    let (h, _temp) = composed_at(&text, Some(1), 76, 36);
    h.assert_screen_contains("┌");
    h.assert_screen_contains("└");
    h.assert_screen_contains("Header");
    // The narrow second column legitimately wraps `cell` as `cel` / `l`.
    h.assert_screen_contains("cel");
    assert!(
        !h.screen_to_string().contains("```"),
        "{}",
        h.screen_to_string()
    );
    assert!(row(&h, "Header").contains('│'));
}

#[test]
fn native_wrapping_keeps_long_single_line_eof_visible_and_editable_with_numbers_on_or_off() {
    for numbers in [false, true] {
        let text = format!("Home.\n\n{}TAIL", "café 日本語 🦀 words ".repeat(100));
        let (mut h, _temp) = composed_at(&text, Some(32), 68, 20);
        if numbers {
            h.run_palette_command("Toggle Line Numbers (Current Buffer)")
                .unwrap();
            h.wait_for_prompt_closed().unwrap();
        }
        h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
        h.wait_for_async_quiescence(4).unwrap();
        h.assert_screen_contains("TAIL");
        assert!(
            h.render_observing_cursor().unwrap().is_some(),
            "{}",
            h.screen_to_string()
        );
        h.type_text("!").unwrap();
        h.assert_screen_contains("TAIL!");
        h.resize(54, 18).unwrap();
        h.wait_for_async_quiescence(4).unwrap();
        h.assert_screen_contains("TAIL!");
    }
}

#[test]
fn an_incomplete_long_run_falls_back_even_when_its_fragment_is_below_budget() {
    let text = format!("Home.\n\n{}\n\n**done**.\n", "Alpha\nBravo\n".repeat(500));
    let (mut h, _temp) = composed(&text, Some(100000));
    assert!(
        !row(&h, "Alpha").contains("Bravo"),
        "{}",
        h.screen_to_string()
    );
    for _ in 0..6 {
        h.mouse_scroll_down(50, 10).unwrap();
    }
    h.wait_for_async_quiescence(4).unwrap();
    assert!(
        !row(&h, "Alpha").contains("Bravo"),
        "{}",
        h.screen_to_string()
    );
}

fn reload_limit(h: &mut EditorTestHarness, root: &std::path::Path, limit: usize) {
    let dir = root.join(".fresh");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.json"),
        serde_json::to_vec(&serde_json::json!({
            "check_for_updates": false,
            "lsp_enabled": false,
            "editor": { "smooth_scroll": false, "animations": false },
            "plugins": { "markdown_compose": { "settings": { "maxProseReflowCodeUnits": limit } } }
        }))
        .unwrap(),
    )
    .unwrap();
    h.run_palette_command("Budget Test Reload").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_async_quiescence(4).unwrap();
}

#[test]
fn reload_rebuilds_visited_offscreen_prose_in_both_directions() {
    let filler = "Filler.\n\n".repeat(50);
    let text = format!("Home.\n\nAlpha\nBravo\n\n{filler}Tail\nending\n\n**done**.\n");
    let (mut h, temp) = composed(&text, Some(0));
    assert!(row(&h, "Alpha").contains("Bravo"));
    h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(row(&h, "Tail").contains("ending"));
    reload_limit(&mut h, temp.path(), 1);
    assert!(
        !row(&h, "Tail").contains("ending"),
        "{}",
        h.screen_to_string()
    );
    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(
        !row(&h, "Alpha").contains("Bravo"),
        "{}",
        h.screen_to_string()
    );
    reload_limit(&mut h, temp.path(), 0);
    assert!(
        row(&h, "Alpha").contains("Bravo"),
        "{}",
        h.screen_to_string()
    );
    h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(
        row(&h, "Tail").contains("ending"),
        "{}",
        h.screen_to_string()
    );
}

#[test]
fn fallback_is_consistent_in_two_compose_panes_with_different_number_pins() {
    let (mut h, _temp) = composed(TWO_LINES, Some(8));
    h.run_palette_command("Split Vertical").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.run_palette_command("Toggle Line Numbers (Current Buffer)")
        .unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    // Drag the rendered divider to give the two Compose panes unequal widths.
    let divider = row(&h, "Alpha").chars().position(|ch| ch == '│').unwrap();
    h.mouse_drag(divider as u16, 10, 68, 10).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let screen = h.screen_to_string();
    let alpha = row(&h, "Alpha");
    assert_ne!(
        alpha.chars().position(|ch| ch == '│').unwrap(),
        divider,
        "{screen}"
    );
    assert_eq!(alpha.matches("Alpha").count(), 2, "{screen}");
    assert!(!alpha.contains("Bravo"), "{screen}");
    assert_eq!(row(&h, "Bravo").matches("Bravo").count(), 2, "{screen}");
}

#[test]
fn unlimited_defaults_still_reflow_blocks_larger_than_the_local_512_cutoff() {
    let text = format!("Home.\n\n{}HEAD\nTAIL\n\n**done**.\n", "word ".repeat(120));
    for limit in [None, Some(0), Some(512)] {
        let (h, _temp) = composed(&text, limit);
        assert_eq!(
            row(&h, "HEAD").contains("TAIL"),
            limit != Some(512),
            "{limit:?}\n{}",
            h.screen_to_string()
        );
    }
}

#[test]
fn prose_policy_keeps_existing_code_and_table_rendering_with_number_pins() {
    let text = "Home.\n\n```js\nconst answer = 42;\n```\n\n| Name | Value |\n| --- | --- |\n| entry | data |\n\n**done**.\n";
    for numbers in [false, true] {
        let render = |limit| {
            let (mut h, _temp) = composed_at(text, Some(limit), 76, 30);
            if numbers {
                h.run_palette_command("Toggle Line Numbers (Current Buffer)")
                    .unwrap();
                h.wait_for_prompt_closed().unwrap();
                h.wait_for_async_quiescence(4).unwrap();
            }
            h.screen_to_string()
        };
        assert_eq!(
            render(0),
            render(1),
            "prose policy must not change specialized layout"
        );
    }
}

/// Performance invariant, deliberately separate from screen assertions: inspect
/// counted renderer work, never elapsed time, with fixed viewport/scroll input.
#[test]
fn finite_fallback_scrolling_does_not_rebuild_document_sized_wrap_indices() {
    for paragraphs in [120, 1200] {
        let text = format!(
            "Home.\n\n{}",
            (0..paragraphs)
                .map(|n| { format!("Block{n} **bold** {}\n\n", "prose words ".repeat(70)) })
                .collect::<String>()
        );
        let (mut h, _temp) = composed(&text, Some(32));
        let mut pass_work = Vec::new();
        for _ in 0..2 {
            h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
            h.editor_mut().sync_plugin_runtime();
            h.tick_and_render().unwrap();
            let before = h.editor().active_state().wrap_indices.stats();
            for _ in 0..20 {
                h.mouse_scroll_down(50, 10).unwrap();
                while h.editor_mut().sync_plugin_runtime() {
                    h.tick_and_render().unwrap();
                }
                h.tick_and_render().unwrap();
            }
            let after = h.editor().active_state().wrap_indices.stats();
            assert_eq!(
                after.lines_built - before.lines_built,
                0,
                "no document-sized rebuild on scroll"
            );
            pass_work.push(after.lines_repaired - before.lines_repaired);
        }
        assert!(
            pass_work[0] <= 20 * 35,
            "work bounded by viewport/steps, not {paragraphs} paragraphs: {pass_work:?}"
        );
        assert_eq!(
            pass_work[1], 0,
            "warm scrolling must reuse decorations: {pass_work:?}"
        );
    }
}

#[test]
fn a_truncated_single_line_uses_native_wrap_even_below_its_configured_budget() {
    let text = format!("Home.\n\n> OPEN {}\n", "words ".repeat(12000));
    let (h, _temp) = composed(&text, Some(200000));
    let screen = h.screen_to_string();
    assert!(
        screen.lines().filter(|line| line.contains("words")).count() > 1,
        "native wrapping stays enabled\n{screen}"
    );
    assert_eq!(
        screen.matches('▌').count(),
        1,
        "fallback has the source quote bar, not custom continuation rails\n{screen}"
    );
}

#[test]
fn finite_policy_keeps_source_and_compose_panes_independent() {
    let (mut h, _temp) = composed(TWO_LINES, Some(8));
    h.run_palette_command("Split Vertical").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.run_palette_command("Markdown: Source Mode").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let screen = h.screen_to_string();
    let done = row(&h, "done");
    assert_eq!(done.matches("done").count(), 2, "{screen}");
    assert_eq!(
        done.matches("**done**").count(),
        1,
        "only Source reveals markup\n{screen}"
    );
    assert_eq!(row(&h, "Alpha").matches("Alpha").count(), 2, "{screen}");
    assert!(!row(&h, "Alpha").contains("Bravo"), "{screen}");
}

#[test]
fn a_large_paste_into_a_continuation_retires_the_old_join_before_native_fallback() {
    let (mut h, _temp) = composed(TWO_LINES, Some(64));
    assert!(row(&h, "Alpha").contains("Bravo"));
    goto(&mut h, 4);
    h.send_paste(&"INSERTED ".repeat(3000)).unwrap();
    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(
        !row(&h, "Alpha").contains("INSERTED"),
        "{}",
        h.screen_to_string()
    );
    h.assert_screen_contains("INSERTED");
    h.send_key(KeyCode::Char('z'), KeyModifiers::CONTROL)
        .unwrap();
    h.wait_until_stable(|h| {
        h.screen_to_string()
            .lines()
            .any(|line| line.contains("Alpha Bravo"))
    })
    .unwrap();
    assert!(!h.screen_to_string().contains("INSERTED"));
}

#[test]
fn a_large_paste_at_the_block_head_removes_offscreen_joins() {
    let (mut h, _temp) = composed(TWO_LINES, Some(64));
    goto(&mut h, 3);
    h.send_paste(&"INSERTED ".repeat(3000)).unwrap();
    h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(
        !row(&h, "Alpha").contains("Bravo"),
        "oversized block kept an old join:\n{}",
        h.screen_to_string()
    );
    h.assert_screen_contains("Bravo");
}

#[test]
fn merging_and_undoing_two_eligible_blocks_retires_and_restores_both_joins() {
    let text = "# Home\n\nAlpha\nBravo\n\nCharlie\nDelta\n\n**done**\n";
    let (mut h, _temp) = composed(text, Some(12));
    assert!(row(&h, "Alpha").contains("Bravo"));
    assert!(row(&h, "Charlie").contains("Delta"));
    goto(&mut h, 5);
    h.send_key(KeyCode::Delete, KeyModifiers::NONE).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(!row(&h, "Alpha").contains("Bravo"));
    assert!(!row(&h, "Charlie").contains("Delta"));
    h.send_key(KeyCode::Char('z'), KeyModifiers::CONTROL)
        .unwrap();
    h.wait_until_stable(|h| {
        let screen = h.screen_to_string();
        screen.lines().any(|line| line.contains("Alpha Bravo"))
            && screen.lines().any(|line| line.contains("Charlie Delta"))
    })
    .unwrap();
}
