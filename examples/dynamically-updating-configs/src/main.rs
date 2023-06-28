use std::io::Cursor;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use arc_swap::ArcSwap;

use mirror_cache::mirror_cache_core::collections::UpdatingObject;
use mirror_cache::mirror_cache_core::processors::RawConfigProcessor;
use mirror_cache::mirror_cache_core::util::{Fallback, FieldUpdate, Result};
use mirror_cache::mirror_cache_sync::cache::MirrorCache;
use mirror_cache::mirror_cache_sync::sources::github::{GitHubConfigSource, Octocrab};

/**
 * This example highlights how arbitrary objects can be refreshed, including configs.
 *
 * Not every config value is controllable like this of course, but this can allow tuning things
 * like sleep delays and buffer sizes via git push, with no deploy or downtime. If access to GH is
 * lost or an invalid config is pushed, the last good value is used for arbitrarily long with
 * support for alerting that issues have arisen.
 */
struct Config {
    read_max_bytes: usize,
    sleep_duration: Duration,
}

impl Config {
    pub fn default() -> Config {
        Config {
            read_max_bytes: 1024,
            sleep_duration: Duration::from_millis(100),
        }
    }

    pub fn get_read_max_bytes(&self) -> usize {
        self.read_max_bytes
    }

    pub fn get_sleep_duration(&self) -> Duration {
        self.sleep_duration
    }
}

fn main() {
    let default_conf = Config::default();
    let service = Arc::new(MyService::new(default_conf.get_read_max_bytes()));
    let sleep = ArcSwap::new(Arc::new(default_conf.get_sleep_duration()));

    let octocrab = Octocrab::builder()
        .personal_token(std::env::var("GITHUB_TOKEN").expect("No Github token specified!"))
        .build().unwrap();

    let source = GitHubConfigSource::new(
        octocrab,
        "repo-owner",
        "repo-name",
        "branch",
        "file-path",
    ).unwrap();

    let cache = MirrorCache::<UpdatingObject<String, Config>>::object_builder()
        .with_source(source)
        .with_processor(GHConfigParser)
        .with_fetch_interval(Duration::from_secs(10))
        //If initial fetch fails, use default values
        .with_fallback(Fallback::with_value(Arc::new(Config::default())))
        .with_update_callback(FieldUpdate::new()
            .add_field(
                |conf: &Arc<Config>| Some(&conf.get_read_max_bytes()),
                |_, maybe_new| {
                    maybe_new.map(|max_bytes|
                        service.update_buffer(*max_bytes)
                    );
                },
            ).add_field(
            |conf: &Arc<Config>| Some(&conf.get_sleep_duration()),
            |_, maybe_new| {
                maybe_new.map(|sleep_dur|
                    sleep.store(Arc::new(*sleep_dur))
                );
            }).build()
        )
        .build().unwrap();

    loop {
        service.do_a_buffered_read_operation();

        thread::sleep(sleep.load_full().as_ref().clone())
    }
}

struct GHConfigParser;

impl RawConfigProcessor<Cursor<Vec<u8>>, Arc<Config>> for GHConfigParser {
    fn process(&self, raw: Cursor<Vec<u8>>) -> Result<Arc<Config>> {
        parse_json_or_toml_or_yaml_or_whatever(raw)
    }
}

fn parse_json_or_toml_or_yaml_or_whatever(raw: Cursor<Vec<u8>>) -> Result<Arc<Config>> {
    todo!()
}

struct MyService {
    read_buf: ArcSwap<Vec<u8>>,
}

impl MyService {
    pub fn new(read_max_bytes: usize) -> MyService {
        MyService {
            read_buf: ArcSwap::new(Arc::new(Vec::with_capacity(read_max_bytes)))
        }
    }

    pub fn update_buffer(&self, len: usize) {
        self.read_buf.store(Arc::new(Vec::with_capacity(len)))
    }

    pub fn do_a_buffered_read_operation(&self) {
        todo!()
    }
}
