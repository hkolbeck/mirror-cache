use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::{BufRead, BufReader, Read};
use crate::cache::Result;

pub trait RawConfigProcessor<S, T> {
    fn process(&self, raw: S) -> Result<T>;
}

pub struct RawLineSetProcessor<V: Eq + Hash + Sync + Send, P: Fn(String) -> Result<V>> {
    parse: P,
}

impl<V: Eq + Hash + Sync + Send, P: Fn(String) -> Result<V>> RawLineSetProcessor<V, P> {
    pub fn new(parse: P) -> RawLineSetProcessor<V, P> {
        RawLineSetProcessor {
            parse
        }
    }
}

impl<R: Read, V: Eq + Hash + Send + Sync, P: Fn(String) -> Result<V>>
RawConfigProcessor<R, HashSet<V>> for RawLineSetProcessor<V, P> {

    fn process(&self, raw: R) -> Result<HashSet<V>> {
        let mut set: HashSet<V> = HashSet::new();
        let mut lines = BufReader::new(raw).lines();
        for line in lines {
            set.insert(self.parse(line?)?)
        }

        Ok(set)
    }
}

pub struct RawLineMapProcessor<K: Eq + Hash + Sync + Send, V: Sync + Send, P: Fn(String) -> Result<Entry<'static, K, V>>> {
    parse: P,
}

impl<K: Eq + Hash + Sync + Send, V: Sync + Send, P: Fn(String) -> Result<Entry<'static, K, V>>>
RawLineMapProcessor<K, V, P> {

    pub fn new(parse: P) -> RawLineMapProcessor<K, V, P> {
        RawLineMapProcessor {
            parse
        }
    }
}

impl<R: Read, K: Eq + Hash + Sync + Send, V: Sync + Send, P: Fn(String) -> Result<Entry<'static, K, V>>>
RawConfigProcessor<R, HashMap<K, V>> for RawLineMapProcessor<K, V, P> {

    fn process(&self, raw: R) -> Result<HashMap<K, V>> {
        let mut map: HashMap<K, V> = HashMap::new();
        let mut lines = BufReader::new(raw).lines();
        for line in lines {
            let entry: Entry<K, V> = self.parse(line)?;
            let (k, v) = entry.into();
            map.insert(k, v);
        }

        Ok(map)
    }
}