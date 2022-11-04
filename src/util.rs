use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::result;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;

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
