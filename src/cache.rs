use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;
use std::result;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use scheduled_thread_pool::ScheduledThreadPool;
use crate::collections::{UpdatingMap, UpdatingSet};
use crate::metrics::Metrics;
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

impl<E: std::error::Error> From<E> for Error {
    fn from(e: E) -> Self {
        Error::new(e.to_string().as_str())
    }
}

pub type Result<T> = result::Result<T, Error>;

pub trait FallbackFn<T> {
    fn get_fallback(&self) -> T;
}

pub trait UpdateFn<T> {
    fn updated(&self, previous: &Option<(u128, T)>, new_version: &u128, new_dataset: &T);
}

pub trait FailureFn {
    fn failed(&self, err: &Error, last_version_and_ts: Option<(u128, Instant)>);
}

pub(crate) type Holder<T> = Arc<RwLock<Arc<Option<(u128, T)>>>>;

pub struct FullDatasetCache<O> {
    collection: Arc<O>,

    #[allow(dead_code)]
    scheduler: ScheduledThreadPool,
}

impl<O: 'static> FullDatasetCache<O> {
    #[allow(clippy::too_many_arguments)]
    fn construct_and_start<
        T: Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, T> + Send + Sync + 'static,
        U: UpdateFn<T> + Send + Sync + 'static,
        F: FailureFn + Send + Sync + 'static,
        A: FallbackFn<T> + 'static,
        M: Metrics + Send + Sync + 'static
    >(
        name: Option<String>, source: C, processor: P, interval: Duration,
        on_update: Option<U>, on_failure: Option<F>, mut metrics: Option<M>,
        fallback: Option<(u128, A)>, constructor: fn(Holder<T>) -> O,
    ) -> Result<FullDatasetCache<O>> {
        let holder: Holder<T> = Arc::new(RwLock::new(Arc::new(None)));
        let update_fn =
            FullDatasetCache::<O>::get_update_fn(holder.clone(), source, processor);
        let initial_fetch = update_fn(metrics.as_mut())?;

        match initial_fetch.as_ref() {
            None => {
                match fallback {
                    Some((v, fallback_fun)) => {
                        let mut guard = holder.write().expect("Failed to claim write lock");
                        *guard = Arc::new(Some((v, fallback_fun.get_fallback())));

                    },
                    None => return Err(Error::new("Couldn't complete initial fetch")),
                }
            }
            Some((v, s)) => {
                if let Some(update_callback) = on_update.borrow() {
                    update_callback.updated(&None, v, s);
                }
            },
        };

        let mut last_success = Instant::now();
        let collection = Arc::new(constructor(holder.clone()));
        let scheduler = match name {
            Some(n) => ScheduledThreadPool::with_name(n.as_str(), 1),
            None => ScheduledThreadPool::new(1),
        };

        scheduler.execute_at_fixed_rate(interval, interval, move || {
            let previous = {
                holder.read().expect("Failed to get read lock").clone()
            };

            match update_fn(metrics.as_mut()) {
                Ok(a) => if let Some((v, t)) = a.as_ref() {
                    last_success = Instant::now();
                    if let Some(update_callback) = &on_update {
                        update_callback.updated(&previous, v, t)
                    }
                }
                Err(e) => {
                    if let Some(failure_callback) = &on_failure {
                        let last = previous.as_ref().as_ref().map(|(v, _)| (*v, last_success));
                        failure_callback.failed(&e, last)
                    }
                }
            }
        });

        Ok(FullDatasetCache {
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
        M: Metrics + Send + Sync + 'static,
    >(
        holder: Holder<T>, source: C, processor: P,
    ) -> impl Fn(Option<&mut M>) -> Result<Arc<Option<(u128, T)>>> {
        move |metrics| {
            let version = {
                #[allow(clippy::manual_map)] //Trust me, it's uglier the other way
                match holder.read().expect("Failed to get read lock").clone().as_ref() {
                    None => None,
                    Some((v, _)) => Some(*v)
                }
            };

            let fetch_start = Instant::now();
            let raw_update = match version {
                None => source.fetch().map(Some),
                Some(v) => source.fetch_if_newer(v),
            };
            let fetch_time = Instant::now().duration_since(fetch_start);

            let process_start = Instant::now();
            let update = match raw_update {
                Ok(None) => None,
                Ok(Some((v, s))) => Some((v, processor.process(s))),
                Err(e) => {
                    if let Some(m) = metrics {
                        m.fetch_error()
                    }
                    return Err(e);
                }
            };
            let process_time = Instant::now().duration_since(process_start);

            match update {
                Some((v, Ok(new_coll))) => {
                    let ret = {
                        let mut write_lock = holder.write().expect("Failed to get write lock");
                        *write_lock = Arc::new(Some((v, new_coll)));
                        Ok(write_lock.clone())
                    };

                    if let Some(m) = metrics {
                        m.last_successful_update(Instant::now());
                        m.update(&v, fetch_time, process_time);
                    };

                    ret
                }
                Some((_, Err(e))) => {
                    if let Some(m) = metrics {
                        m.process_error()
                    }
                    Err(e)
                }
                None => {
                    if let Some(m) = metrics {
                        m.last_successful_check(&Instant::now());
                        m.check_no_update(&fetch_time);
                    }

                    Ok(Arc::new(None))
                }
            }
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn map_builder<
        K: Eq + Hash + Send + Sync + 'static,
        V: Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashMap<K, Arc<V>>> + Send + Sync + 'static,
        U: UpdateFn<HashMap<K, Arc<V>>> + Send + Sync + 'static,
        F: FailureFn + Send + Sync + 'static,
        A: FallbackFn<HashMap<K, Arc<V>>>,
        M: Metrics + Sync + Send + 'static,
    >() -> Builder<UpdatingMap<K, V>, HashMap<K, Arc<V>>, S, C, P, U, F, A, M> {
        builder(UpdatingMap::new)
    }

    pub fn set_builder<
        V: Eq + Hash + Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashSet<V>> + Send + Sync + 'static,
        U: UpdateFn<HashSet<V>> + Send + Sync + 'static,
        F: FailureFn + Send + Sync + 'static,
        A: FallbackFn<HashSet<V>>,
        M: Metrics + Sync + Send + 'static,
    >() -> Builder<UpdatingSet<V>, HashSet<V>, S, C, P, U, F, A, M> {
        builder(UpdatingSet::new)
    }
}

pub struct Builder<
    O: 'static,
    T: Send + Sync + 'static,
    S: 'static,
    C: ConfigSource<S> + Send + Sync + 'static,
    P: RawConfigProcessor<S, T> + Send + Sync + 'static,
    U: UpdateFn<T> + Send + Sync + 'static=Absent,
    F: FailureFn + Send + Sync + 'static=Absent,
    A: FallbackFn<T> + 'static=Absent,
    M: Metrics + Sync + Send + 'static=Absent,
> {
    constructor: fn(Holder<T>) -> O,
    name: Option<String>,
    fetch_interval: Option<Duration>,
    config_source: Option<C>,
    config_processor: Option<P>,
    failure_callback: Option<F>,
    update_callback: Option<U>,
    fallback: Option<(u128, A)>,
    metrics: Option<M>,
    phantom: PhantomData<S>,
}

impl<
    O: Send + Sync + 'static,
    T: Send + Sync + 'static,
    S: 'static,
    C: ConfigSource<S> + Send + Sync + 'static,
    P: RawConfigProcessor<S, T> + Send + Sync + 'static,
    U: UpdateFn<T> + Send + Sync + 'static,
    F: FailureFn + Send + Sync + 'static,
    A: FallbackFn<T> + 'static,
    M: Metrics + Sync + Send + 'static
> Builder<O, T, S, C, P, U, F, A, M> {
    pub fn with_name(mut self, name: String) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.name = Some(name);
        self
    }

    pub fn with_source(mut self, source: C) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.config_source = Some(source);
        self
    }

    pub fn with_processor(mut self, processor: P) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.config_processor = Some(processor);
        self
    }

    pub fn with_fetch_interval(mut self, fetch_interval: Duration) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.fetch_interval = Some(fetch_interval);
        self
    }

    pub fn with_update_callback(mut self, callback: U) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.update_callback = Some(callback);
        self
    }

    pub fn with_failure_callback(mut self, callback: F) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.failure_callback = Some(callback);
        self
    }

    pub fn with_metrics(mut self, metrics: M) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.metrics = Some(metrics);
        self
    }

    pub fn with_fallback(mut self, fallback_version: u128, fallback: A) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.fallback = Some((fallback_version, fallback));
        self
    }

    pub fn build(self) -> Result<FullDatasetCache<O>> {
        if self.config_source.is_none() {
            return Err(Error::new("No config source specified"));
        }

        if self.config_processor.is_none() {
            return Err(Error::new("No config processor specified"));
        }

        if self.fetch_interval.is_none() {
            return Err(Error::new("No  fetch interval specified"));
        }

        FullDatasetCache::construct_and_start(
            self.name,
            self.config_source.unwrap(),
            self.config_processor.unwrap(),
            self.fetch_interval.unwrap(),
            self.update_callback,
            self.failure_callback,
            self.metrics,
            self.fallback,
            self.constructor,
        )
    }
}

