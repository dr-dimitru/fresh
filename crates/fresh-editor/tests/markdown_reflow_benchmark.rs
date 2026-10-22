//! Manual optimized benchmark, no timing assertions.
//! cargo test -p fresh-editor --profile profiling --test all_tests
//! markdown_reflow_benchmark:: -- --ignored --nocapture --test-threads=1
//! REFLOW_BENCH_LABEL identifies the build; REFLOW_BENCH_LIMIT selects policy.
//! Copy this file unchanged into a baseline source export for a full-build comparison.
#![cfg(feature = "plugins")]

use crate::common::harness::{copy_plugin, copy_plugin_lib, EditorTestHarness, HarnessOptions};
use crossterm::event::{KeyCode, KeyModifiers, MouseEvent, MouseEventKind};
use fresh::config::{Config, PluginConfig};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Frames {
    total: Duration,
    worst: Duration,
    count: usize,
    sync: Duration,
}

impl Frames {
    fn settle(&mut self, h: &mut EditorTestHarness) {
        // A runtime barrier, not a fixed sleep or a guess at how many ticks the
        // plugin needs. Each render can offer more newly visible source lines.
        loop {
            let started = Instant::now();
            let work = h.editor_mut().sync_plugin_runtime();
            self.sync += started.elapsed();
            let started = Instant::now();
            h.tick_and_render().unwrap();
            let elapsed = started.elapsed();
            self.total += elapsed;
            self.worst = self.worst.max(elapsed);
            self.count += 1;
            let started = Instant::now();
            let more = h.editor_mut().sync_plugin_runtime();
            self.sync += started.elapsed();
            if !work && !more {
                break;
            }
        }
    }
}

fn document(kind: &str) -> String {
    let prose = "**bold** café 日本語 🦀 and words across the viewport. ".repeat(16);
    let mut out = String::from("# Benchmark\n\n");
    let paragraphs = if kind == "eligible" { 1200 } else { 120 };
    for n in 0..paragraphs {
        out.push_str(&format!("Block{n:04} "));
        if kind == "eligible" {
            out.push_str(
                "Short **bold** café source line\n  continued on its second source line.\n\n",
            );
        } else if kind == "single" {
            out.push_str(&prose);
            out.push_str("\n\n");
        } else {
            for part in prose.split_inclusive("viewport. ") {
                out.push_str(part);
                out.push('\n');
            }
            out.push('\n');
        }
        if kind == "mixed" && n % 4 == 0 {
            out.push_str("1. List item with prose\n   continued on its source line.\n\n> Quoted prose\n> with continuation.\n\n```js\nconst answer = 42;\n```\n\n| Name | Value |\n| --- | --- |\n| entry | data |\n\n");
        }
    }
    out.push_str("END_MARKER\n");
    out
}

// Stable FNV-1a workload identity, independent of Rust's randomized hash seeds.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
#[ignore = "manual optimized baseline/candidate benchmark"]
fn first_and_warm_prose_passes() {
    let label = std::env::var("REFLOW_BENCH_LABEL").unwrap_or_else(|_| "unspecified".into());
    let limit: usize = std::env::var("REFLOW_BENCH_LIMIT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    for kind in ["single", "multiline", "mixed", "eligible"] {
        for numbers in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let plugins = temp.path().join("plugins");
            std::fs::create_dir(&plugins).unwrap();
            copy_plugin(&plugins, "markdown_compose");
            copy_plugin_lib(&plugins);
            let text = document(kind);
            let path = temp.path().join("benchmark.md");
            std::fs::write(&path, &text).unwrap();
            let mut config = Config {
                lsp_enabled: false,
                check_for_updates: false,
                ..Config::default()
            };
            config.editor.smooth_scroll = false;
            config.editor.animations = false;
            config.editor.line_numbers = numbers;
            config.plugins.insert(
                "markdown_compose".into(),
                PluginConfig {
                    enabled: true,
                    path: None,
                    settings: serde_json::json!({"maxProseReflowCodeUnits": limit}),
                },
            );
            let mut h = EditorTestHarness::create(
                100,
                35,
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
            if numbers {
                h.run_palette_command("Toggle Line Numbers (Current Buffer)")
                    .unwrap();
                h.wait_for_prompt_closed().unwrap();
            }
            Frames::default().settle(&mut h);
            for pass in ["first", "warm"] {
                h.send_key(KeyCode::Home, KeyModifiers::CONTROL).unwrap();
                Frames::default().settle(&mut h);
                let before = h.editor().active_state().wrap_indices.stats();
                h.reset_highlight_stats();
                let started = Instant::now();
                let mut frames = Frames::default();
                let mut worst_step = Duration::ZERO;
                let notches = if kind == "eligible" { 240 } else { 60 };
                for _ in 0..notches {
                    let step_started = Instant::now();
                    h.send_mouse(MouseEvent {
                        kind: MouseEventKind::ScrollDown,
                        column: 50,
                        row: 10,
                        modifiers: KeyModifiers::NONE,
                    })
                    .unwrap();
                    frames.settle(&mut h);
                    worst_step = worst_step.max(step_started.elapsed());
                }
                let elapsed = started.elapsed();
                let after = h.editor().active_state().wrap_indices.stats();
                let screen = h.screen_to_string();
                let coverage = screen
                    .lines()
                    .filter(|line| line.contains("Block"))
                    .collect::<Vec<_>>()
                    .join(" / ");
                eprintln!("REFLOW label={label} limit={limit} kind={kind} numbers={numbers} pass={pass} hash={:016x} bytes={} viewport=100x35 wheel={notches} total_ms={:.3} frame_total_ms={:.3} worst_frame_ms={:.3} worst_step_ms={:.3} frames={} sync_ms={:.3} built={} repaired={} highlight={:?} coverage={coverage:?}",
                    hash(&text), text.len(), elapsed.as_secs_f64()*1000.0,
                    frames.total.as_secs_f64()*1000.0, frames.worst.as_secs_f64()*1000.0,
                    worst_step.as_secs_f64()*1000.0, frames.count, frames.sync.as_secs_f64()*1000.0,
                    after.lines_built-before.lines_built, after.lines_repaired-before.lines_repaired, h.highlight_stats());
            }
        }
    }
}
