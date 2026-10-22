//! Internal pane-decoration isolation contracts. Visible flows live in the
//! Compose screen suite; these tests cover ownership and cache work directly.
use fresh::model::marker::MarkerList;
use fresh::view::conceal::ConcealManager;
use fresh::view::pane_decorations::DecorationScope;
use fresh::view::soft_break::SoftBreakManager;
use fresh::view::virtual_text::{VirtualTextManager, VirtualTextPosition};
use fresh_core::overlay::OverlayNamespace;
use fresh_core::SplitId;
use ratatui::style::Style;

fn pane(id: usize) -> DecorationScope {
    DecorationScope::Pane {
        split_id: SplitId(id),
        plugin_name: "compose".into(),
    }
}

#[test]
fn pane_conceal_clear_preserves_sibling_and_buffer_scope() {
    let mut markers = MarkerList::new();
    let mut manager = ConcealManager::new();
    for scope in [DecorationScope::Buffer, pane(1), pane(2)] {
        manager.set_scope(scope);
        manager.add(
            &mut markers,
            OverlayNamespace::from_string("same".into()),
            1..4,
            None,
        );
    }
    let before = manager.version_for(Some(SplitId(2)));
    manager.set_scope(pane(1));
    manager.remove_in_range(&(0..5), &mut markers);
    assert_eq!(
        manager
            .query_viewport_for_pane(0, 5, &markers, true, &[], Some(SplitId(1)))
            .len(),
        1
    );
    assert_eq!(
        manager
            .query_viewport_for_pane(0, 5, &markers, true, &[], Some(SplitId(2)))
            .len(),
        2
    );
    assert_eq!(manager.version_for(Some(SplitId(2))), before);
    manager.set_scope(pane(2));
    manager.clear(&mut markers);
    assert_eq!(manager.query_viewport(0, 5, &markers, &[]).len(), 1);
}

#[test]
fn pane_soft_break_clear_preserves_sibling_and_buffer_scope() {
    let mut markers = MarkerList::new();
    let mut manager = SoftBreakManager::new();
    for scope in [DecorationScope::Buffer, pane(1), pane(2)] {
        manager.set_scope(scope);
        manager.add(
            &mut markers,
            OverlayNamespace::from_string("same".into()),
            2,
            0,
        );
    }
    manager.set_scope(pane(1));
    manager.clear_namespace(&OverlayNamespace::from_string("same".into()), &mut markers);
    assert_eq!(
        manager
            .query_viewport_for_pane(0, 5, &markers, &[], Some(SplitId(1)))
            .len(),
        1
    );
    assert_eq!(
        manager
            .query_viewport_for_pane(0, 5, &markers, &[], Some(SplitId(2)))
            .len(),
        2
    );
}

#[test]
fn pane_inline_id_and_prefix_removal_preserve_sibling_and_buffer_scope() {
    let mut markers = MarkerList::new();
    let mut manager = VirtualTextManager::new();
    for scope in [DecorationScope::Buffer, pane(1), pane(2)] {
        manager.set_scope(scope);
        manager.add_with_id(
            &mut markers,
            2,
            "rail".into(),
            Style::default(),
            VirtualTextPosition::BeforeChar,
            0,
            "same-rail".into(),
        );
    }
    manager.set_scope(pane(1));
    assert!(manager.remove_by_id(&mut markers, "same-rail"));
    assert_eq!(
        manager
            .query_inline_for_pane(&markers, 0, 5, Some(SplitId(2)))
            .len(),
        2
    );
    manager.add_with_id(
        &mut markers,
        3,
        "own".into(),
        Style::default(),
        VirtualTextPosition::BeforeChar,
        0,
        "same-second".into(),
    );
    manager.remove_by_prefix(&mut markers, "same");
    assert_eq!(
        manager
            .query_inline_for_pane(&markers, 0, 5, Some(SplitId(1)))
            .len(),
        1,
        "prefix removal must actually delete the owning pane's matching entry"
    );
    assert_eq!(
        manager
            .query_inline_for_pane(&markers, 0, 5, Some(SplitId(2)))
            .len(),
        2
    );
}

