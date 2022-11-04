use aws_sdk_s3::Client;
use aws_sdk_s3::types::{ByteStream, DateTime, SdkError};
use reqwest::StatusCode;
use crate::cache::Result;
use crate::sources::ConfigSource;

pub struct S3ConfigSource {
    client: Client,
    bucket: String,
    path: String,
}

impl S3ConfigSource {
    pub fn new<S: Into<String>>(client: Client, bucket: S, path: S) -> S3ConfigSource {
        S3ConfigSource {
            client,
            bucket: bucket.into(),
            path: path.into()
        }
    }
}

impl ConfigSource<DateTime, ByteStream> for S3ConfigSource {
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
            .if_modified_since(*version)
            .send());

        match result {
            Ok(resp) => Ok(Some((resp.last_modified().cloned(), resp.body))),
            Err(SdkError::ServiceError{err, raw}) => {
                if raw.http().status() == StatusCode::NOT_MODIFIED {
                    Ok(None)
                } else {
                    Err(err.into())
                }
            },
            Err(err) => Err(err.into())
        }
    }
}