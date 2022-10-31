Full Dataset Cache
==================

Many datasets are small enough to keep the full collection in memory. For some like accept and
deny lists, this is crucial as a traditional cache would need to save negative results as well.
Others such as per-client overrides might be small enough to store in configs, but keeping them
there necessitates a deploy when they change. Storing them in a database would again require 
caching negative results.

Both cases result in caches often larger than the dataset backing them, while producing a 
bimodal latency distribution as some calls require an external call. This library presents 
a set or map like interface over a structure guaranteed to hold the full backing dataset in
memory, with a background thread polling the backend for changes and atomically swapping them 
in. Should the backing store become unavailable, operation can continue as normal, albeit with
stale values.

This library is best suited to problems where the underlying data changes infrequently and
there is a tolerance for slightly stale values.


Usage
=====

To create a `FullDatasetCache` requires a source implementing `ConfigSource` and a processor
which implements `RawConfigProcessor` that transforms the raw source output into a set or map.

TODO: Build example.