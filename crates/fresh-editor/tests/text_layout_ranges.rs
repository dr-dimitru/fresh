//! Native indexed ownership/work contracts for pane-local text layout.
// Coverage is intentionally a slice of ranges, including a single range.
#![allow(clippy::single_range_in_vec_init)]
use fresh::model::marker::MarkerList;
use fresh::view::pane_decorations::DecorationScope;
use fresh::view::text_layout_ranges::TextLayoutRanges;
use fresh_core::overlay::OverlayNamespace;
use fresh_core::SplitId;

fn pane(id: usize, plugin: &str) -> DecorationScope {
    DecorationScope::Pane {
        split_id: SplitId(id),
        plugin_name: plugin.into(),
    }
}
fn ns(name: &str) -> OverlayNamespace {
    OverlayNamespace::from_string(name.into())
}

#[test]
fn inner_fragment_retires_the_whole_old_span_and_requests_a_reoffer() {
    let mut markers = MarkerList::new();
    let mut layout = TextLayoutRanges::default();
    layout.set_scope(pane(1, "compose"));
    layout
        .replace(&mut markers, &ns("flow"), 20, &[10..30], &[10..30])
        .unwrap();
    let retired = layout
        .replace(&mut markers, &ns("flow"), 20, &[18..22], &[])
        .unwrap();
    assert_eq!(retired.ranges, vec![10..30]);
    assert!(retired.escaped);
    assert!(!retired.policy_changed);
    assert!(markers.query_range(0, 40).is_empty());
    assert!(
        !layout
            .replace(&mut markers, &ns("flow"), 20, &[18..22], &[])
            .unwrap()
            .escaped
    );
}

#[test]
fn a_large_insertion_at_the_block_head_remains_owned_for_partial_retirement() {
    let mut markers = MarkerList::new();
    let mut layout = TextLayoutRanges::default();
    layout
        .replace(&mut markers, &ns("flow"), 20, &[10..30], &[10..30])
        .unwrap();
    markers.adjust_for_insert(10, 100000);
    let retired = layout
        .replace(&mut markers, &ns("flow"), 20, &[10..50], &[])
        .unwrap();
    assert_eq!(retired.ranges, vec![10..100030]);
    assert!(retired.escaped);
}

#[test]
fn range_anchors_follow_prefix_and_internal_edits() {
    let mut markers = MarkerList::new();
    let mut layout = TextLayoutRanges::default();
    layout
        .replace(&mut markers, &ns("flow"), 20, &[10..30], &[10..30])
        .unwrap();
    markers.adjust_for_insert(0, 4);
    markers.adjust_for_insert(20, 8);
    let retired = layout
        .replace(&mut markers, &ns("flow"), 20, &[22..24], &[])
        .unwrap();
    assert_eq!(retired.ranges, vec![14..42]);
    assert!(retired.escaped);
}

#[test]
fn scopes_namespaces_and_policy_changes_have_distinct_owners() {
    let mut markers = MarkerList::new();
    let mut layout = TextLayoutRanges::default();
    for owner in [pane(1, "compose"), pane(2, "compose"), pane(1, "other")] {
        layout.set_scope(owner);
        for name in ["flow", "other-flow"] {
            layout
                .replace(&mut markers, &ns(name), 20, &[10..30], &[10..30])
                .unwrap();
        }
    }
    layout.set_scope(pane(1, "compose"));
    let retired = layout
        .replace(&mut markers, &ns("flow"), 10, &[100..110], &[])
        .unwrap();
    assert!(retired.policy_changed);
    assert_eq!(markers.query_range(0, 40).len(), 5);
    layout.clear_scope(&mut markers);
    assert_eq!(markers.query_range(0, 40).len(), 4);
    layout.set_scope(pane(2, "compose"));
    let retired = layout
        .replace(&mut markers, &ns("flow"), 20, &[12..14], &[])
        .unwrap();
    assert!(!retired.policy_changed);
    assert_eq!(retired.ranges, vec![10..30]);
}

