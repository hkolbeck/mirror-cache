use std::time::Duration;
use chrono::{DateTime, Utc};
use crate::util::Error;

pub trait Metrics<E> {
    fn update(&self, new_version: &Option<E>, fetch_time: Duration, process_time: Duration);
    fn last_successful_update(&self, ts: &DateTime<Utc>);
    fn check_no_update(&self, check_time: &Duration);
    fn last_successful_check(&self, ts: &DateTime<Utc>);
    fn fallback_invoked(&self);
    fn fetch_error(&self, err: &Error);
    fn process_error(&self, err: &Error);
}