pub use core;

#[cfg(feature = "sync")]
pub use mirror_cache_sync;

#[cfg(feature = "async")]
pub use mirror_cache_async;

// #[cfg(all(feature = "sync", feature = "github"))]
// pub use mirror_cache_sync::sources::github as github;
//
// #[cfg(all(feature = "sync", feature = "http"))]
// pub use mirror_cache_sync::sources::http as http;
//
// #[cfg(all(feature = "sync", feature = "s3"))]
// pub use mirror_cache_sync::sources::s3 as s3;
//
// #[cfg(all(feature = "async", feature = "github"))]
// pub use mirror_cache_async::sources::github as github;
//
// #[cfg(all(feature = "async", feature = "http"))]
// pub use mirror_cache_async::sources::http as http;
//
// #[cfg(all(feature = "async", feature = "s3"))]
// pub use mirror_cache_async::sources::s3 as s3;
