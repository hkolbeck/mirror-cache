use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::UNIX_EPOCH;
use mirror_cache_shared::util::Result;


pub trait ConfigSource<E, S> {
    fn fetch(&self) -> Result<(Option<E>, S)>;
    fn fetch_if_newer(&self, version: &E) -> Result<Option<(Option<E>, S)>>;
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

impl<P: AsRef<Path>> ConfigSource<u128, BufReader<File>> for LocalFileConfigSource<P> {
    fn fetch(&self) -> Result<(Option<u128>, BufReader<File>)> {
        let file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        match metadata.modified() {
            Ok(t) => {
                let mtime = t.duration_since(UNIX_EPOCH)?.as_millis();
                Ok((Some(mtime), BufReader::new(file)))
            }

            //We're on a platform that doesn't support file mtime, unconditional it is.
            Err(_) => Ok((None, BufReader::new(file)))
        }
    }

    fn fetch_if_newer(&self, version: &u128) -> Result<Option<(Option<u128>, BufReader<File>)>> {
        let file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        match metadata.modified() {
            Ok(t) => {
                let mtime = t.duration_since(UNIX_EPOCH)?.as_millis();
                if version < &mtime {
                    Ok(Some((Some(mtime), BufReader::new(file))))
                } else {
                    Ok(None)
                }
            },

            //We're on a platform that doesn't support file mtime, unconditional it is.
            Err(_) => Ok(Some((None, BufReader::new(file))))
        }
    }
}

