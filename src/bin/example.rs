use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use full_dataset_cache::processors::RawLineMapProcessor;
use full_dataset_cache::sources::LocalFileConfigSource;
use full_dataset_cache::cache::{Error, FullDatasetCache, Result};
use full_dataset_cache::collections::UpdatingMap;

fn main() {
    let source = LocalFileConfigSource::new(String::from("./src/bin/my.config"));
    let processor = RawLineMapProcessor::new(parse);

    let cache = FullDatasetCache::<UpdatingMap<String, i32>>::new_map(
        String::from("My Config"),
        source,
        processor,
        Duration::from_secs(10)
    ).unwrap();

    let map = cache.get_collection();
    loop {
        println!("C={}", map.get(&String::from("C")).unwrap());
        sleep(Duration::from_secs(10));
    }
}

fn parse(raw: String) -> Result<Option<(String, i32)>> {
    if raw.is_empty() || raw.starts_with('#') {
        return Ok(None);
    }

    if let Some((k, v)) = raw.split_once('=') {
        Ok(Some((String::from(k), i32::from_str(v)?)))
    } else {
        Err(Error::new(format!("Failed to parse '{}'", raw).as_str()))
    }
}