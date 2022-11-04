use std::io::Cursor;
use octocrab::repos::RepoHandler;
use crate::sources::ConfigSource;
use crate::cache::{Error, Result};

pub struct GitHubConfigSource {
    repo: RepoHandler<'static>,
    branch: String,
    path: String,
}

impl ConfigSource<String, Cursor<Vec<u8>>> for GitHubConfigSource {
    fn fetch(&self) -> Result<(Option<String>, Cursor<Vec<u8>>)> {
        let content_items = futures::executor::block_on(
            self.repo.get_content()
                .r#ref(self.branch.clone())
                .path(self.path.clone())
                .send()
        )?;

        if let Some(content_wrapper) = content_items.items.first() {
            if let Some(raw_content) = content_wrapper.decoded_content() {
                Ok((Some(content_wrapper.sha.clone()), Cursor::new(raw_content.into())))
            } else {
                Err(Error::new("File had no content, or it failed to decode"))
            }
        } else {
            Err(Error::new("File not found"))
        }
    }

    fn fetch_if_newer(&self, version: &String) -> Result<Option<(Option<String>, Cursor<Vec<u8>>)>> {
        let commits = futures::executor::block_on(
            self.repo.list_commits()
                .branch(self.branch.clone())
                .path(self.path.clone())
                .send()
        )?;

        if let Some(last_commit) = commits.items.first() {
            if &last_commit.sha == version {
                return Ok(None)
            }
        }

        self.fetch().map(Some)
    }
}