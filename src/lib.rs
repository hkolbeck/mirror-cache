extern crate core;

#[cfg(feature = "tokio-cache")]
pub mod tokio_cache;
#[cfg(feature = "tokio-cache")]
pub mod tokio_sources;

#[cfg(not(feature = "tokio-cache"))]
pub mod cache;
#[cfg(not(feature = "tokio-cache"))]
pub mod sources;

pub mod processors;
pub mod collections;
pub mod metrics;
pub mod util;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "gcs")]
pub mod gcs;

#[cfg(feature = "s3")]
pub mod s3;

#[cfg(feature = "github")]
pub mod github;

