pub mod sources;

#[cfg(feature = github)]
pub mod github;

#[cfg(feature = http)]
pub mod http;

#[cfg(feature = s3)]
pub mod s3;