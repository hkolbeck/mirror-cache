use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, RwLock, RwLockReadGuard};

pub struct UpdatingSet<T: Eq + Hash + Send + Sync> {
    backing: Arc<RwLock<Arc<Option<(u64, HashSet<T>)>>>>
}

const NON_RUNNING: &'static str = "Attempt to read collection from non-running update service";

impl<T: Eq + Hash + Send + Sync> UpdatingSet<T> {
    pub(crate) fn new(backing: Arc<RwLock<Arc<Option<(u64, HashSet<T>)>>>>) -> UpdatingSet<T> {
        UpdatingSet {
            backing
        }
    }

    pub fn contains(&self, val: &T) -> bool {
        match self.get_read_lock().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.contains(val)
        }
    }

    pub fn len(&self) -> usize {
        match self.get_read_lock().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        match self.get_read_lock().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.is_empty()
        }
    }

    fn get_read_lock(&self) -> RwLockReadGuard<Arc<Option<(u64, HashSet<T>)>>> {
        self.backing.read()
            .expect("Couldn't acquire lock on backing data structure")

    }
}

pub struct UpdatingMap<K: Eq + Hash, V> {
    backing: Arc<RwLock<Arc<Option<(u64, HashMap<K, Arc<V>>)>>>>
}

impl<K: Eq + Hash + Send + Sync, V: Send + Sync> UpdatingMap<K, V> {
    pub(crate) fn new(backing: Arc<RwLock<Arc<Option<(u64, HashMap<K, Arc<V>>)>>>>) -> UpdatingMap<K, V> {
        UpdatingMap {
            backing
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        match self.get_read_lock().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.contains_key(key)
        }
    }

    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        match self.get_read_lock().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => match h.get(key) {
                None => None,
                Some(v) => Some(v.clone())
            }
        }
    }

    pub fn len(&self) -> usize {
        match self.get_read_lock().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self.get_read_lock().as_ref() {
            None => panic!("{}", NON_RUNNING),
            Some((_, h)) => h.is_empty(),
        }
    }

    fn get_read_lock(&self) -> Arc<Option<(u64, HashMap<K, Arc<V>>)>> {
        self.backing.read()
            .expect("Couldn't acquire lock on backing data structure")
            .clone()
    }
}