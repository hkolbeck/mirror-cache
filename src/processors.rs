use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::{BufRead, BufReader, Read};
use std::sync::Arc;
use crate::cache::Result;

pub trait RawConfigProcessor<S, T> {
    fn process(&self, raw: S) -> Result<T>;
}

pub struct RawLineSetProcessor<V: Eq + Hash + Sync + Send, P: Fn(String) -> Result<Option<V>>> {
    parse: P,
}

impl<
    V: Eq + Hash + Sync + Send + 'static,
    P: Fn(String) -> Result<Option<V>> + 'static
> RawLineSetProcessor<V, P> {
    pub fn new(parse: P) -> RawLineSetProcessor<V, P> {
        RawLineSetProcessor {
            parse
        }
    }
}

impl<
    R: Read,
    V: Eq + Hash + Send + Sync + 'static,
    P: Fn(String) -> Result<Option<V>> + 'static
> RawConfigProcessor<R, HashSet<V>> for RawLineSetProcessor<V, P> {
    fn process(&self, raw: R) -> Result<HashSet<V>> {
        let mut set: HashSet<V> = HashSet::new();
        let lines = BufReader::new(raw).lines();
        for line in lines {
            if let Some(v) = (self.parse)(line?)? {
                set.insert(v);
            }
        }

        Ok(set)
    }
}

pub struct RawLineMapProcessor<
    K: Eq + Hash + Sync + Send + 'static,
    V: Sync + Send + 'static,
    P: Fn(String) -> Result<Option<(K, V)>> + 'static
> {
    parse: P,
}

impl<
    K: Eq + Hash + Sync + Send + 'static,
    V: Sync + Send + 'static,
    P: Fn(String) -> Result<Option<(K, V)>> + 'static
> RawLineMapProcessor<K, V, P> {
    pub fn new(parse: P) -> RawLineMapProcessor<K, V, P> {
        RawLineMapProcessor {
            parse
        }
    }
}

impl<
    R: Read,
    K: Eq + Hash + Sync + Send + 'static,
    V: Sync + Send + 'static,
    P: Fn(String) -> Result<Option<(K, V)>> + 'static
> RawConfigProcessor<R, HashMap<K, Arc<V>>> for RawLineMapProcessor<K, V, P> {
    fn process(&self, raw: R) -> Result<HashMap<K, Arc<V>>> {
        let mut map: HashMap<K, Arc<V>> = HashMap::new();
        let lines = BufReader::new(raw).lines();
        for line in lines {
            if let Some((k, v)) = (self.parse)(line?)? {
                map.insert(k, Arc::new(v));
            }
        }

        Ok(map)
    }
}