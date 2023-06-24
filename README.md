Mirror Cache
============

### Status: In Progress. Everything described below is implemented targetting both sync and async 
### usage, currently resolving issues with annotations around the sync/async split

Many datasets are small enough to keep the full collection in memory. For some like accept and
deny lists, this is crucial as a traditional cache would need to save negative results as well.
Others such as per-client overrides might be small enough to store in configs, but keeping them
there necessitates a deploy when they change. Storing them in a database would again require
caching negative results.

Both cases result in caches often larger than the dataset backing them, while producing a
bimodal latency distribution as some calls require an external lookup. This library presents
a set, map, or user-defined interface over a structure guaranteed to hold the full backing
dataset in memory, with a background thread polling the backend for changes and atomically
swapping them in. Should the backing store become unavailable, operation can continue as
normal, albeit with stale values.

This library is best suited to problems where the underlying data changes infrequently and
there is a tolerance for slightly stale values, similar to any caching use-case.

//TODO: Arbitrary https client support

//TODO: A separate project: A proxy server that allows only a few instances to maintain data 
//TODO: direct from the source and serves the stored data out for usage by service instances 

Usage
=====

Cache instances are constructed using a builder, which is retrieved by calling
`MirrorCache::<UpdatingMap<$Version, $Key, $Value>>::map_builder()`,
`MirrorCache::<UpdatingSet<$Version, $Value>>::set_builder()`, or
`MirrorCache::<UpdatingObject<$Version, $Value>>::object_builder()`
depending on the desired collection type. Thanks to rust's type checker, your code 
won't compile if required fields are unset. See the appropriate section below for 
more details on each of the builder functions.

```rust
fn main() -> FullDatasetCache<UpdatingMap<K, V>> {
    let source = LocalFileConfigSource::new("my.config");
    let processor = RawLineMapProcessor::new(|line| { /* Parsing! */ });

    MirrorCache::<UpdatingMap<VersionType, KeyType, ValueType>>::map_builder()
        // These are required. Failing to specify any of these will cause type-checker errors.
        .with_source(source)
        .with_processor(processor)
        .with_fetch_interval(Duration::from_secs(10))
        // These are optional
        .with_name("my-cache")
        .with_fallback(Fallback::with_value(HashMap::new()))
        .with_update_callback(OnUpdate::with_fn(|_, v, _| println!("Updated to version {}", v)))
        .with_failure_callback(OnFailure::with_fn(|e, _| println!("Failed with error: {}", e)))
        .with_metrics(ExampleMetrics::new())
        .build().unwrap();
}
```

Sources
=======

While users may implement their own, a number of sources are provided:

- `LocalFileConfigSource` exposes a file on the local file system, provided with core library.
- `HttpConfigSource` wraps a [reqwest](https://github.com/seanmonstar/reqwest) client and
  fetches data over the network via HTTP(S). Requires `features = ["http"]`.
- `S3ConfigSource` exposes an object in S3. Requires `features = ["s3"]`.
- `GitHubConfigSource` exposes a file on GitHub. Requires `features = ["github"]`.

Suggestions for other sources are welcome. Google Cloud Storage is not included due to a 
dependency conflict with the GitHub client used. Ideally, backends will
support some get-if-newer functionality. Those that don't can still be used, but
implementations will have to issue an unconditional fetch every time and care should be
taken when choosing the fetch interval.


Processors
==========

Two processors are provided in the core library, both consume a Reader and pass lines to a
specified parse function to be transformed into values or (key, value) tuples depending on
whether a set or map is being constructed. The set values and map keys must be `Eq + Hash`,
and they as well as the map values must be `Send + Sync`.

When implementing your parse function, remember to allow blank lines and comments. It's also
recommended to be forgiving about whitespace. It can return `Ok(None)` to indicate that
nothing is wrong with the stream, the line in question just didn't translate to an entry in
the collection.


Name
====

Name is only passed to the thread scheduler for use as a thread label component. Not present in
async version.


Callbacks
=========

Two callbacks may be specified, one to be invoked any time a new backing datasource state
is swapped in, and one to be invoked any time a fetch or process fails. If nothing else,
it's recommended to at least log the occurrence of any errors, but logging the fact that
the dataset has updated can help with debugging.

The callback traits may be implemented directly, or `OnUpdate::with_fn()` and
`OnFailure::with_fn()` convenience methods are provided, both will accept a closure or
anything implementing the appropriate `Fn` type.


Fallback
========

In order to make useful guarantees, the cache must complete an initial fetch when `build()`
is invoked. By default, if that fetch fails, `build()` will return an error. If a fallback
is provided, it will be used until a successful fetch can be completed. This can be
important, as a backing data source going unavailable can cause new service instances to
not come up if they just `unwrap()` after `build()`.


Metrics
=======

It's optional to collect metrics, but it's strongly recommended to do so if running this code
in production. Particular care should be given to `last_successful_check()`, as it exposes how
stale the data might be. It's recommended to alert on this value if it exceeds tolerable
staleness.

See [metrics.rs](shared/src/metrics.rs) for other metrics that can be collected.


Demonstration
=============

The following is a log of [the provided example](examples/local-example.rs), edited with comments.
The example sets up a cache backed by a local file of `key=value` pairs, where `value` is an
i32, then loops forever printing the value of the key `C`. It's very noisy, as the example
metrics implementation just calls `println!()`.

```
Fallback invoked! #Initial fetch failed, fallback value of an empty map used
C=0 #Example defaults to 0 if key isn't in map
Last successful update is now at 2022-11-03 00:46:09.121312 UTC #Successful fetch completed
Update fetch took 0ms and process took 0ms
Updated to version 1667435391751
C=3
Last successful check is now at 2022-11-03 00:46:11.122168 UTC
File hasn't changed. Check in 0ms
C=3
Last successful update is now at 2022-11-03 00:46:17.126346 UTC #File updated with new value
Update fetch took 0ms and process took 0ms
Updated to version 1667436376483
C=2
Last successful check is now at 2022-11-03 00:46:21.124309 UTC
File hasn't changed. Check in 0ms
C=2
Process failed with: 'invalid digit found in string' #'2' in configs replaced with 'five'
Failed with error: invalid digit found in string
C=2 #Client code is still able to read the last known value
Last successful update is now at 2022-11-03 00:46:31.124433 UTC #Config fixed
Update fetch took 0ms and process took 0ms
Updated to version 1667436390340
C=5
Last successful check is now at 2022-11-03 00:46:33.125560 UTC
File hasn't changed. Check in 0ms
C=5
Fetch failed with: 'No such file or directory (os error 2)' #File moved or deleted
Failed with error: No such file or directory (os error 2)
C=5 #Client code is still able to read the last known value
Last successful update is now at 2022-11-03 00:46:45.125176 UTC #File restored
Update fetch took 0ms and process took 0ms
Updated to version 1667436404799
C=5
```
