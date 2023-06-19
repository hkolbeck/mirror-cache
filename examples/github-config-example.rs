use std::collections::HashSet;
use std::hash::Hash;
use std::io::{BufReader, Read};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

/**
 * This example highlights how arbitrary objects can be refreshed, including configs.
 *
 * Not every config value is controllable like this of course, but this can allow tuning things
 * like sleep delays and buffer sizes via git push, with no deploy or downtime. If access to GH is
 * lost or an invalid config is pushed, the last good value is used for arbitrarily long with support
 * for alerting that issues have arisen.
 */
struct Config {
    // Must be Sync + Send
    pub read_max_bytes: usize,
    pub sleep_duration: Duration,
}

impl Config {
    pub fn default() -> Config {
        Config {
            read_max_bytes: 1024,
            sleep_duration: Duration::from_millis(100),
        }
    }
}

fn main() {
    let mut shared_service = RwLock::new(MyService::new(DEFAULT_MAX_BYTES));
    let mirror_cache = make_cache(&mut shared_service);
    let config = mirror_cache.cache();

    loop {
        let service = shared_service.read().unwrap();
        service.do_a_buffered_read_operation();

        // get_current() retrieves an Arc of the current version of the config, the returned value
        // is static and should not be held onto. Note that this requires claiming a read lock, but
        // one that's only held by writers for a reference swap
        thread::sleep(config.get_current().sleep_duration)
    }
}

fn make_cache(shared_service: &'static mut RwLock<Arc<MyService>>)
              -> MirrorCache<UpdatingObject<String, Config>> {
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

    let cache = MirrorCache::<Config>::object_builder()
        .with_source(source)
        .with_processor(GHConfigParser)
        .with_fetch_interval(Duration::from_secs(10))
        //If initial fetch fails, use default values
        .with_fallback(FallbackFn::with_value(Config::default()))
        .with_update_callback(OnUpdate::with_fn(|previous, new_version, new_value| {
            // Using an update callback we can reach out into the running parts of the system and
            // update them where possible instead of passing a UpdatingObject<String, Config>
            // everywhere. Version here is the commit SHA.
            match previous {
                None => {
                    println!("Initial fetch got version: '{}' with max bytes: '{}'",
                             new_version, new_version.read_max_bytes);
                    update_service(&mut shared_service, new_version.read_max_bytes)
                }

                Some((maybe_prev_version, prev_value)) => {
                    match maybe_prev_version {
                        Some(prev_version) => {
                            println!("Config version changed: '{}' => '{}'",
                                     prev_version, new_version)
                        }
                        None => {
                            println!("Fetch succeeded after relying on fallback. New version: '{}'",
                                     new_version);
                        }
                    }

                    if prev_value.read_max_bytes != new_value.read_max_bytes {
                        println!("Updating read_max_bytes: '{}' => '{}'",
                                 prev_value.read_max_bytes, new_value.read_max_bytes);
                        update_service(&mut shared_service, new_version.read_max_bytes);
                    }
                }
            }
        }))
        .build().unwrap()
}

struct GHConfigParser;

impl<R: Read> RawConfigProcessor<R, Config> for GHConfigParser {
    fn process(&self, raw: R) -> Result<Config> {
        let config = parse_json_or_toml_or_yaml_or_whatever(raw)?;
        Ok(config)
    }
}