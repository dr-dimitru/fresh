//! Folding in a Markdown buffer with no language server: a heading folds its
//! section, a list item its nested items, and a fenced block its body. The
//! tests fold from the cursor with the palette's "Toggle Fold", the way a
//! user does, and assert on what the pane draws.

use crate::common::harness::{copy_plugin, copy_plugin_lib, EditorTestHarness, HarnessOptions};
use crossterm::event::{KeyCode, KeyModifiers};

/// Line numbers are 1-based, as Go to Line takes them.
const HEADING_LINE: usize = 5;
const LIST_LINE: usize = 9;
const FENCE_LINE: usize = 12;

fn document() -> String {
    [
        "# Guide",
        "",
        "Intro.",
        "",
        "## Setup",
        "",
        "Setup text line.",
        "",
        "- Parent item",
        "  - Child item",
        "",
        "```bash",
        "# not a heading",
        "echo hi",
        "```",
        "",
        "## Usage",
        "",
        "Usage text.",
        "",
    ]
    .join("\n")
}

const PLUGINS: &[&str] = &[
    "markdown_compose",
    "markdown_folding",
    "markdown_source",
    "markdown_toc",
];

fn harness() -> (EditorTestHarness, tempfile::TempDir) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_root = temp_dir.path().join("project");
    let plugins_dir = project_root.join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    for plugin in PLUGINS {
        copy_plugin(&plugins_dir, plugin);
    }
    copy_plugin_lib(&plugins_dir);
    let md_path = project_root.join("guide.md");
    std::fs::write(&md_path, document()).unwrap();

    let mut harness = EditorTestHarness::create(
        100,
        40,
        HarnessOptions::new()
            .with_working_dir(project_root)
            .without_empty_plugins_dir(),
    )
    .unwrap();
    harness.open_file(&md_path).unwrap();
    harness.render().unwrap();
    harness.wait_for_async_quiescence(4).unwrap();
    (harness, temp_dir)
}

fn goto_line(harness: &mut EditorTestHarness, line: usize) {
    harness
        .send_key(KeyCode::Char('g'), KeyModifiers::CONTROL)
        .unwrap();
    harness.wait_for_prompt().unwrap();
    harness.type_text(&line.to_string()).unwrap();
    harness
        .send_key(KeyCode::Enter, KeyModifiers::NONE)
        .unwrap();
    harness.wait_for_prompt_closed().unwrap();
}

fn command(harness: &mut EditorTestHarness, name: &str) {
    harness.run_palette_command(name).unwrap();
    harness.wait_for_prompt_closed().unwrap();
}

fn fold_at(harness: &mut EditorTestHarness, line: usize) {
    goto_line(harness, line);
    command(harness, "Toggle Fold");
    harness.wait_for_async_quiescence(4).unwrap();
    harness.render().unwrap();
}

fn assert_hidden(harness: &EditorTestHarness, hidden: &[&str], shown: &[&str]) {
    let screen = harness.screen_to_string();
    for text in hidden {
        assert!(
            !screen.contains(text),
            "{text:?} should be folded away:\n{screen}"
        );
    }
    for text in shown {
        assert!(
            screen.contains(text),
            "{text:?} should stay visible:\n{screen}"
        );
    }
}

#[test]
fn heading_folds_its_section() {
    let (mut h, _tmp) = harness();
    fold_at(&mut h, HEADING_LINE);
    assert_hidden(
        &h,
        &["Setup text line.", "Parent item", "echo hi"],
        &["## Setup", "## Usage", "Usage text."],
    );
}

#[test]
fn list_item_folds_its_nested_items() {
    let (mut h, _tmp) = harness();
    fold_at(&mut h, LIST_LINE);
    assert_hidden(
        &h,
        &["Child item"],
        &["Parent item", "Setup text line.", "echo hi"],
    );
}

#[test]
fn fenced_block_folds_its_body() {
    let (mut h, _tmp) = harness();
    fold_at(&mut h, FENCE_LINE);
    assert_hidden(
        &h,
        &["echo hi", "# not a heading"],
        &["```bash", "## Usage"],
    );
}

#[test]
fn heading_folds_its_section_in_compose() {
    let (mut h, _tmp) = harness();
    command(&mut h, "Toggle Compose");
    h.wait_for_async_quiescence(4).unwrap();
    fold_at(&mut h, HEADING_LINE);
    assert_hidden(
        &h,
        &["Setup text line.", "Parent item"],
        &["Setup", "Usage text."],
    );
}

#[test]
fn a_heading_typed_after_open_folds_once_the_ranges_refresh() {
    let (mut h, _tmp) = harness();
    let before = h.editor().active_state().folding_ranges.len();
    h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
    h.type_text("## New").unwrap();
    h.send_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    h.send_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    h.type_text("New text.").unwrap();
    // The refresh is debounced after the last edit.
    h.wait_until(|h| h.editor().active_state().folding_ranges.len() == before + 1)
        .unwrap();
    fold_at(&mut h, 20);
    assert_hidden(&h, &["New text."], &["## New", "Usage text."]);
}
