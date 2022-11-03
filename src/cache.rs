use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;
use std::result;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
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
    fn get_fallback(self) -> T;
}

pub struct Fallback<T> {
    t: T
}

impl<T> FallbackFn<T> for Fallback<T> {
    fn get_fallback(self) -> T {
        self.t
    }
}

impl<T> Fallback<T> {
    pub fn with_value(t: T) -> Fallback<T> {
        Fallback {
            t
        }
    }
}

pub trait UpdateFn<T> {
    fn updated(&self, previous: &Option<(u128, T)>, new_version: &u128, new_dataset: &T);
}

pub struct OnUpdate<F: Fn(&Option<(u128, T)>, &u128, &T), T> {
    f: F,
    _phantom: PhantomData<T>
}

impl<F: Fn(&Option<(u128, T)>, &u128, &T), T> UpdateFn<T> for OnUpdate<F, T> {
    fn updated(&self, previous: &Option<(u128, T)>, new_version: &u128, new_dataset: &T) {
        (self.f)(previous, new_version, new_dataset)
    }
}

impl<F: Fn(&Option<(u128, T)>, &u128, &T), T> OnUpdate<F, T> {
    pub fn with_fn(f: F) -> OnUpdate<F, T> {
        OnUpdate {
            f,
            _phantom: PhantomData::default()
        }
    }
}

pub trait FailureFn {
    fn failed(&self, err: &Error, last_version_and_ts: Option<(u128, Instant)>);
}

pub struct OnFailure<F: Fn(&Error, Option<(u128, Instant)>)> {
    f: F
}

impl<F: Fn(&Error, Option<(u128, Instant)>)> FailureFn for OnFailure<F> {
    fn failed(&self, err: &Error, last_version_and_ts: Option<(u128, Instant)>) {
        (self.f)(err, last_version_and_ts)
    }
}

impl<F: Fn(&Error, Option<(u128, Instant)>)> OnFailure<F> {
    pub fn with_fn(f: F) -> OnFailure<F> {
        OnFailure {
            f
        }
    }
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
        let initial_fetch = update_fn(metrics.as_mut());

