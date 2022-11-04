use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;
use crate::cache::Holder;

pub struct UpdatingSet<E, T: Eq + Hash + Send + Sync> {
    backing: Holder<E, HashSet<T>>
}

const NON_RUNNING: &str = "Attempt to read collection from non-running update service";

impl<E, T: Eq + Hash + Send + Sync> UpdatingSet<E, T> {
    pub(crate) fn new(backing: Holder<E, HashSet<T>>) -> UpdatingSet<E, T> {
        UpdatingSet {
            backing
        }
    }

    pub fn contains(&self, val: &T) -> bool {
        match self.get_collection().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.contains(val)
        }
    }

    pub fn len(&self) -> usize {
        match self.get_collection().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        match self.get_collection().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.is_empty()
        }
    }

    fn get_collection(&self) -> Arc<Option<(Option<E>, HashSet<T>)>> {
        self.backing.read().clone()
    }
}

pub struct UpdatingMap<E, K: Eq + Hash, V> {
    backing: Holder<E, HashMap<K, Arc<V>>>
}

impl<E, K: Eq + Hash, V> UpdatingMap<E, K, V> {
    pub(crate) fn new(backing: Holder<E, HashMap<K, Arc<V>>>) -> UpdatingMap<E, K, V> {
        UpdatingMap {
            backing
        }
    }
}

impl<E, K: Eq + Hash + Send + Sync, V: Send + Sync> UpdatingMap<E, K, V> {
    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        match self.get_collection().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.get(key).cloned()
        }
    }

    pub fn len(&self) -> usize {
        match self.get_collection().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self.get_collection().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.is_empty(),
        }
    }

    #[allow(clippy::type_complexity)]
    fn get_collection(&self) -> Arc<Option<(Option<E>, HashMap<K, Arc<V>>)>> {
        self.backing.read().clone()
    }
}