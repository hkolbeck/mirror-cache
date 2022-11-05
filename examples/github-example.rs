use std::collections::HashMap;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use chrono::{DateTime, Utc};
use octocrab::Octocrab;
use mirror_cache::processors::RawLineMapProcessor;
use mirror_cache::util::{Error, Fallback, OnFailure, OnUpdate, Result};
use mirror_cache::collections::UpdatingMap;
use mirror_cache::github::GitHubConfigSource;
use mirror_cache::metrics::Metrics;
use mirror_cache::cache::MirrorCache;

fn main() {
    let octocrab = Octocrab::builder()
        .personal_token(std::env::var("GITHUB_TOKEN").unwrap())
        .build().unwrap();

    let source = GitHubConfigSource::new(
        octocrab,
        "hkolbeck",
        "config-example",
        "main",
        "my.config",
    ).unwrap();

    let processor = RawLineMapProcessor::new(parse_line);

    let cache = MirrorCache::<UpdatingMap<String, String, i32>>::map_builder()
        // These are required.
        .with_source(source)
        .with_processor(processor)
        .with_fetch_interval(Duration::from_secs(2))
        // These are optional
        .with_name("my-cache")
        .with_fallback(Fallback::with_value(HashMap::new()))
        .with_update_callback(
            OnUpdate::with_fn(
                |_, v, _|
                    println!("Updated to version {}", v.clone().unwrap_or_else(|| String::from("None")))))
        .with_failure_callback(
            OnFailure::with_fn(|e, _| println!("Failed with error: {}", e)))
        .with_metrics(ExampleMetrics::new())
        .build().unwrap();

    let map = cache.cache();
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


impl Metrics<String> for ExampleMetrics {
    fn update(&self, _new_version: &Option<String>, fetch_time: Duration, process_time: Duration) {
        println!("Update fetch took {}ms and process took {}ms", fetch_time.as_millis(), process_time.as_millis());
    }

    fn last_successful_update(&self, ts: &DateTime<Utc>) {
        println!("Last successful update is now at {}", ts);
    }

    fn check_no_update(&self, check_time: &Duration) {
        println!("File hasn't changed. Check in {}ms", check_time.as_millis())
    }

    fn last_successful_check(&self, ts: &DateTime<Utc>) {
        println!("Last successful check is now at {}", ts);
    }

    fn fallback_invoked(&self) {
        println!("Fallback invoked!");
    }

    fn fetch_error(&self, err: &Error) {
        println!("Fetch failed with: '{}'", err)
    }

    fn process_error(&self, err: &Error) {
        println!("Process failed with: '{}'", err)
    }
}


impl ExampleMetrics {
    fn new() -> ExampleMetrics {
        ExampleMetrics {}
    }
}