use std::time::Duration;
use chrono::{DateTime, Utc};
use crate::cache::Error;

pub trait Metrics<E> {
    fn update(&mut self, new_version: &Option<E>, fetch_time: Duration, process_time: Duration);
    fn last_successful_update(&mut self, ts: &DateTime<Utc>);
    fn check_no_update(&mut self, check_time: &Duration);
    fn last_successful_check(&mut self, ts: &DateTime<Utc>);
    fn fallback_invoked(&mut self);
    fn fetch_error(&mut self, err: &Error);
    fn process_error(&mut self, err: &Error);
}