use aws_sdk_s3::Client;
use aws_sdk_s3::error::GetObjectError;
use aws_sdk_s3::output::GetObjectOutput;
use aws_sdk_s3::types::{ByteStream, DateTime, SdkError};
use crate::cache::Result;
use crate::sources::ConfigSource;

pub struct S3ObjectConfigSource {
    client: Client,
    bucket: String,
    path: String,
}

impl S3ObjectConfigSource {
    pub fn new<S: Into<String>>(client: Client, bucket: S, path: S) -> S3ObjectConfigSource {
        S3ObjectConfigSource {
            client,
            bucket: bucket.into(),
            path: path.into()
        }
    }
}

impl ConfigSource<DateTime, ByteStream> for S3ObjectConfigSource {
    fn fetch(&self) -> Result<(Option<DateTime>, ByteStream)> {
        let resp = futures::executor::block_on(self.client.get_object()
            .bucket(self.bucket.clone())
            .key(self.path.clone())
            .send())?;

        Ok((resp.last_modified().cloned(), resp.body))
    }

    fn fetch_if_newer(&self, version: &DateTime) -> Result<Option<(Option<DateTime>, ByteStream)>> {
        let result = futures::executor::block_on(self.client.get_object()
            .bucket(self.bucket.clone())
            .key(self.path.clone())
            .if_modified_since(version.clone())
            .send());

        match result {
            Ok(resp) => Ok(Some((resp.last_modified().cloned(), resp.body))),
            Err(e) => {
                e.
            }
        }


    }
}