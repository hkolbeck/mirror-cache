use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use chrono::DateTime;
use parking_lot::RwLock;
use scheduled_thread_pool::ScheduledThreadPool;
use crate::collections::{UpdatingMap, UpdatingObject, UpdatingSet};
use crate::metrics::Metrics;
use crate::processors::RawConfigProcessor;
use crate::sources::ConfigSource;
use crate::util::{Holder, Result, Error, UpdateFn, FailureFn, FallbackFn, Absent};

pub struct MirrorCache<O> {
    cache: Arc<O>,

    #[allow(dead_code)]
    scheduler: ScheduledThreadPool,
}

impl<O: 'static> MirrorCache<O> {
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
    ) -> Result<MirrorCache<O>> {
        let holder: Holder<E, T> = Arc::new(RwLock::new(Arc::new(None)));
        let update_fn =
            MirrorCache::<O>::get_update_fn(holder.clone(), source, processor);
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
        let cache = Arc::new(constructor(holder.clone()));
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

        Ok(MirrorCache {
            cache,
            scheduler,
        })
    }

    pub fn cache(&self) -> Arc<O> {
        self.cache.clone()
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

    pub fn map_builder<
        K: Eq + Hash + Send + Sync + 'static,
        V: Send + Sync + 'static,
        S: 'static,
        E: Sync + Send + 'static,
        C: ConfigSource<E, S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashMap<K, Arc<V>>> + Send + Sync + 'static,
        D: Into<Duration>,
    >() -> Builder<UpdatingMap<E, K, V>, HashMap<K, Arc<V>>, S, E, C, P, D, Absent, Absent, Absent, Absent> {
        builder(UpdatingMap::new)
    }

    pub fn set_builder<
        V: Eq + Hash + Send + Sync + 'static,
        S: 'static,
        E: Sync + Send + 'static,
        C: ConfigSource<E, S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, HashSet<V>> + Send + Sync + 'static,
        D: Into<Duration>,
    >() -> Builder<UpdatingSet<E, V>, HashSet<V>, S, E, C, P, D, Absent, Absent, Absent, Absent> {
        builder(UpdatingSet::new)
    }

    pub fn object_builder<
        V: Send + Sync + 'static,
        S: 'static,
        E: Sync + Send + 'static,
        C: ConfigSource<E, S> + Send + Sync + 'static,
        P: RawConfigProcessor<S, Arc<V>> + Send + Sync + 'static,
        D: Into<Duration>
    >() -> Builder<UpdatingObject<E, V>, Arc<V>, S, E, C, P, D, Absent, Absent, Absent, Absent>{
        builder(UpdatingObject::new)
    }
}

pub struct Builder<
    O,
    T,
    S,
    E,
    C,
    P,
    D,
    U=Absent,
    F=Absent,
    A=Absent,
    M=Absent,
> {
    constructor: fn(Holder<E, T>) -> O,
    name: Option<String>,
    fetch_interval: Option<D>,
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
    D: Into<Duration> + Send + Sync + 'static,
    U: UpdateFn<T, E> + Send + Sync + 'static,
    F: FailureFn<E> + Send + Sync + 'static,
    A: FallbackFn<T> + 'static,
    M: Metrics<E> + Sync + Send + 'static
> Builder<O, T, S, E, C, P, D, U, F, A, M> {
    pub fn with_name<N: Into<String>>(mut self, name: N) -> Builder<O, T, S, E, C, P, D, U, F, A, M> {
        self.name = Some(name.into());
        self
    }

    pub fn with_source(mut self, source: C) -> Builder<O, T, S, E, C, P, D, U, F, A, M> {
        self.config_source = Some(source);
        self
    }

    pub fn with_processor(mut self, processor: P) -> Builder<O, T, S, E, C, P, D, U, F, A, M> {
        self.config_processor = Some(processor);
        self
    }

    pub fn with_fetch_interval(mut self, fetch_interval: D) -> Builder<O, T, S, E, C, P, D, U, F, A, M> {
        self.fetch_interval = Some(fetch_interval);
        self
    }

    pub fn with_update_callback<UU: UpdateFn<T, E>>(self, callback: UU) -> Builder<O, T, S, E, C, P, D, UU, F, A, M> {
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

    pub fn with_failure_callback<FF: FailureFn<E>>(self, callback: FF) -> Builder<O, T, S, E, C, P, D, U, FF, A, M> {
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

    pub fn with_metrics<MM: Metrics<E>>(self, metrics: MM) -> Builder<O, T, S, E, C, P, D, U, F, A, MM> {
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

    pub fn with_fallback<AA: FallbackFn<T>>(self, fallback: AA) -> Builder<O, T, S, E, C, P, D, U, F, AA, M> {
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

    pub fn build(self) -> Result<MirrorCache<O>> {
        if self.config_source.is_none() {
            return Err(Error::new("No config source specified"));
        }

        if self.config_processor.is_none() {
            return Err(Error::new("No config processor specified"));
        }

        if self.fetch_interval.is_none() {
            return Err(Error::new("No  fetch interval specified"));
        }

        MirrorCache::construct_and_start(
            self.name,
            self.config_source.unwrap(),
            self.config_processor.unwrap(),
            self.fetch_interval.unwrap().into(),
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
    D: Into<Duration>,
>(constructor: fn(Holder<E, T>) -> O) -> Builder<O, T, S, E, C, P, D, Absent, Absent, Absent, Absent> {
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