#[test]
fn pane_overlay_clear_preserves_sibling_and_buffer_scope() {
    use fresh::view::overlay::{Overlay, OverlayFace, OverlayManager};
    let mut markers = MarkerList::new();
    let mut manager = OverlayManager::new();
    for scope in [DecorationScope::Buffer, pane(1), pane(2)] {
        manager.set_scope(scope);
        manager.add(Overlay::with_namespace(
            &mut markers,
            1..4,
            OverlayFace::Style {
                style: Style::default(),
            },
            OverlayNamespace::from_string("same".into()),
        ));
    }
    manager.set_scope(pane(1));
    manager.clear_namespace(&OverlayNamespace::from_string("same".into()), &mut markers);
    assert_eq!(
        manager
            .query_viewport_for_pane(0, 5, &markers, Some(SplitId(1)))
            .len(),
        1
    );
    assert_eq!(
        manager
            .query_viewport_for_pane(0, 5, &markers, Some(SplitId(2)))
            .len(),
        2
    );
}

#[cfg(feature = "plugins")]
fn pane_api_harness() -> (crate::common::harness::EditorTestHarness, tempfile::TempDir) {
    use crate::common::harness::{EditorTestHarness, HarnessOptions};
    let temp = tempfile::TempDir::new().unwrap();
    let plugins = temp.path().join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    std::fs::write(plugins.join("measure.ts"), r#"
const editor = getEditor();
editor.on("lines_changed", data => {
  for (const line of data.lines) {
    if (line.content.startsWith("LEGACY")) {
      editor.addConceal(data.buffer_id, "measure", line.byte_start, line.byte_start + 6, "BUFFER");
    }
  }
});
registerHandler("measureLegacyClear", () => editor.clearConcealNamespace(editor.getActiveBufferId(), "measure"));
editor.registerCommand("Measure legacy clear", "Clear only legacy buffer decorations", "measureLegacyClear", null);
editor.on("pane_lines_changed", data => {
  for (const line of data.lines) {
    if (line.content.startsWith("IDENTITY")) {
      editor.clearConcealsInRange(data.buffer_id, line.byte_start, line.byte_end);
      editor.addConceal(data.buffer_id, "measure", line.byte_start, line.byte_start + 8,
        `W${data.window_id} S${data.split_id} B${data.buffer_id} G${data.layout_generation}`);
      continue;
    }
    if (line.content.startsWith("CHANGED")) {
      editor.clearConcealsInRange(data.buffer_id, line.byte_start, line.byte_end);
      editor.addConceal(data.buffer_id, "measure", line.byte_start, line.byte_start + 7, "REPLACED");
      continue;
    }
    if (!line.content.startsWith("MEASURE")) continue;
    editor.clearConcealsInRange(data.buffer_id, line.byte_start, line.byte_end);
    editor.addConceal(data.buffer_id, "measure", line.byte_start, line.byte_start + 7,
      `text=${data.layout.textWidth},gutter=${data.layout.gutterWidth}`);
  }
});
registerHandler("measureReady", () => editor.setStatus("measure plugin ready"));
editor.registerCommand("Measure ready", "Synchronize the measurement test", "measureReady", null);
let measuredVirtual = null;
registerHandler("measureVirtualCreate", async () => {
  const result = await editor.createVirtualBuffer({ name: "*Measured*", showLineNumbers: true,
    entries: [{ text: "MEASURE\nIDENTITY\n", properties: {} }] });
  measuredVirtual = result.bufferId;
});
editor.registerCommand("Measure virtual create", "Create isolated measured panel", "measureVirtualCreate", null);
registerHandler("measureVirtualReplace", () => {
  editor.setVirtualBufferContent(measuredVirtual, [{ text: "CHANGED\nIDENTITY\n", properties: {} }]);
  editor.setStatus("OK");
});
editor.registerCommand("Measure virtual replace", "Replace measured panel content", "measureVirtualReplace", null);
registerHandler("measureLayoutOutside", () => {
  editor.insertAtCursor("outside=" + editor.replaceTextLayoutRanges(editor.getActiveBufferId(), "flow", 1, [], []));
});
editor.registerCommand("Measure layout outside", "Check explicit pane-only layout ownership", "measureLayoutOutside", null);
"#).unwrap();
    let path = temp.path().join("measure.txt");
    std::fs::write(&path, "MEASURE\nIDENTITY\nLEGACY\n").unwrap();
    let mut h = EditorTestHarness::create(
        44,
        20,
        HarnessOptions::new()
            .with_working_dir(temp.path().to_path_buf())
            .without_empty_plugins_dir(),
    )
    .unwrap();
    h.editor_mut().set_clipboard_for_test(String::new());
    h.open_file(&path).unwrap();
    h.run_palette_command("Measure ready").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    (h, temp)
}

#[cfg(feature = "plugins")]
#[test]
fn pane_batch_reports_the_drawn_text_and_gutter() {
    let (h, _temp) = pane_api_harness();
    let screen = h.screen_to_string();
    // 44 terminal cells minus one scrollbar, minus the six-cell numbered gutter.
    assert!(screen.contains("text=37,gutter=6"), "{screen}");
}

#[test]
fn themed_inline_add_invalidates_only_its_pane_version() {
    let mut markers = MarkerList::new();
    let mut manager = VirtualTextManager::new();
    manager.set_scope(pane(1));
    let before = manager.version_for(Some(SplitId(1)));
    manager.add_with_id_and_theme_keys(
        &mut markers,
        2,
        "rail".into(),
        Style::default(),
        None,
        None,
        VirtualTextPosition::BeforeChar,
        0,
        "rail".into(),
        Some(30),
        true,
    );
    assert_ne!(manager.version_for(Some(SplitId(1))), before);
    assert_eq!(manager.version_for(Some(SplitId(2))), 0);
}

#[cfg(feature = "plugins")]
#[test]
fn stale_pane_clear_cannot_erase_resized_projection() {
    use fresh_core::api::{PaneDecorationContext, PluginCommand};
    let (mut h, _temp) = pane_api_harness();
    // Read the hook's identity from its rendered output, not editor state.
    let identity = |screen: &str, prefix: char| {
        screen
            .split_whitespace()
            .find_map(|word| {
                word.strip_prefix(prefix)
                    .and_then(|n| n.parse::<u64>().ok())
            })
            .unwrap()
    };
    let screen = h.screen_to_string();
    let context = PaneDecorationContext {
        window_id: identity(&screen, 'W'),
        split_id: SplitId(identity(&screen, 'S') as usize),
        buffer_id: fresh_core::BufferId(identity(&screen, 'B') as usize),
        layout_generation: identity(&screen, 'G'),
        plugin_name: "measure".into(),
    };
    h.resize(70, 20).unwrap();
    h.wait_until_stable(|h| h.screen_to_string().contains("gutter=6"))
        .unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert_ne!(
        identity(&h.screen_to_string(), 'G'),
        context.layout_generation
    );
    // Inject an obsolete internal reply at the dispatcher boundary. This is an
    // API contract test, not a simulated user command or a timing-based race.
    let buffer_id = context.buffer_id;
    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context,
            command: Box::new(PluginCommand::ClearConcealNamespace {
                buffer_id,
                namespace: OverlayNamespace::from_string("measure".into()),
            }),
        })
        .unwrap();
    h.render().unwrap();
    assert!(
        h.screen_to_string().contains("text=63,gutter=6"),
        "obsolete clear removed current projection:\n{}",
        h.screen_to_string()
    );
    let screen = h.screen_to_string();
    let wrong_window = PaneDecorationContext {
        window_id: identity(&screen, 'W') + 1000,
        split_id: SplitId(identity(&screen, 'S') as usize),
        buffer_id,
        layout_generation: identity(&screen, 'G'),
        plugin_name: "measure".into(),
    };
    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context: wrong_window,
            command: Box::new(PluginCommand::ClearConcealNamespace {
                buffer_id,
                namespace: OverlayNamespace::from_string("measure".into()),
            }),
        })
        .unwrap();
    h.render().unwrap();
    assert!(
        h.screen_to_string().contains("text=63,gutter=6"),
        "reply from another window erased this pane's projection:\n{}",
        h.screen_to_string()
    );
}

