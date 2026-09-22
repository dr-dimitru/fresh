//! E2E tests for setFileExplorerSlots plugin API.

use crate::common::harness::{copy_plugin_lib, EditorTestHarness};
use crossterm::event::{KeyCode, KeyModifiers};
use fresh::config::Config;
use std::fs;

#[test]
fn test_file_explorer_slots_leading_override_renders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_root = temp_dir.path().to_path_buf();
    fs::write(project_root.join("foo.txt"), "hello").unwrap();

    let plugins_dir = project_root.join("plugins");
    fs::create_dir(&plugins_dir).unwrap();
    copy_plugin_lib(&plugins_dir);

    let test_plugin = r###"
/// <reference path="./lib/fresh.d.ts" />
const editor = getEditor();

editor.on("editor_initialized", () => {
  const path = editor.pathJoin(editor.getCwd(), "foo.txt");
  editor.setFileExplorerSlots("test-slots", [{
    path,
    leading: {
      text: "◆",
      color: "ui.syntax.keyword",
      minWidth: 2,
    },
    priority: 99,
  }]);
});
"###;
    fs::write(plugins_dir.join("file_explorer_slots_test.ts"), test_plugin).unwrap();

    let mut config = Config::default();
    config.plugins.insert(
        "file_explorer_slots_test".to_string(),
        fresh_core::config::PluginConfig {
            enabled: true,
            path: Some(plugins_dir.join("file_explorer_slots_test.ts")),
            ..Default::default()
        },
    );

    let mut harness =
        EditorTestHarness::with_config_and_working_dir(120, 40, config, project_root.clone())
            .unwrap();

    // Drain any plugin commands queued during `editor_initialized` (for
    // example `setFileExplorerSlots`) before we open the explorer.
    harness.editor_mut().process_async_messages();
    harness.render().unwrap();

    harness
        .send_key(KeyCode::Char('e'), KeyModifiers::CONTROL)
        .unwrap();
    harness.wait_for_screen_contains("File Explorer").unwrap();
    harness.wait_for_screen_contains("foo.txt").unwrap();
    harness
        .wait_until_stable(|h| {
            h.screen_to_string()
                .lines()
                .any(|line| line.contains("foo.txt") && line.contains('◆'))
        })
        .unwrap();
}

#[test]
fn file_explorer_scope_api_returns_null_while_hidden() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_root = temp_dir.path().to_path_buf();
    fs::write(project_root.join("foo.txt"), "hello").unwrap();

    let plugins_dir = project_root.join("plugins");
    fs::create_dir(&plugins_dir).unwrap();
    copy_plugin_lib(&plugins_dir);

    let test_plugin = r###"
/// <reference path="./lib/fresh.d.ts" />
const editor = getEditor();
const currentScope = () => {
  const scope = editor.getFileExplorerScope();
  return scope === null
    ? "scope:null"
    : `scope:${scope.revision}:${scope.visiblePaths.join("|")}`;
};

editor.on("file_explorer_scope_changed", () => {
  editor.setStatus(currentScope());
});
editor.on("editor_initialized", () => {
  editor.setStatus(`initial:${currentScope()}`);
});
"###;
    fs::write(plugins_dir.join("file_explorer_scope_test.ts"), test_plugin).unwrap();

    let mut config = Config::default();
    config.plugins.insert(
        "file_explorer_scope_test".to_string(),
        fresh_core::config::PluginConfig {
            enabled: true,
            path: Some(plugins_dir.join("file_explorer_scope_test.ts")),
            ..Default::default()
        },
    );

    let mut harness =
        EditorTestHarness::with_config_and_working_dir(120, 40, config, project_root).unwrap();
    harness.wait_for_screen_contains("initial:scope:null").unwrap();
}

#[test]
fn file_explorer_scope_hook_does_not_repeat_for_render_or_decorations() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_root = temp_dir.path().to_path_buf();
    let data_dir = tempfile::TempDir::new().unwrap();
    let _data_dir_pin = crate::common::global_state::pin_data_dir(data_dir.path());
    fs::write(project_root.join("foo.txt"), "hello").unwrap();

    let plugins_dir = project_root.join("plugins");
    fs::create_dir(&plugins_dir).unwrap();
    copy_plugin_lib(&plugins_dir);
    let plugin_path = plugins_dir.join("file_explorer_scope_probe.ts");
    fs::write(
        &plugin_path,
        r###"
/// <reference path="./lib/fresh.d.ts" />
const editor = getEditor();
let scopeEvents = 0;
editor.on("file_explorer_scope_changed", () => {
  scopeEvents += 1;
  const scope = editor.getFileExplorerScope();
  editor.setStatus(`scope-events=${scopeEvents};revision=${scope?.revision ?? "null"}`);
  editor.setFileExplorerDecorations("scope-probe", []);
  editor.setFileExplorerSlots("scope-probe", []);
});
"###,
    )
    .unwrap();

    let mut config = Config::default();
    config.plugins.insert(
        "file_explorer_scope_probe".to_string(),
        fresh_core::config::PluginConfig {
            enabled: true,
            path: Some(plugin_path),
            ..Default::default()
        },
    );
    let mut harness =
        EditorTestHarness::with_config_and_working_dir(120, 40, config, project_root).unwrap();
    harness.editor_mut().focus_file_explorer();
    harness.wait_for_file_explorer().unwrap();
    let mut frames = 0;
    harness
        .wait_until(|h| {
            frames += 1;
            h.screen_to_string().contains("scope-events=") || frames >= 10
        })
        .unwrap();
    let screen_after_scope = harness.screen_to_string();
    let marker = screen_after_scope
        .split_whitespace()
        .find(|token| token.starts_with("scope-events="))
        .expect("visible Explorer should emit scope change hook")
        .to_string();

    harness.render().unwrap();
    harness.render().unwrap();
    harness
        .wait_until_stable(|h| h.screen_to_string().contains(&marker))
        .unwrap();
    assert_eq!(
        harness
            .screen_to_string()
            .split_whitespace()
            .find(|token| token.starts_with("scope-events=")),
        Some(marker.as_str())
    );
}
