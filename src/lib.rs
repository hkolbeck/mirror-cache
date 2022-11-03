extern crate core;

pub mod cache;
pub mod sources;
pub mod processors;
pub mod collections;
pub mod metrics;

pub mod s3;
pub mod http;
pub mod gcs;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