#[cfg(feature = "plugins")]
#[test]
fn virtual_content_replacement_reoffers_unchanged_byte_ranges() {
    let (mut h, _temp) = pane_api_harness();
    h.resize(100, 20).unwrap();
    h.run_palette_command("Measure virtual create").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_screen_contains("*Measured*").unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(h.screen_to_string().contains("text="));
    let generation = |screen: &str| {
        screen
            .split_whitespace()
            .find_map(|word| word.strip_prefix('G').and_then(|n| n.parse::<u64>().ok()))
            .unwrap()
    };
    let before = generation(&h.screen_to_string());
    h.run_palette_command("Measure virtual replace").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_screen_contains("OK").unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(
        h.screen_to_string().contains("REPLACED"),
        "new content at identical byte ranges must get a fresh pane batch:\n{}",
        h.screen_to_string()
    );
    assert_ne!(
        generation(&h.screen_to_string()),
        before,
        "replacement must retire the old generation, not just its seen ranges"
    );
}

#[cfg(feature = "plugins")]
#[test]
fn pane_commands_preserve_unscoped_legacy_decorations() {
    let (mut h, _temp) = pane_api_harness();
    let screen = h.screen_to_string();
    assert!(
        screen.contains("BUFFER"),
        "legacy line hook still draws buffer-wide decorations:\n{screen}"
    );
    assert!(screen.contains("text=37,gutter=6"));
    h.run_palette_command("Measure legacy clear").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    let screen = h.screen_to_string();
    assert!(
        screen.contains("LEGACY"),
        "legacy clear still removes its buffer-wide namespace:\n{screen}"
    );
    assert!(
        screen.contains("text=37,gutter=6"),
        "legacy clear must not remove the pane projection with the same namespace:\n{screen}"
    );
}

