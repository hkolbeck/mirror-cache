use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use full_dataset_cache::processors::RawLineMapProcessor;
use full_dataset_cache::sources::LocalFileConfigSource;
use full_dataset_cache::cache::{Error, FullDatasetCache, Result};
use full_dataset_cache::collections::UpdatingMap;

fn main() {
    let source = LocalFileConfigSource::new("./src/bin/my.config");
    let processor = RawLineMapProcessor::new(parse);

    let mut cache_builder = FullDatasetCache::<UpdatingMap<String, i32>>::map_builder();
    cache_builder = cache_builder.with_source(source);
    cache_builder = cache_builder.with_processor(processor);
    cache_builder = cache_builder.with_fetch_interval(Duration::from_secs(10));
    let cache = cache_builder.build().unwrap();

    let map = cache.get_collection();
    loop {
        println!("C={}", map.get(&String::from("C")).unwrap());
        sleep(Duration::from_secs(10));
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