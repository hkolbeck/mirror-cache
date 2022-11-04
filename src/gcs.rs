use std::io::Cursor;
use chrono::{DateTime, Utc};
use tokio::runtime::Runtime;
use google_cloud_storage::client::Client;
use google_cloud_storage::http::objects::download::Range;
use google_cloud_storage::http::objects::get::GetObjectRequest;
use crate::sources::ConfigSource;

pub struct GcsConfigSource {
    client: Client,
    bucket: String,
    object: String,
    rt: Runtime,
}

impl GcsConfigSource {
    pub fn new<S: Into<String>>(client: Client, bucket: S, object: S) -> Result<GcsConfigSource> {
        Ok(GcsConfigSource {
            client,
            bucket: bucket.into(),
            object: object.into(),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
        })
    }
}

impl ConfigSource<DateTime<Utc>, Cursor<Vec<u8>>> for GcsConfigSource {
    fn fetch(&self) -> crate::cache::Result<(Option<DateTime<Utc>>, Cursor<Vec<u8>>)> {
        let request = GetObjectRequest {
            bucket: self.bucket.clone(),
            object: self.object.clone(),
            ..Default::default()
        };

        // Object might be updated between the call for the mtime and the actual download, but
        // the next conditional fetch will just re-fetch the same data and get the new version.
        let version = self.rt.block_on(
            self.client.get_object(&request, None)
        )?.updated;

        let object = futures::executor::block_on(
            self.client.download_object(&request,&Range::default(),None)
        )?;

        Ok((version, Cursor::new(object)))
    }

    fn fetch_if_newer(&self, version: &DateTime<Utc>) -> crate::cache::Result<Option<(Option<DateTime<Utc>>, Cursor<Vec<u8>>)>> {
        let request = GetObjectRequest {
            bucket: self.bucket.clone(),
            object: self.object.clone(),
            ..Default::default()
        };

        // Object might be updated between the call for the mtime and the actual download, but
        // the next conditional fetch will just re-fetch the same data and get the new version.
        let maybe_new_version = self.rt.block_on(
            self.client.get_object(&request, None)
        )?.updated;

        if let Some(new_version) = maybe_new_version {
            if &new_version == version {
                return Ok(None)
            }
        }

        let object = futures::executor::block_on(
            self.client.download_object(&request,&Range::default(),None)
        )?;

        Ok(Some((maybe_new_version, Cursor::new(object))))
    }
}