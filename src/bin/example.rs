use std::collections::HashMap;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use full_dataset_cache::processors::RawLineMapProcessor;
use full_dataset_cache::sources::LocalFileConfigSource;
use full_dataset_cache::cache::{Error, Fallback, FullDatasetCache, OnFailure, OnUpdate, Result};
use full_dataset_cache::collections::UpdatingMap;

fn main() {
    let source = LocalFileConfigSource::new("my.config");
    let processor = RawLineMapProcessor::new(parse);

    let cache_builder = FullDatasetCache::<UpdatingMap<String, i32>>::map_builder()
        .with_source(source)
        .with_processor(processor)
        .with_fetch_interval(Duration::from_secs(2))
        .with_fallback(0, Fallback::with_value(HashMap::new()))
        .with_update_callback(OnUpdate::with_fn(|_, v, _| println!("Updated to version {}", v)))
        .with_failure_callback(OnFailure::with_fn(|e, _| println!("Failed with error: {}", e)));
    let cache = cache_builder.build().unwrap();

    let map = cache.get_collection();
    loop {
        println!("C={}", map.get(&String::from("C")).unwrap());
        sleep(Duration::from_secs(3));
    }
}

fn parse(raw: String) -> Result<Option<(String, i32)>> {
    if raw.trim().is_empty() || raw.starts_with('#') {
        return Ok(None);
    }

    if let Some((k, v)) = raw.split_once('=') {
        Ok(Some((String::from(k), i32::from_str(v)?)))
    } else {
        Err(Error::new(format!("Failed to parse '{}'", raw).as_str()))
    }
}