use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::result;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use scheduled_executor::ThreadPoolExecutor;
use crate::collections::{UpdatingMap, UpdatingSet};
use crate::processors::RawConfigProcessor;
use crate::sources::ConfigSource;

#[derive(Debug)]
pub struct Error {
    pub msg: String,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.msg.as_str())
    }
}

impl Error {
    pub fn new(msg: &str) -> Error {
        Error {
            msg: String::from(msg)
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

pub struct FullDatasetCache<O> {
    collection: Arc<O>,
    name: String,
    scheduler: ThreadPoolExecutor,
}

impl<O: 'static> FullDatasetCache<O> {
    pub fn new_set<
        V: Eq + Hash + Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashSet<V>> + Send + Sync + 'static
    >(
        name: String, source: C, processor: P, interval: Duration,
    ) -> Result<FullDatasetCache<UpdatingSet<V>>> {
        FullDatasetCache::<UpdatingSet<V>>::set_with_callbacks(
            name, source, processor, interval, |_, _, _| {}, |_| {},
        )
    }

    pub fn set_with_callbacks<
        V: Eq + Hash + Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashSet<V>> + Send + Sync + 'static,
        U: Fn(&Option<(u64, HashSet<V>)>, &u64, &HashSet<V>) -> () + Send + Sync + 'static,
        F: Fn(&Error) -> () + Send + Sync + 'static,
    >(
        name: String, source: C, processor: P, interval: Duration,
        on_update: U, on_failure: F,
    ) -> Result<FullDatasetCache<UpdatingSet<V>>> {
        FullDatasetCache::construct_and_start(
            name, source, processor,interval, on_update, on_failure, UpdatingSet::new
        )
    }

    pub fn new_map<
        K: Eq + Hash + Send + Sync + 'static,
        V: Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashMap<K, Arc<V>>> + Send + Sync + 'static
    >(
        name: String, source: C, processor: P, interval: Duration,
    ) -> Result<FullDatasetCache<UpdatingMap<K, V>>> {
        FullDatasetCache::<UpdatingMap<K, V>>::map_with_callbacks(
            name, source, processor, interval, |_, _, _| {}, |_| {},
        )
    }

    pub fn map_with_callbacks<
        K: Eq + Hash + Send + Sync + 'static,
        V: Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashMap<K, Arc<V>>> + Send + Sync + 'static,
        U: Fn(&Option<(u64, HashMap<K, Arc<V>>)>, &u64, &HashMap<K, Arc<V>>) -> () + Send + Sync + 'static,
        F: Fn(&Error) -> () + Send + Sync + 'static,
    >(
        name: String, source: C, processor: P, interval: Duration,
        on_update: U, on_failure: F,
    ) -> Result<FullDatasetCache<UpdatingMap<K, V>>> {
        FullDatasetCache::construct_and_start(
            name, source, processor,interval, on_update, on_failure, UpdatingMap::new
        )
    }

    fn construct_and_start<
        T: Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, T> + Send + Sync + 'static,
        U: Fn(&Option<(u64, T)>, &u64, &T) -> () + Send + Sync + 'static,
        F: Fn(&Error) -> () + Send + Sync + 'static,
        R: Fn(Arc<RwLock<Arc<Option<(u64, T)>>>>) -> O + Send + Sync + 'static,
    >(
        name: String, source: C, processor: P, interval: Duration,
        on_update: U, on_failure: F, constructor: R
    ) -> Result<FullDatasetCache<O>> {
        let holder: Arc<RwLock<Arc<Option<(u64, T)>>>> = Arc::new(RwLock::new(Arc::new(None)));
        let update_fn = FullDatasetCache::<O>::get_update_fn(holder.clone(), source, processor);
        let initial_fetch = update_fn()?;

        match initial_fetch.as_ref() {
            None => return Err(Error::new("Couldn't complete initial fetch")),
            Some((v, s)) => on_update(&None, v, s),
        }

        let collection = Arc::new(constructor(holder.clone()));
        let scheduler = ThreadPoolExecutor::with_prefix(1, name.as_str()).unwrap();
        scheduler.schedule_fixed_rate(interval, interval, move |_| {
            let previous = {
                holder.read().expect("Failed to read lock ").clone()
            };

            match update_fn() {
                Ok(a) => if let Some((v, t)) = a.as_ref() {
                    on_update(previous.as_ref(), v, t);
                }
                Err(e) => on_failure(&e),
            }
        });

        Ok(FullDatasetCache {
            name,
            collection,
            scheduler,
        })

    }

    pub fn get_collection(&self) -> Arc<O> {
        self.collection.clone()
    }

    fn get_update_fn<
        S,
        T,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, T> + Send + Sync + 'static,
    >(
        holder: Arc<RwLock<Arc<Option<(u64, T)>>>>, source: C, processor: P
    ) -> impl Fn() -> Result<Arc<Option<(u64, T)>>> {
        move || {
            let version = match holder.read().expect("Failed to get read lock").clone().as_ref() {
                None => None,
                Some((v, _)) => Some(v.clone())
            };

            let raw_update = match version {
                None => Ok(Some(source.fetch()?)),
                Some(v) => source.fetch_if_newer(v),
            }?;

            //TODO: Definitely a cleaner way to do this
            let update = match raw_update {
                None => None,
                Some((v, s)) => Some((v, processor.process(s)?)),
            };

            match &update {
                Some(_) => {
                    let mut write_lock = holder.write().expect("Failed to get write lock");
                    *write_lock = Arc::new(update);
                    Ok(write_lock.clone())
                },
                None => Ok(Arc::new(None))
            }
        }
    }
}
