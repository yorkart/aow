//! Correlation survives message eviction, but abandoned requests cannot grow forever.
use super::RequestContext;
use std::collections::VecDeque;

const MAX_PENDING_CONTEXTS: usize = 1024;

#[derive(Default)]
pub(super) struct Contexts<K> {
    entries: VecDeque<(K, RequestContext)>,
}

impl<K: PartialEq> Contexts<K> {
    pub(super) fn insert(&mut self, key: K, value: RequestContext) {
        self.remove(&key);
        if self.entries.len() == MAX_PENDING_CONTEXTS {
            self.entries.pop_front();
        }
        self.entries.push_back((key, value));
    }

    pub(super) fn get(&self, key: &K) -> Option<&RequestContext> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, value)| value)
    }

    pub(super) fn remove(&mut self, key: &K) -> Option<RequestContext> {
        let index = self.entries.iter().position(|(entry, _)| entry == key)?;
        self.entries.remove(index).map(|(_, value)| value)
    }

    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut RequestContext> {
        self.entries.iter_mut().map(|(_, value)| value)
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&RequestContext) -> bool) {
        self.entries.retain(|(_, value)| keep(value));
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
