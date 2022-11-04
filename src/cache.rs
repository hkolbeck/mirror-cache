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

pub trait UpdateFn<T, E> {
    fn updated(&self, previous: &Option<(Option<E>, T)>, new_version: &Option<E>, new_dataset: &T);
}

pub struct OnUpdate<E, F: Fn(&Option<(Option<E>, T)>, &Option<E>, &T), T> {
    f: F,
    _phantom_t: PhantomData<T>,
    _phantom_e: PhantomData<E>,
}

impl<E, F: Fn(&Option<(Option<E>, T)>, &Option<E>, &T), T> UpdateFn<T, E> for OnUpdate<E, F, T> {
    fn updated(&self, previous: &Option<(Option<E>, T)>, new_version: &Option<E>, new_dataset: &T) {
        (self.f)(previous, new_version, new_dataset)
    }
}

impl<E, F: Fn(&Option<(Option<E>, T)>, &Option<E>, &T), T> OnUpdate<E, F, T> {
    pub fn with_fn(f: F) -> OnUpdate<E, F, T> {
        OnUpdate {
            f,
            _phantom_t: PhantomData::default(),
            _phantom_e: PhantomData::default(),
        }
    }
}

pub trait FailureFn<E> {
    fn failed(&self, err: &Error, last_version_and_ts: Option<(Option<E>, DateTime<Utc>)>);
}

pub struct OnFailure<E, F: Fn(&Error, Option<(Option<E>, DateTime<Utc>)>)> {
    f: F,
    _phantom_e: PhantomData<E>,
}

impl<E, F: Fn(&Error, Option<(Option<E>, DateTime<Utc>)>)> FailureFn<E> for OnFailure<E, F> {
    fn failed(&self, err: &Error, last_version_and_ts: Option<(Option<E>, DateTime<Utc>)>) {
        (self.f)(err, last_version_and_ts)
    }
}

impl<E, F: Fn(&Error, Option<(Option<E>, DateTime<Utc>)>)> OnFailure<E, F> {
    pub fn with_fn(f: F) -> OnFailure<E, F> {
        OnFailure {
            f,
            _phantom_e: PhantomData::default(),
        }
    }
}

