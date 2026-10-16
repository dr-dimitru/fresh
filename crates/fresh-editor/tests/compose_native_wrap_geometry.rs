//! Compose's native wrapping must use the same page and gutter geometry for
//! scrolling and drawing, even when a plugin emits no explicit soft breaks.

use crate::common::harness::{copy_plugin_lib, EditorTestHarness};
use crossterm::event::{KeyCode, KeyModifiers};
use fresh::config::Config;

fn setup(show_scrollbar: bool) -> (EditorTestHarness, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    copy_plugin_lib(&plugins);
    std::fs::write(
        plugins.join("native_compose.ts"),
        r#"
const editor = getEditor();
registerHandler("native_compose", () => {
    const id = editor.getActiveBufferId();
    editor.setViewMode(id, "compose");
    editor.setLineNumbersDefault(id, true);
    editor.setLineWrap(id, editor.getActiveSplitId(), true);
    editor.setLayoutHints(id, null, { composeWidth: 80 });
    editor.setStatus("native-compose-ready");
});
editor.registerCommand("Native Compose", "Compose with native wrapping", "native_compose", null);
"#,
    )
    .unwrap();
    let text = (1..=150)
        .map(|i| {
            format!(
                "Row {i:03} café 日本語 🦀 {}",
                "words across wrapped rows. ".repeat(5)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let file = temp.path().join("page.md");
    std::fs::write(&file, text).unwrap();
    let mut config = Config::default();
    config.editor.line_numbers = true;
    config.editor.line_wrap = true;
    config.editor.smooth_scroll = false;
    config.editor.show_vertical_scrollbar = show_scrollbar;
    let mut h =
        EditorTestHarness::with_config_and_working_dir(110, 32, config, temp.path().to_path_buf())
            .unwrap();
    h.open_file(&file).unwrap();
    h.run_palette_command("Native Compose").unwrap();
    h.wait_for_screen_contains("native-compose-ready").unwrap();
    (h, temp)
}

#[test]
fn compose_native_wrapping_keeps_eof_visible() {
    let (mut h, _temp) = setup(false);
    h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
    h.wait_until_stable(|_| true).unwrap();
    h.assert_screen_contains("Row 150");
    assert!(
        h.render_observing_cursor().unwrap().is_some(),
        "EOF caret must be visible"
    );
}

#[test]
fn compose_retains_requested_line_numbers_in_a_narrow_window() {
    let (mut h, _temp) = setup(false);
    h.assert_screen_contains("1 │ Row 001");
    h.resize(76, 26).unwrap();
    h.wait_until_stable(|_| true).unwrap();
    h.assert_screen_contains("1 │ Row 001");
    h.send_key(KeyCode::End, KeyModifiers::CONTROL).unwrap();
    h.wait_until_stable(|_| true).unwrap();
    h.assert_screen_contains("150 │ Row 150");
}

#[test]
fn compose_native_wheel_reaches_eof_with_a_scrollbar() {
    let (mut h, _temp) = setup(true);
    for _ in 0..180 {
        h.mouse_scroll_down(50, 12).unwrap();
    }
    h.wait_until_stable(|_| true).unwrap();
    h.assert_screen_contains("150 │ Row 150");
    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
    h.assert_screen_contains("1 │ Row 001");
}
