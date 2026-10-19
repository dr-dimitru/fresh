//! Ownership of opt-in pane decorations. Coordinates still live in the
//! buffer's marker list; a pane owns only its projection of those coordinates.

use fresh_core::SplitId;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum DecorationScope {
    #[default]
    Buffer,
    Pane {
        split_id: SplitId,
        plugin_name: String,
    },
}

impl DecorationScope {
    pub fn visible_in(&self, pane: Option<SplitId>) -> bool {
        match self {
            Self::Buffer => true,
            Self::Pane { split_id, .. } => Some(*split_id) == pane,
        }
    }

    fn pane(&self) -> Option<SplitId> {
        match self {
            Self::Buffer => None,
            Self::Pane { split_id, .. } => Some(*split_id),
        }
    }
}

/// Mutation scope is installed only while synchronously applying one validated
/// plugin command, then restored. Queries always name their pane explicitly.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScopedVersions {
    pub write: DecorationScope,
    versions: HashMap<Option<SplitId>, u32>,
}

impl ScopedVersions {
    pub fn retain_panes(&mut self, live: &std::collections::HashSet<SplitId>) {
        self.versions
            .retain(|pane, _| pane.is_none_or(|pane| live.contains(&pane)));
    }

    pub fn bump(&mut self) {
        let version = self.versions.entry(self.write.pane()).or_default();
        *version = version.wrapping_add(1);
    }

    pub fn version_for(&self, pane: Option<SplitId>) -> u32 {
        let global = self.versions.get(&None).copied().unwrap_or_default();
        match pane {
            None => global,
            Some(pane) => {
                global.wrapping_add(self.versions.get(&Some(pane)).copied().unwrap_or_default())
            }
        }
    }
}

/// Seen content and geometry for one visible pane/buffer pair. A generation
/// changes only with layout/mode or lifetime, never merely with scrolling.
#[derive(Debug)]
pub(crate) struct PaneLineOffer {
    pub layout: fresh_core::api::PaneLayout,
    pub view_mode: String,
    pub generation: u64,
    pub seen: std::collections::HashSet<(usize, usize)>,
}

/// Deterministic work instrumentation for viewport offer regressions.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaneOfferStats {
    pub batches: u64,
    pub bytes: u64,
    pub lines: u64,
}

thread_local! {
    static OFFER_STATS: std::cell::Cell<PaneOfferStats> = const { std::cell::Cell::new(PaneOfferStats { batches: 0, bytes: 0, lines: 0 }) };
}

#[doc(hidden)]
pub fn offer_stats() -> PaneOfferStats {
    OFFER_STATS.with(|stats| stats.get())
}

pub(crate) fn count_offer(lines: usize, bytes: usize) {
    OFFER_STATS.with(|stats| {
        let mut value = stats.get();
        value.batches += 1;
        value.lines += lines as u64;
        value.bytes += bytes as u64;
        stats.set(value);
    });
}
