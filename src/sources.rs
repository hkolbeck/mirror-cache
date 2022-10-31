use std::fs::File;
use std::io::BufReader;
use std::ops::Add;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use reqwest::blocking::{Client, Response};
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

pub struct LocalFileConfigSource<P: AsRef<Path>> {
    path: P,
}

impl<P: AsRef<Path>> LocalFileConfigSource<P> {
    pub fn new(path: P) -> LocalFileConfigSource<P> {
        LocalFileConfigSource {
            path
        }
    }
}

impl<P: AsRef<Path>> ConfigSource<BufReader<File>> for LocalFileConfigSource<P> {
    fn fetch(&self) -> Result<(u64, BufReader<File>)> {
        match self.fetch_if_newer(0)? {
            None => Err(Error::new("Unconditional fetch failed")),
            Some((v, b)) => Ok((v, b))
        }
    }

    fn fetch_if_newer(&self, version: u64) -> Result<Option<(u64, BufReader<File>)>> {
        let file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        match metadata.modified() {
            Ok(t) => {
                let mtime = t.duration_since(UNIX_EPOCH)?.as_millis();
                //If version overflows a u64 fetch_if_newer will be effectively unconditional
                if (version as u128) < mtime {
                    Ok(Some((mtime as u64, BufReader::new(file))))
                } else {
                    Ok(None)
                }
            },

            //We're on a platform that doesn't support file mtime, unconditional it is.
            Err(_) => Ok(Some((0, BufReader::new(file))))
        }

    }
}

