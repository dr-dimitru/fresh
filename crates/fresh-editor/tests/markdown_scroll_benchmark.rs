//! Manual attribution benchmark. Run with `cargo test -p fresh-editor
//! --profile profiling --test all_tests markdown_scroll_benchmark -- --ignored --nocapture`.
//! No wall-clock assertions: timings are evidence, not CI pass/fail criteria.

use crate::common::harness::{copy_plugin, copy_plugin_lib, EditorTestHarness, HarnessOptions};
use crossterm::event::{KeyCode, KeyModifiers};
use fresh::config::Config;
use std::time::Instant;

#[test]
#[ignore = "manual optimized-build scrolling benchmark"]
fn small_wrapped_markdown() {
    let lines = std::env::var("SCROLL_BENCH_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(150);
    let passes = std::env::var("SCROLL_BENCH_PASSES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    for compose in [false, true] {
        for numbers in [false, true] {
            for threshold in [10 * 1024 * 1024, 0] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("scroll.md");
                let text: String = (0..lines).map(|i| format!(
                    "Paragraph {i:04}: **bold**, *italic*, `code`, and Unicode café 日本語 🦀. {}\n",
                    "Words that wrap across the viewport. ".repeat(5)
                )).collect();
                std::fs::write(&path, &text).unwrap();
                let plugins = temp.path().join("plugins");
                std::fs::create_dir(&plugins).unwrap();
                if compose {
                    copy_plugin(&plugins, "markdown_compose");
                    copy_plugin_lib(&plugins);
                }
                let mut config = Config::default();
                config.editor.line_wrap = true;
                // Measure delivered rows, not queued wheel-animation frames.
                config.editor.smooth_scroll = false;
                config.editor.line_numbers = numbers;
                config.editor.large_file_threshold_bytes = threshold;
                let mut h = EditorTestHarness::create(
                    100,
                    35,
                    HarnessOptions::new()
                        .with_config(config)
                        .with_working_dir(temp.path().to_path_buf())
                        .with_full_grammar_registry(),
                )
                .unwrap();
                h.open_file(&path).unwrap();
                h.render().unwrap();
                if compose {
                    h.send_key(KeyCode::Char('p'), KeyModifiers::CONTROL)
                        .unwrap();
                    h.type_text("Toggle Compose").unwrap();
                    h.wait_for_screen_contains("Toggle Compose").unwrap();
                    h.send_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
                    h.wait_for_prompt_closed().unwrap();
                    // Compose hides numbers on entry, regardless of config.
                    if numbers {
                        h.run_palette_command("Toggle Line Numbers (Current Buffer)")
                            .unwrap();
                        h.wait_for_prompt_closed().unwrap();
                    }
                    h.wait_until_stable(|_| true).unwrap();
                }
                let start = Instant::now();
                for pass in 0..passes {
                    h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
                    h.reset_highlight_stats();
                    let before = h.editor().active_state().wrap_indices.stats();
                    let started = Instant::now();
                    for _ in 0..60 {
                        h.mouse_scroll_down(60, 10).unwrap();
                        h.editor_mut().process_async_messages();
                    }
                    let after = h.editor().active_state().wrap_indices.stats();
                    eprintln!("compose={compose} numbers={numbers} threshold={threshold} lines={lines} pass={pass}: {:?}, highlight={:?}, built={}, repaired={}",
                        started.elapsed(), h.highlight_stats(), after.lines_built - before.lines_built, after.lines_repaired - before.lines_repaired);
                }
                eprintln!("case total {:?}", start.elapsed());
            }
        }
    }
}
