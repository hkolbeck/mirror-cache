use std::io::Cursor;
use octocrab::Octocrab;

#[cfg(not(feature = "tokio-cache"))]
use tokio::runtime::Runtime;
#[cfg(not(feature = "tokio-cache"))]
use crate::sources::ConfigSource;

#[cfg(feature = "tokio-cache")]
use crate::tokio_sources::ConfigSource;
#[cfg(feature = "tokio-cache")]
use async_trait::async_trait;

use crate::util::{Error, Result};

pub struct GitHubConfigSource {
    client: Octocrab,
    owner: String,
    repo: String,
    branch: String,
    path: String,
    #[cfg(not(feature = "tokio-cache"))]
    rt: Runtime,
}

impl GitHubConfigSource {
    pub fn new<S: Into<String>>(octocrab: Octocrab, owner: S, repo: S, branch: S, path: S) -> Result<GitHubConfigSource> {
        Ok(GitHubConfigSource {
            client: octocrab,
            owner: owner.into(),
            repo: repo.into(),
            branch: branch.into(),
            path: path.into(),
            #[cfg(not(feature = "tokio-cache"))]
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        })
    }

}

#[cfg(not(feature = "tokio-cache"))]
impl ConfigSource<String, Cursor<Vec<u8>>> for GitHubConfigSource {
    fn fetch(&self) -> Result<(Option<String>, Cursor<Vec<u8>>)> {
        let handler = self.client.repos(self.owner.clone(), self.repo.clone());
        let content_items = self.rt.block_on(
            handler.get_content()
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
        let handler = self.client.repos(self.owner.clone(), self.repo.clone());
        let commits = self.rt.block_on(
            handler.list_commits()
                .branch(self.branch.clone())
                .path(self.path.clone())
                .send()
        )?;

        if let Some(last_commit) = commits.items.first() {
            if &last_commit.sha == version {
                return Ok(None);
            }
        }

        self.fetch().map(Some)
    }
}

#[cfg(feature = "tokio-cache")]
#[async_trait]
impl ConfigSource<String, Cursor<Vec<u8>>> for GitHubConfigSource {
    async fn fetch(&self) -> Result<(Option<String>, Cursor<Vec<u8>>)> {
        let handler = self.client.repos(self.owner.clone(), self.repo.clone());
        let content_items = handler.get_content()
                .r#ref(self.branch.clone())
                .path(self.path.clone())
                .send().await?;

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

    async fn fetch_if_newer(&self, version: &String) -> Result<Option<(Option<String>, Cursor<Vec<u8>>)>> {
        let handler = self.client.repos(self.owner.clone(), self.repo.clone());
        let commits = handler.list_commits()
                .branch(self.branch.clone())
                .path(self.path.clone())
                .send().await?;

        if let Some(last_commit) = commits.items.first() {
            if &last_commit.sha == version {
                return Ok(None);
            }
        }

        self.fetch().await.map(Some)
    }
}