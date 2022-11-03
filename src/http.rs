use std::ops::Add;
use std::time::{Duration, UNIX_EPOCH};
use reqwest::blocking::{Client, Response};
use reqwest::StatusCode;
use crate::cache::{Error, Result};
use crate::sources::ConfigSource;

pub struct HttpConfigSource {
    client: Client,
    url: String,
}

impl HttpConfigSource {
    pub fn new(client: Client, url: String) -> HttpConfigSource {
        HttpConfigSource {
            client,
            url,
        }
    }
}

impl ConfigSource<Response> for HttpConfigSource {
    fn fetch(&self) -> Result<(u128, Response)> {
        let resp = self.client.get(self.url.as_str()).send()?;

        if resp.status().is_success() {
            let version = if let Some(header) = resp.headers().get("Last-Modified") {
                let date = httpdate::parse_http_date(header.to_str()?)?;
                date.duration_since(UNIX_EPOCH)?.as_millis()
            } else {
                0
            };

            Ok((version, resp))
        } else {
            Err(Error::new(format!("Fetch failed. Status: {}", resp.status().as_str()).as_str()))
        }
    }

    fn fetch_if_newer(&self, version: &u128) -> Result<Option<(u128, Response)>> {
        let date = UNIX_EPOCH.add(Duration::from_millis(*version as u64));
        let resp = self.client.get(self.url.as_str())
            .header("If-Modified-Since", httpdate::fmt_http_date(date))
            .send()?;

        if resp.status().is_success() {
            let version = if let Some(header) = resp.headers().get("Last-Modified") {
                let date = httpdate::parse_http_date(header.to_str()?)?;
                date.duration_since(UNIX_EPOCH)?.as_millis()
            } else {
                0
            };

            Ok(Some((version, resp)))
        } else if resp.status() == StatusCode::NOT_MODIFIED {
            Ok(None)
        } else {
            Err(Error::new(format!("Fetch failed. Status: {}", resp.status().as_str()).as_str()))
        }
    }
}
