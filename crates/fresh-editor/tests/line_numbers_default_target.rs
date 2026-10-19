//! A plugin's line-number default aimed at one pane must not reach a buffer
//! that pane is not showing. Observed on screen only.
#![cfg(feature = "plugins")]

use crate::common::harness::{EditorTestHarness, HarnessOptions};
use fresh::config::Config;

#[test]
fn default_for_a_pane_not_showing_the_buffer_is_ignored() {
    let temp = tempfile::TempDir::new().unwrap();
    let plugins = temp.path().join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    std::fs::write(
        plugins.join("pin_other.ts"),
        r#"
const editor = getEditor();
registerHandler("pinOther", () => {
  const buffer = editor.getActiveBufferId();
  const other = editor.listSplits().find((s) => s.bufferId !== buffer);
  editor.setLineNumbersDefault(buffer, false, other.splitId);
  editor.setStatus("pinned for another pane");
});
editor.registerCommand("Pin other pane", "Aim a default at a pane", "pinOther", null);
"#,
    )
    .unwrap();
    let a = temp.path().join("a.txt");
    let b = temp.path().join("b.txt");
    std::fs::write(&a, "alpha one\nalpha two\n").unwrap();
    std::fs::write(&b, "beta one\nbeta two\n").unwrap();
    let mut h = EditorTestHarness::create(
        120,
        20,
        HarnessOptions::new()
            .with_config(Config::default())
            .with_working_dir(temp.path().to_path_buf())
            .without_empty_plugins_dir(),
    )
    .unwrap();
    h.open_file(&a).unwrap();
    h.run_palette_command("Split Vertical").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.open_file(&b).unwrap();
    h.run_palette_command("Pin other pane").unwrap();
    h.wait_for_screen_contains("pinned for another pane").unwrap();

    // Show b.txt in the pane the default was aimed at.
    h.mouse_click(10, 3).unwrap();
    h.open_file(&b).unwrap();
    h.wait_until_stable(|h| h.screen_to_string().matches("beta one").count() == 2)
        .unwrap();
    let screen = h.screen_to_string();
    // The pane separator's column, read from the tab bar.
    let sep = screen.lines().nth(1).unwrap().chars().position(|c| c == '│').unwrap();
    let left: String = screen
        .lines()
        .find(|r| r.chars().take(sep).collect::<String>().contains("beta one"))
        .unwrap()
        .chars()
        .take(sep)
        .collect();
    assert!(
        left.trim_start().starts_with("1 "),
        "b.txt opened in that pane keeps the global line numbers:\n{screen}"
    );
}
