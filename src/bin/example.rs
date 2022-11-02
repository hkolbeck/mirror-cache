use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use chrono::{DateTime, Utc};
use full_dataset_cache::processors::RawLineMapProcessor;
use full_dataset_cache::sources::LocalFileConfigSource;
use full_dataset_cache::cache::{Error, Fallback, FullDatasetCache, OnFailure, OnUpdate, Result};
use full_dataset_cache::collections::UpdatingMap;
use full_dataset_cache::metrics::Metrics;

fn main() {
    let source = LocalFileConfigSource::new("./src/bin/my.config");
    let processor = RawLineMapProcessor::new(parse_line);

    let cache = FullDatasetCache::<UpdatingMap<String, i32>>::map_builder()
        // These are required.
        .with_source(source)
        .with_processor(processor)
        .with_fetch_interval(Duration::from_secs(2))
        // These are optional
        .with_fallback(0, Fallback::with_value(HashMap::new()))
        .with_update_callback(OnUpdate::with_fn(|_, v, _| println!("Updated to version {}", v)))
        .with_failure_callback(OnFailure::with_fn(|e, _| println!("Failed with error: {}", e)))
        .with_metrics(ExampleMetrics::new())
        .build().unwrap();

    let map = cache.get_collection();
    loop {
        println!("C={}", map.get(&String::from("C")).unwrap_or_default());
        sleep(Duration::from_secs(3));
    }
}

fn parse_line(raw: String) -> Result<Option<(String, i32)>> {
    if raw.trim().is_empty() || raw.starts_with('#') {
        return Ok(None);
    }

    if let Some((k, v)) = raw.split_once('=') {
        Ok(Some((String::from(k), i32::from_str(v)?)))
    } else {
        Err(Error::new(format!("Failed to parse '{}'", raw).as_str()))
    }
}

struct ExampleMetrics {}

impl Metrics for ExampleMetrics {
    fn update(&mut self, _new_version: &u128, fetch_time: Duration, process_time: Duration) {
        println!("Update fetch took {}ms and process took {}ms", fetch_time.as_millis(), process_time.as_millis());
    }

    fn last_successful_update(&mut self, ts: &DateTime<Utc>) {
        println!("Last successful update is now at {}", ts);
    }

    fn check_no_update(&mut self, check_time: &Duration) {
        println!("File hasn't changed. Check in {}ms", check_time.as_millis())
    }

    fn last_successful_check(&mut self, ts: &DateTime<Utc>) {
        println!("Last successful check is now at {}", ts);
    }

    fn fallback_invoked(&mut self) {
        println!("Fallback invoked!");
    }

    fn fetch_error(&mut self, err: &Error) {
        println!("Fetch failed with: '{}'", err)
    }

    fn process_error(&mut self, err: &Error) {
        println!("Process failed with: '{}'", err)
    }
}

impl ExampleMetrics {
    fn new() -> ExampleMetrics {
        ExampleMetrics {}
    }
}