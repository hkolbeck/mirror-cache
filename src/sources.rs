use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::UNIX_EPOCH;
use crate::cache::Result;

pub trait ConfigSource<S> {
    fn fetch(&self) -> Result<(u128, S)>;
    fn fetch_if_newer(&self, version: &u128) -> Result<Option<(u128, S)>>;
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
    fn fetch(&self) -> Result<(u128, BufReader<File>)> {
        let file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        match metadata.modified() {
            Ok(t) => {
                let mtime = t.duration_since(UNIX_EPOCH)?.as_millis();
                Ok((mtime, BufReader::new(file)))
            }

            //We're on a platform that doesn't support file mtime, unconditional it is.
            Err(_) => Ok((0, BufReader::new(file)))
        }
    }

    fn fetch_if_newer(&self, version: &u128) -> Result<Option<(u128, BufReader<File>)>> {
        let file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        match metadata.modified() {
            Ok(t) => {
                let mtime = t.duration_since(UNIX_EPOCH)?.as_millis();
                if version < &mtime {
                    Ok(Some((mtime, BufReader::new(file))))
                } else {
                    Ok(None)
                }
            },

            //We're on a platform that doesn't support file mtime, unconditional it is.
            Err(_) => Ok(Some((0, BufReader::new(file))))
        }
    }
}

