use std::io::Read;
use std::ops::Add;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderValue;
use reqwest::StatusCode;
use crate::cache::{Error, Result};

pub trait ConfigSource<S> {
    fn fetch(&self) -> Result<(u64, S)>;
    fn fetch_if_newer(&self, version: u64) -> Result<Option<(u64, S)>>;
}

pub struct HttpConfigSource {
    client: Client,
    url: String,
}

impl HttpConfigSource {
    pub fn new(client: Client, url: String) -> HttpConfigSource {
        HttpConfigSource {
            client,
            url
        }
    }
}

impl ConfigSource<Response> for HttpConfigSource {
    fn fetch(&self) -> Result<(u64, Response)> {
        let fetched = self.fetch_if_newer(0)?;
        match fetched {
            None => Err(Error::new("Unconditional fetch returned nothing")),
            Some(r) => Ok(r),
        }
    }

    fn fetch_if_newer(&self, version: u64) -> Result<Option<(u64, Response)>> {
        let date = UNIX_EPOCH.add(Duration::from_millis(version));
        let resp = self.client.get(self.url.as_str())
            .header("If-Modified-Since", httpdate::fmt_http_date(date))
            .send()?;

        if resp.status().is_success() {
            let version = if let Some(header) = resp.headers().get("Last-Modified") {
                let date = httpdate::parse_http_date(header.to_str()?)?;
                date.duration_since(UNIX_EPOCH)?.as_millis() as u64
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

pub struct MultiHttpConfigSource {
    sources: Vec<HttpConfigSource>,
}

impl MultiHttpConfigSource {
    pub fn new(client: Client, urls: Vec<String>) -> MultiHttpConfigSource {
        MultiHttpConfigSource {
            sources: urls.into_iter().map(|url| HttpConfigSource::new(client,url)).collect()
        }
    }
}

impl ConfigSource<Response> for MultiHttpConfigSource {
    fn fetch(&self) -> Result<(u64, Response)> {
        for source in self.sources {
            if let Ok(r) = source.fetch() {
                return Ok(r)
            }
        }

        Err(Error::new("All HTTP backends returned Err"))
    }

    fn fetch_if_newer(&self, version: u64) -> Result<Option<(u64, Response)>> {
        for source in self.sources {
            if let Ok(o) = source.fetch_if_newer(version) {
                return Ok(o)
            }
        }

        Err(Error::new("All HTTP backends returned Err"))    }
}
