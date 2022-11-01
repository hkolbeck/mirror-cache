use std::time::{Duration, Instant};

pub trait Metrics {
    fn update(&mut self, new_version: &u128, fetch_time: Duration, process_time: Duration);
    fn last_successful_update(&mut self, ts: Instant);
    fn check_no_update(&mut self, check_time: &Duration);
    fn last_successful_check(&mut self, ts: &Instant);
    fn fallback_invoked(&mut self);
    fn fetch_error(&mut self);
    fn process_error(&mut self);
}