#[cfg(feature = "plugins")]
#[test]
fn zero_row_pane_retires_projection_before_restoring_same_width() {
    let (mut h, _temp) = pane_api_harness();
    let generation = |screen: &str| {
        screen
            .split_whitespace()
            .find_map(|word| word.strip_prefix('G').and_then(|n| n.parse::<u64>().ok()))
            .unwrap()
    };
    let before = generation(&h.screen_to_string());
    h.resize(44, 2).unwrap();
    for _ in 0..4 {
        h.render().unwrap();
    }
    h.resize(44, 20).unwrap();
    h.wait_for_async_quiescence(4).unwrap();
    assert!(h.screen_to_string().contains("text=37,gutter=6"));
    assert_ne!(
        generation(&h.screen_to_string()),
        before,
        "a pane without text rows must retire its projection even when restored at the same width"
    );
}

#[cfg(feature = "plugins")]
#[test]
fn retargeted_pane_rejects_old_buffer_mutation_and_nested_wrappers() {
    use fresh_core::api::{PaneDecorationContext, PluginCommand};
    let (mut h, temp) = pane_api_harness();
    let screen = h.screen_to_string();
    let identity = |prefix: char| {
        screen
            .split_whitespace()
            .find_map(|word| {
                word.strip_prefix(prefix)
                    .and_then(|n| n.parse::<u64>().ok())
            })
            .unwrap()
    };
    let context = PaneDecorationContext {
        window_id: identity('W'),
        split_id: SplitId(identity('S') as usize),
        buffer_id: fresh_core::BufferId(identity('B') as usize),
        layout_generation: identity('G'),
        plugin_name: "measure".into(),
    };
    let buffer_id = context.buffer_id;
    let target = temp.path().join("another.txt");
    std::fs::write(&target, "another buffer\n").unwrap();
    // Exercise the dispatcher before another frame can prune the old offer.
    // The invariant is resource ownership, not visible UI: inspect this
    // component's marker-owning manager rather than invent a screen wait.
    h.editor_mut().open_file(&target).unwrap();
    let before = h
        .editor()
        .active_window()
        .buffer_state(buffer_id)
        .unwrap()
        .conceals
        .len();
    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context: context.clone(),
            command: Box::new(PluginCommand::AddConceal {
                buffer_id,
                namespace: OverlayNamespace::from_string("measure".into()),
                start: 0,
                end: 1,
                replacement: Some("STALE".into()),
                epoch: None,
                activation: None,
                compose_only: false,
            }),
        })
        .unwrap();
    assert_eq!(
        h.editor()
            .active_window()
            .buffer_state(buffer_id)
            .unwrap()
            .conceals
            .len(),
        before,
        "a pane retargeted to another buffer must not mutate its previous projection"
    );
    let nested = PluginCommand::PaneDecoration {
        context: context.clone(),
        command: Box::new(PluginCommand::ClearConcealNamespace {
            buffer_id,
            namespace: OverlayNamespace::from_string("measure".into()),
        }),
    };
    assert!(
        h.editor_mut()
            .handle_plugin_command(PluginCommand::PaneDecoration {
                context,
                command: Box::new(nested),
            })
            .is_err(),
        "nested envelopes must be rejected before dispatch"
    );
}