        match initial_fetch.as_ref() {
            Err(e) => {
                match fallback {
                    Some((v, fallback_fun)) => {
                        let mut guard = holder.write();
                        *guard = Arc::new(Some((v, fallback_fun.get_fallback())));
                        if let Some(m) = metrics.as_mut() {
                            m.fallback_invoked();
                        }
                    },
                    None => return Err(Error::new(format!("Couldn't complete initial fetch: {}", e).as_str())),
                }
            }
            Ok(init) => {
                match init.as_ref() {
                    None => {
                        match fallback {
                            Some((v, fallback_fun)) => {
                                let mut guard = holder.write();
                                *guard = Arc::new(Some((v, fallback_fun.get_fallback())));
                                if let Some(m) = metrics.as_mut() {
                                    m.fallback_invoked();
                                }
                            },
                            None => return Err(Error::new("Initial fetch should be unconditional but failed and no fallback specified")),
                        }
                    }
                    Some((v, s)) => {
                        if let Some(update_callback) = on_update.borrow() {
                            update_callback.updated(&None, v, s);
                        }
                    }
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
                holder.read().clone()
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
                holder.read().as_ref().as_ref().map(|(v, _)| *v)
            };

            let fetch_start = Instant::now();
            let raw_update = match version {
                None => source.fetch().map(Some),
                Some(v) => source.fetch_if_newer(&v),
            };
            let fetch_time = Instant::now().duration_since(fetch_start);

            let process_start = Instant::now();
            let update = match raw_update {
                Ok(None) => None,
                Ok(Some((v, s))) => Some((v, processor.process(s))),
                Err(e) => {
                    if let Some(m) = metrics {
                        m.fetch_error(&e)
                    }
                    return Err(e);
                }
            };
            let process_time = Instant::now().duration_since(process_start);

            match update {
                Some((v, Ok(new_coll))) => {
                    let ret = {
                        let mut write_lock = holder.write();
                        *write_lock = Arc::new(Some((v, new_coll)));
                        Ok(write_lock.clone())
                    };

                    if let Some(m) = metrics {
                        m.last_successful_update(&DateTime::from(SystemTime::now()));
                        m.update(&v, fetch_time, process_time);
                    };

                    ret
                }
                Some((_, Err(e))) => {
                    if let Some(m) = metrics {
                        m.process_error(&e)
                    }
                    Err(e)
                }
                None => {
                    if let Some(m) = metrics {
                        m.last_successful_check(&DateTime::from(SystemTime::now()));
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
    >() -> Builder<UpdatingMap<K, V>, HashMap<K, Arc<V>>, S, C, P, Absent, Absent, Absent, Absent> {
        builder(UpdatingMap::new)
    }

    pub fn set_builder<
        V: Eq + Hash + Send + Sync + 'static,
        S: 'static,
        C: ConfigSource<S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashSet<V>> + Send + Sync + 'static,
    >() -> Builder<UpdatingSet<V>, HashSet<V>, S, C, P, Absent, Absent, Absent, Absent> {
        builder(UpdatingSet::new)
    }
}

pub struct Builder<
    O,
    T,
    S,
    C,
    P,
    U=Absent,
    F=Absent,
    A=Absent,
    M=Absent,
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
    pub fn with_name<N: Into<String>>(mut self, name: N) -> Builder<O, T, S, C, P, U, F, A, M> {
        self.name = Some(name.into());
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

    pub fn with_update_callback<UU: UpdateFn<T>>(self, callback: UU) -> Builder<O, T, S, C, P, UU, F, A, M> {
        Builder {
            constructor: self.constructor,
            name: self.name,
            fetch_interval: self.fetch_interval,
            config_source: self.config_source,
            config_processor: self.config_processor,
            failure_callback: self.failure_callback,
            update_callback: Some(callback),
            fallback: self.fallback,
            metrics: self.metrics,
            phantom: Default::default()
        }
    }

    pub fn with_failure_callback<FF: FailureFn>(self, callback: FF) -> Builder<O, T, S, C, P, U, FF, A, M> {
        Builder {
            constructor: self.constructor,
            name: self.name,
            fetch_interval: self.fetch_interval,
            config_source: self.config_source,
            config_processor: self.config_processor,
            failure_callback: Some(callback),
            update_callback: self.update_callback,
            fallback: self.fallback,
            metrics: self.metrics,
            phantom: Default::default()
        }
    }

    pub fn with_metrics<MM: Metrics>(self, metrics: MM) -> Builder<O, T, S, C, P, U, F, A, MM> {
        Builder {
            constructor: self.constructor,
            name: self.name,
            fetch_interval: self.fetch_interval,
            config_source: self.config_source,
            config_processor: self.config_processor,
            failure_callback: self.failure_callback,
            update_callback: self.update_callback,
            fallback: self.fallback,
            metrics: Some(metrics),
            phantom: Default::default()
        }
    }

    pub fn with_fallback<AA: FallbackFn<T>>(self, fallback_version: u128, fallback: AA) -> Builder<O, T, S, C, P, U, F, AA, M> {
        Builder {
            constructor: self.constructor,
            name: self.name,
            fetch_interval: self.fetch_interval,
            config_source: self.config_source,
            config_processor: self.config_processor,
            failure_callback: self.failure_callback,
            update_callback: self.update_callback,
            fallback: Some((fallback_version, fallback)),
            metrics: self.metrics,
            phantom: Default::default()
        }
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
>(constructor: fn(Holder<T>) -> O) -> Builder<O, T, S, C, P, Absent, Absent, Absent, Absent> {
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
    fn get_fallback(self) -> T {
        panic!("Should never be called");
    }
}

impl FailureFn for Absent {
    fn failed(&self, _err: &Error, _last_version_and_ts: Option<(u128, Instant)>) {
        panic!("Should never be called");
    }
}

impl Metrics for Absent {
    fn update(&mut self, _new_version: &u128, _fetch_time: Duration, _process_time: Duration) {
        panic!("Should never be called");
    }

    fn last_successful_update(&mut self, _ts: &DateTime<Utc>) {
        panic!("Should never be called");
    }

    fn check_no_update(&mut self, _check_time: &Duration) {
        panic!("Should never be called");
    }

    fn last_successful_check(&mut self, _ts: &DateTime<Utc>) {
        panic!("Should never be called");
    }

    fn fallback_invoked(&mut self) {
        panic!("Should never be called");
    }

    fn fetch_error(&mut self, _err: &Error) {
        panic!("Should never be called");
    }

    fn process_error(&mut self, _err: &Error) {
        panic!("Should never be called");
    }
}