#[test]
fn invalid_new_coverage_is_rejected_before_retiring_existing_layout() {
    let mut markers = MarkerList::new();
    let mut layout = TextLayoutRanges::default();
    layout
        .replace(&mut markers, &ns("flow"), 20, &[10..30], &[10..30])
        .unwrap();
    assert!(layout
        .replace(&mut markers, &ns("flow"), 10, &[10..15, 20..30], &[10..30])
        .is_none());
    assert_eq!(markers.query_range(0, 40).len(), 1);
    let retired = layout
        .replace(&mut markers, &ns("flow"), 20, &[10..30], &[])
        .unwrap();
    assert!(!retired.policy_changed);
    assert_eq!(retired.ranges, vec![10..30]);
}

#[test]
fn a_deleted_block_retires_its_collapsed_anchor() {
    let mut markers = MarkerList::new();
    let mut layout = TextLayoutRanges::default();
    layout
        .replace(&mut markers, &ns("flow"), 20, &[0..10], &[0..10])
        .unwrap();
    markers.adjust_for_delete(0, 10);
    let retired = layout
        .replace(&mut markers, &ns("flow"), 20, &[0..1], &[])
        .unwrap();
    assert_eq!(retired.ranges, vec![0..0]);
    assert!(markers.query_range(0, 10).is_empty());
}

#[test]
fn namespace_retirement_keeps_other_soft_breaks() {
    use fresh::view::soft_break::SoftBreakManager;
    let mut markers = MarkerList::new();
    let mut breaks = SoftBreakManager::new();
    breaks.add(&mut markers, ns("flow"), 4, 0);
    breaks.add(&mut markers, ns("table"), 4, 0);
    breaks.remove_in_range_for_namespace(&ns("flow"), 0, 10, &mut markers);
    assert_eq!(breaks.len(), 1);
}

#[test]
fn collapsed_join_anchors_are_removed_at_buffer_start() {
    use fresh::view::conceal::ConcealManager;
    let mut markers = MarkerList::new();
    let mut conceals = ConcealManager::new();
    conceals.add(&mut markers, ns("flow"), 0..5, Some(" ".into()));
    markers.adjust_for_delete(0, 5);
    conceals.remove_in_range_for_namespace(&ns("flow"), &(0..1), &mut markers);
    assert_eq!(conceals.len(), 0);
    assert!(markers.query_range(0, 10).is_empty());
}

#[test]
#[cfg(debug_assertions)]
fn native_query_work_scales_with_the_viewport_not_visited_paragraphs() {
    use fresh::model::marker_tree::query_nodes_visited;
    for visited in [100, 10_000] {
        let mut markers = MarkerList::new();
        let mut layout = TextLayoutRanges::default();
        for i in 0..visited {
            let range = i * 100..i * 100 + 20;
            layout
                .replace(
                    &mut markers,
                    &ns("flow"),
                    20,
                    std::slice::from_ref(&range),
                    std::slice::from_ref(&range),
                )
                .unwrap();
        }
        let at = visited / 2 * 100;
        let before = query_nodes_visited();
        let retired = layout
            .replace(&mut markers, &ns("flow"), 20, &[at + 5..at + 10], &[])
            .unwrap();
        let work = query_nodes_visited() - before;
        eprintln!("REFLOW_WORK visited={visited} native_query_nodes={work}");
        assert_eq!(retired.ranges, vec![at..at + 20]);
        assert!(
            work < 128,
            "{visited} stored blocks cost {work} tree nodes for one fragment"
        );
    }
}

#[test]
fn large_byte_coordinates_and_an_empty_last_line_retire_collapsed_ranges() {
    let mut markers = MarkerList::new();
    let mut layout = TextLayoutRanges::default();
    let start = (1usize << 31) + 100;
    layout
        .replace(
            &mut markers,
            &ns("flow"),
            20,
            &[start..start + 20],
            &[start..start + 20],
        )
        .unwrap();
    markers.adjust_for_delete(start, 20);
    let retired = layout
        .replace(&mut markers, &ns("flow"), 20, &[start..start], &[])
        .unwrap();
    assert_eq!(retired.ranges, vec![start..start]);
    assert!(markers.query_range(start - 1, start + 1).is_empty());
}