pub(crate) type Holder<E, T> = Arc<RwLock<Arc<Option<(Option<E>, T)>>>>;

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
        E: Send + Sync + Clone + 'static,
        C: ConfigSource<E, S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, T> + Send + Sync + 'static,
        U: UpdateFn<T, E> + Send + Sync + 'static,
        F: FailureFn<E> + Send + Sync + 'static,
        A: FallbackFn<T> + 'static,
        M: Metrics<E> + Send + Sync + 'static
    >(
        name: Option<String>, source: C, processor: P, interval: Duration,
        on_update: Option<U>, on_failure: Option<F>, mut metrics: Option<M>,
        fallback: Option<A>, constructor: fn(Holder<E, T>) -> O,
    ) -> Result<FullDatasetCache<O>> {
        let holder: Holder<E, T> = Arc::new(RwLock::new(Arc::new(None)));
        let update_fn =
            FullDatasetCache::<O>::get_update_fn(holder.clone(), source, processor);
        let initial_fetch = update_fn(metrics.as_mut());

        match initial_fetch.as_ref() {
            Err(e) => {
                match fallback {
                    Some(fallback_fun) => {
                        let mut guard = holder.write();
                        *guard = Arc::new(Some((None, fallback_fun.get_fallback())));
                        if let Some(m) = metrics.as_mut() {
                            m.fallback_invoked();
                        }
                    },
                    None => return Err(Error::new(format!("Couldn't complete initial fetch: {}", e).as_str())),
                }
            },
            Ok(init) => {
                match init.as_ref() {
                    None => {
                        match fallback {
                            Some(fallback_fun) => {
                                let mut guard = holder.write();
                                *guard = Arc::new(Some((None, fallback_fun.get_fallback())));
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

        let mut last_success = DateTime::from(SystemTime::now());
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
                    last_success = DateTime::from(SystemTime::now());
                    if let Some(update_callback) = &on_update {
                        update_callback.updated(&previous, v, t)
                    }
                },
                Err(e) => {
                    if let Some(failure_callback) = &on_failure {
                        let last = previous.as_ref().as_ref().map(|(v, _)| (v.clone(), last_success));
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
        E: Clone,
        C: ConfigSource<E, S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, T> + Send + Sync + 'static,
        M: Metrics<E> + Send + Sync + 'static,
    >(
        holder: Holder<E, T>, source: C, processor: P,
    ) -> impl Fn(Option<&mut M>) -> Result<Arc<Option<(Option<E>, T)>>> {
        move |metrics| {
            let version = {
                let guard = holder.read();
                guard.as_ref().as_ref().map(|(v, _)| v.clone())
            };

            let fetch_start = Instant::now();
            let raw_update = match version {
                None | Some(None) => source.fetch().map(Some),
                Some(Some(v)) => source.fetch_if_newer(&v),
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
                        *write_lock = Arc::new(Some((v.clone(), new_coll)));
                        Ok(write_lock.clone())
                    };

                    if let Some(m) = metrics {
                        let now = SystemTime::now();
                        m.last_successful_check(&DateTime::from(now));
                        m.last_successful_update(&DateTime::from(now));
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
        E: Sync + Send + 'static,
        C: ConfigSource<E, S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashMap<K, Arc<V>>> + Send + Sync + 'static,
    >() -> Builder<UpdatingMap<E, K, V>, HashMap<K, Arc<V>>, S, E, C, P, Absent, Absent, Absent, Absent> {
        builder(UpdatingMap::new)
    }

    #[allow(clippy::type_complexity)]
    pub fn set_builder<
        V: Eq + Hash + Send + Sync + 'static,
        S: 'static,
        E: Sync + Send + 'static,
        C: ConfigSource<E, S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashSet<V>> + Send + Sync + 'static,
    >() -> Builder<UpdatingSet<E, V>, HashSet<V>, S, E, C, P, Absent, Absent, Absent, Absent> {
        builder(UpdatingSet::new)
    }
}

pub struct Builder<
    O,
    T,
    S,
    E,
    C,
    P,
    U=Absent,
    F=Absent,
    A=Absent,
    M=Absent,
> {
    constructor: fn(Holder<E, T>) -> O,
    name: Option<String>,
    fetch_interval: Option<Duration>,
    config_source: Option<C>,
    config_processor: Option<P>,
    failure_callback: Option<F>,
    update_callback: Option<U>,
    fallback: Option<A>,
    metrics: Option<M>,
    phantom: PhantomData<S>,
}

impl<
    O: Send + Sync + 'static,
    T: Send + Sync + 'static,
    S: 'static,
    E: Send + Sync + Clone + 'static,
    C: ConfigSource<E, S> + Send + Sync + 'static,
    P: RawConfigProcessor<S, T> + Send + Sync + 'static,
    U: UpdateFn<T, E> + Send + Sync + 'static,
    F: FailureFn<E> + Send + Sync + 'static,
    A: FallbackFn<T> + 'static,
    M: Metrics<E> + Sync + Send + 'static
> Builder<O, T, S, E, C, P, U, F, A, M> {
    pub fn with_name<N: Into<String>>(mut self, name: N) -> Builder<O, T, S, E, C, P, U, F, A, M> {
        self.name = Some(name.into());
        self
    }

    pub fn with_source(mut self, source: C) -> Builder<O, T, S, E, C, P, U, F, A, M> {
        self.config_source = Some(source);
        self
    }

    pub fn with_processor(mut self, processor: P) -> Builder<O, T, S, E, C, P, U, F, A, M> {
        self.config_processor = Some(processor);
        self
    }

    pub fn with_fetch_interval(mut self, fetch_interval: Duration) -> Builder<O, T, S, E, C, P, U, F, A, M> {
        self.fetch_interval = Some(fetch_interval);
        self
    }

    pub fn with_update_callback<UU: UpdateFn<T, E>>(self, callback: UU) -> Builder<O, T, S, E, C, P, UU, F, A, M> {
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
            phantom: PhantomData::default()
        }
    }

    pub fn with_failure_callback<FF: FailureFn<E>>(self, callback: FF) -> Builder<O, T, S, E, C, P, U, FF, A, M> {
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
            phantom: PhantomData::default()
        }
    }

    pub fn with_metrics<MM: Metrics<E>>(self, metrics: MM) -> Builder<O, T, S, E, C, P, U, F, A, MM> {
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
            phantom: PhantomData::default()
        }
    }

    pub fn with_fallback<AA: FallbackFn<T>>(self, fallback: AA) -> Builder<O, T, S, E, C, P, U, F, AA, M> {
        Builder {
            constructor: self.constructor,
            name: self.name,
            fetch_interval: self.fetch_interval,
            config_source: self.config_source,
            config_processor: self.config_processor,
            failure_callback: self.failure_callback,
            update_callback: self.update_callback,
            fallback: Some(fallback),
            metrics: self.metrics,
            phantom: PhantomData::default()
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
    E,
    C: ConfigSource<E, S> + Send + Sync + 'static,
    P: RawConfigProcessor<S, T> + Send + Sync + 'static,
>(constructor: fn(Holder<E, T>) -> O) -> Builder<O, T, S, E, C, P, Absent, Absent, Absent, Absent> {
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
        phantom: PhantomData::default()
    }
}

pub struct Absent {}

impl<E, T> UpdateFn<T, E> for Absent {
    fn updated(&self, _previous: &Option<(Option<E>, T)>, _new_version: &Option<E>, _new_dataset: &T) {
        panic!("Should never be called");
    }
}

impl<T> FallbackFn<T> for Absent {
    fn get_fallback(self) -> T {
        panic!("Should never be called");
    }
}

impl<E> FailureFn<E> for Absent {
    fn failed(&self, _err: &Error, _last_version_and_ts: Option<(Option<E>, DateTime<Utc>)>) {
        panic!("Should never be called");
    }
}

impl<E> Metrics<E> for Absent {
    fn update(&mut self, _new_version: &Option<E>, _fetch_time: Duration, _process_time: Duration) {
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