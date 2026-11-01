//! Purpose: Share propagated identifier bytes while retaining per-scope authority.

use std::collections::BTreeMap;
use std::sync::Arc;

/// Named unknown contributors retain one shared string across their scope chain.
#[derive(Default)]
pub(super) struct NameBarriers {
    /// Each genuinely new scope/name pair uses an admitted lexical-name slot.
    scopes: BTreeMap<usize, BTreeMap<Arc<str>, usize>>,
}

impl NameBarriers {
    /// Merge an existing monotone barrier without copying text or consuming a slot.
    pub(super) fn update_existing(&mut self, scope: usize, name: &str, from: usize) -> bool {
        let Some(existing) = self
            .scopes
            .get_mut(&scope)
            .and_then(|names| names.get_mut(name))
        else {
            return false;
        };
        *existing = (*existing).min(from);
        true
    }

    /// Insert only after the owning collector admits this new retained-name slot.
    pub(super) fn insert_admitted(&mut self, scope: usize, name: Arc<str>, from: usize) {
        self.scopes.entry(scope).or_default().insert(name, from);
    }

    /// Lookup is by identifier value, independent of the shared allocation identity.
    pub(super) fn visible(&self, scope: usize, name: &str, offset: usize) -> bool {
        self.scopes
            .get(&scope)
            .and_then(|names| names.get(name))
            .is_some_and(|from| *from <= offset)
    }
}

#[cfg(test)]
mod tests {
    use super::NameBarriers;
    use std::sync::Arc;

    #[test]
    fn propagated_barriers_share_identifier_storage_and_earliest_visibility() {
        let name: Arc<str> = Arc::from("x".repeat(32_768));
        let mut barriers = NameBarriers::default();
        for scope in 0..64 {
            assert!(!barriers.update_existing(scope, &name, 20));
            barriers.insert_admitted(scope, Arc::clone(&name), 20);
            assert!(barriers.update_existing(scope, &name, 10));
            assert!(!barriers.visible(scope, &name, 9));
            assert!(barriers.visible(scope, &name, 10));
        }
        assert_eq!(Arc::strong_count(&name), 65);
        assert!(
            barriers
                .scopes
                .values()
                .flat_map(|names| names.keys())
                .all(|key| Arc::ptr_eq(key, &name))
        );
        let equal_value = name.to_string();
        assert!(barriers.visible(0, &equal_value, 10));
        assert!(!barriers.visible(0, "different", usize::MAX));
    }
}