fn builder<
    O: Sync + Send + 'static,
    T: Send + Sync + 'static,
    S: 'static,
    C: ConfigSource<S> + Send + Sync + 'static,
    P: RawConfigProcessor<S, T> + Send + Sync + 'static,
    U: UpdateFn<T> + Send + Sync + 'static,
    F: FailureFn + Send + Sync + 'static,
    A: FallbackFn<T>,
    M: Metrics + Sync + Send + 'static,
>(constructor: fn(Holder<T>) -> O) -> Builder<O, T, S, C, P, U, F, A, M> {
    Builder {
        constructor,
        name: None,
        fetch_interval: None,
        config_source: None,
        config_processor: None,
        failure_callback: None,
        update_callback: None,
        fallback: None,
        metrics: None,
        phantom: Default::default(),
    }
}

pub struct Absent {}

impl<T> UpdateFn<T> for Absent {
    fn updated(&self, _previous: &Option<(u128, T)>, _new_version: &u128, _new_dataset: &T) {
        panic!("Should never be called");
    }
}

impl<T> FallbackFn<T> for Absent {
    fn get_fallback(&self) -> T {
        panic!("Should never be called");
    }
}

impl FailureFn for Absent {
    fn failed(&self, _err: &Error, _last_version_and_ts: Option<(u128, Instant)>) {
        panic!("Should never be called");
    }
}

impl Metrics for Absent {
    fn update(&mut self, _new_version: &u128, _fetch_time: Duration, _process_time: Duration) {}

    fn last_successful_update(&mut self, _ts: Instant) {}

    fn check_no_update(&mut self, _check_time: &Duration) {}

    fn last_successful_check(&mut self, _ts: &Instant) {}

    fn fallback_invoked(&mut self) {}

    fn fetch_error(&mut self) {}

    fn process_error(&mut self) {}
}