#[cfg(feature = "plugins")]
#[test]
fn text_layout_api_rejects_calls_outside_a_synchronous_pane_hook() {
    let (mut h, _temp) = pane_api_harness();
    h.run_palette_command("Measure layout outside").unwrap();
    h.wait_for_prompt_closed().unwrap();
    h.wait_for_screen_contains("outside=false").unwrap();
}

#[cfg(feature = "plugins")]
#[test]
fn text_layout_ownership_maps_epochs_and_rejects_unscoped_or_stale_commands() {
    use fresh_core::api::{PaneDecorationContext, PluginCommand, TextLayoutRange};
    let (mut h, _temp) = pane_api_harness();
    let screen = h.screen_to_string();
    let identity = |prefix: char| {
        screen
            .split_whitespace()
            .find_map(|word| {
                word.strip_prefix(prefix)
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .unwrap()
    };
    let context = PaneDecorationContext {
        window_id: identity('W'),
        split_id: SplitId(identity('S') as usize),
        buffer_id: fresh_core::BufferId(identity('B') as usize),
        layout_generation: identity('G'),
        plugin_name: "measure".into(),
    };
    let buffer_id = context.buffer_id;
    let epoch = h.editor().active_state().buffer.version();
    let count = |h: &crate::common::harness::EditorTestHarness| {
        h.editor()
            .active_state()
            .marker_list
            .query_range(0, 10000)
            .len()
    };
    let command = |policy, epoch, reflowed| PluginCommand::ReplaceTextLayoutRanges {
        buffer_id,
        namespace: OverlayNamespace::from_string("flow-test".into()),
        policy,
        covered: vec![TextLayoutRange { start: 0, end: 7 }],
        reflowed,
        epoch,
    };
    let span = || vec![TextLayoutRange { start: 0, end: 7 }];
    let before = count(&h);
    h.editor_mut()
        .handle_plugin_command(command(1, Some(epoch), span()))
        .unwrap();
    assert_eq!(
        count(&h),
        before,
        "unscoped ownership must not be installed"
    );

    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context: context.clone(),
            command: Box::new(command(1, Some(epoch), span())),
        })
        .unwrap();
    assert_eq!(count(&h), before + 1);
    h.type_text("xx").unwrap();
    h.editor_mut().sync_plugin_runtime();
    let before = count(&h);
    let mut obsolete = context.clone();
    obsolete.layout_generation += 1000;
    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context: obsolete,
            command: Box::new(command(2, Some(epoch), vec![])),
        })
        .unwrap();
    assert_eq!(
        count(&h),
        before,
        "stale generation must not retire current layout"
    );

    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context: context.clone(),
            command: Box::new(command(1, Some(epoch), vec![])),
        })
        .unwrap();
    assert_eq!(
        count(&h),
        before - 1,
        "old coordinates must map to the shifted span"
    );
    let boundary_before = h
        .editor()
        .active_state()
        .marker_list
        .query_range(0, 0)
        .len();
    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context: context.clone(),
            command: Box::new(command(1, Some(epoch), span())),
        })
        .unwrap();
    assert_eq!(
        h.editor()
            .active_state()
            .marker_list
            .query_range(0, 0)
            .len(),
        boundary_before + 1,
        "stale ownership must include a newly inserted source prefix"
    );
    let current = h.editor().active_state().buffer.version();
    h.editor_mut()
        .active_state_mut()
        .coord_map
        .record_barrier(current);
    let before = count(&h);
    h.editor_mut()
        .handle_plugin_command(PluginCommand::PaneDecoration {
            context,
            command: Box::new(command(2, Some(epoch), vec![])),
        })
        .unwrap();
    assert_eq!(
        count(&h),
        before,
        "unmappable epochs must not partially clear ownership"
    );
}
