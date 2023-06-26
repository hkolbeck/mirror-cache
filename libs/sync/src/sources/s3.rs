use aws_sdk_s3::Client;
use aws_sdk_s3::types::{ByteStream, DateTime, SdkError};
use aws_smithy_http::result::ServiceError;
use tokio::runtime::Runtime;
use mirror_cache_shared::util::Result;
use crate::sources::ConfigSource;

pub struct S3ConfigSource {
    client: Client,
    bucket: String,
    path: String,
    rt: Runtime,
}

impl S3ConfigSource {
    pub fn new<S: Into<String>>(client: Client, bucket: S, path: S) -> Result<S3ConfigSource> {
        Ok(S3ConfigSource {
            client,
            bucket: bucket.into(),
            path: path.into(),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
        })
    }
}

impl ConfigSource<DateTime, ByteStream> for S3ConfigSource {
    fn fetch(&self) -> Result<(Option<DateTime>, ByteStream)> {
        let resp = self.rt.block_on(self.client.get_object()
            .bucket(self.bucket.clone())
            .key(self.path.clone())
            .send())?;

        Ok((resp.last_modified().cloned(), resp.body))
    }

    fn fetch_if_newer(&self, version: &DateTime) -> Result<Option<(Option<DateTime>, ByteStream)>> {
        let result = self.rt.block_on(self.client.get_object()
            .bucket(self.bucket.clone())
            .key(self.path.clone())
            .if_modified_since(*version)
            .send());

        match result {
            Ok(resp) => Ok(Some((resp.last_modified().cloned(), resp.body))),
            Err(SdkError::ServiceError(ServiceError{source, raw})) => {
                if raw.http().status() == 304 {
                    Ok(None)
                } else {
                    Err(source.into())
                }
            },
            Err(err) => Err(err.into())
        }
    }
}