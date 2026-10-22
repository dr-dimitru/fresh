//! Indexed, edit-anchored ownership of pane-local text layout. These ranges
//! draw nothing: they identify old conceal/soft-break projections to retire.
use crate::model::marker::{MarkerId, MarkerList};
use crate::view::pane_decorations::DecorationScope;
use fresh_core::overlay::OverlayNamespace;
use std::collections::HashMap;
use std::ops::Range;

type Owner = (DecorationScope, OverlayNamespace);

#[derive(Debug, Default)]
pub struct LayoutRetirement {
    pub ranges: Vec<Range<usize>>,
    pub escaped: bool,
    pub policy_changed: bool,
}

#[derive(Debug, Default)]
pub struct TextLayoutRanges {
    scope: DecorationScope,
    owners: HashMap<MarkerId, Owner>,
    policies: HashMap<Owner, u64>,
}

impl TextLayoutRanges {
    pub fn set_scope(&mut self, scope: DecorationScope) {
        self.scope = scope;
    }

    pub fn scope(&self) -> &DecorationScope {
        &self.scope
    }

    /// Validate all new coverage before mutation, retire intersecting old spans,
    /// then record the new layout. Query cost is O(log markers + local hits),
    /// not a scan of every previously visited paragraph. Policy changes and
    /// teardown deliberately clear accumulated state once.
    pub fn replace(
        &mut self,
        markers: &mut MarkerList,
        namespace: &OverlayNamespace,
        policy: u64,
        covered: &[Range<usize>],
        reflowed: &[Range<usize>],
    ) -> Option<LayoutRetirement> {
        if covered.iter().any(|range| range.start > range.end)
            || reflowed.iter().any(|range| range.start >= range.end)
        {
            return None;
        }
        let mut covered = covered.to_vec();
        covered.sort_unstable_by_key(|range| range.start);
        let mut union: Vec<Range<usize>> = Vec::new();
        for range in covered {
            if let Some(last) = union.last_mut().filter(|last| range.start <= last.end) {
                last.end = last.end.max(range.end);
            } else {
                union.push(range);
            }
        }
        let contains = |range: &Range<usize>| {
            union
                .iter()
                .any(|part| part.start <= range.start && range.end <= part.end)
        };
        if reflowed.iter().any(|range| !contains(range)) {
            return None;
        }

        let owner = (self.scope.clone(), namespace.clone());
        let policy_changed = self.policies.get(&owner) != Some(&policy);
        let mut retired = LayoutRetirement {
            policy_changed,
            ..Default::default()
        };
        if policy_changed {
            let ids: Vec<_> = self
                .owners
                .iter()
                .filter_map(|(id, key)| (key == &owner).then_some(*id))
                .collect();
            for id in ids {
                self.owners.remove(&id);
                markers.delete(id);
            }
            self.policies.insert(owner.clone(), policy);
        } else {
            let mut hits = HashMap::new();
            for part in &union {
                for (id, start, end) in markers.query_range(part.start, part.end) {
                    // The marker tree's query is closed; layout membership is
                    // half-open, except a deleted span collapsed inside coverage.
                    if self.owners.get(&id) == Some(&owner)
                        && ((start < part.end && part.start < end)
                            || (start == end && part.start <= start && start <= part.end))
                    {
                        hits.insert(id, start..end);
                    }
                }
            }
            for (id, range) in hits {
                retired.escaped |= !contains(&range);
                retired.ranges.push(range);
                self.owners.remove(&id);
                markers.delete(id);
            }
            retired
                .ranges
                .sort_unstable_by_key(|range| (range.start, range.end));
        }
        for range in reflowed {
            let id = markers.create_span_left_gravity(range.start, range.end);
            self.owners.insert(id, owner.clone());
        }
        Some(retired)
    }

    pub fn clear_namespace(&mut self, namespace: &OverlayNamespace, markers: &mut MarkerList) {
        let owner = (self.scope.clone(), namespace.clone());
        let ids: Vec<_> = self
            .owners
            .iter()
            .filter_map(|(id, key)| (key == &owner).then_some(*id))
            .collect();
        for id in ids {
            self.owners.remove(&id);
            markers.delete(id);
        }
        self.policies.remove(&owner);
    }

    /// Called by the same pane/plugin lifetime owner as drawn decorations.
    pub fn clear_scope(&mut self, markers: &mut MarkerList) {
        let ids: Vec<_> = self
            .owners
            .iter()
            .filter_map(|(id, (scope, _))| (scope == &self.scope).then_some(*id))
            .collect();
        for id in ids {
            self.owners.remove(&id);
            markers.delete(id);
        }
        self.policies.retain(|(scope, _), _| scope != &self.scope);
    }
}
