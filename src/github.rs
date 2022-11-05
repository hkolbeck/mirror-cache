use std::io::Cursor;

#[cfg(not(feature = "tokio-cache"))]
use tokio::runtime::Runtime;
#[cfg(not(feature = "tokio-cache"))]
use crate::sources::ConfigSource;

#[cfg(feature = "tokio-cache")]
use crate::tokio_sources::ConfigSource;
#[cfg(feature = "tokio-cache")]
use async_trait::async_trait;

use reqwest::Client;
use serde_derive::Deserialize;
use crate::util::{Error, Result};

pub struct GitHubConfigSource {
    client: GitHubClient,
    #[cfg(not(feature = "tokio-cache"))]
    rt: Runtime,
}

impl GitHubConfigSource {
    pub fn new<S: Into<String>>(
        client: Client, token: S, owner: S, repo: S, branch: S, path: S,
    ) -> Result<GitHubConfigSource> {
        Ok(GitHubConfigSource {
            client: GitHubClient {
                client,
                token: token.into(),
                owner: owner.into(),
                repo: repo.into(),
                branch: branch.into(),
                path: path.into(),
            },
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
        let (sha, content) = self.rt.block_on(self.client.get_content())?;
        Ok((Some(sha), Cursor::new(content)))
    }

    fn fetch_if_newer(&self, version: &String) -> Result<Option<(Option<String>, Cursor<Vec<u8>>)>> {
        let last_commit = self.rt.block_on(self.client.get_last_commit())?;
        if &last_commit == version {
            Ok(None)
        } else {
            self.fetch().map(Some)
        }
    }
}

#[cfg(feature = "tokio-cache")]
#[async_trait]
impl ConfigSource<String, Cursor<Vec<u8>>> for GitHubConfigSource {
    async fn fetch(&self) -> Result<(Option<String>, Cursor<Vec<u8>>)> {
        let (sha, content) = self.client.get_content().await?;
        Ok((Some(sha), Cursor::new(content)))
    }

    async fn fetch_if_newer(&self, version: &String) -> Result<Option<(Option<String>, Cursor<Vec<u8>>)>> {
        let commits = self.client.get_last_commit().await?;
        if let Some(last_commit) = commits.items.first() {
            if &last_commit.sha == version {
                return Ok(None);
            }
        }

        self.fetch().await.map(Some)
    }
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
}

#[derive(Deserialize)]
struct FileContent {
    pub sha: String,
    pub content: Option<String>,
}

struct GitHubClient {
    client: Client,
    token: String,
    owner: String,
    repo: String,
    branch: String,
    path: String,
}

impl GitHubClient {
    async fn get_last_commit(&self) -> Result<String> {
        let url = format!(
            "repos/{owner}/{repo}/commits?per_page=1&sha={branch}&path={path}",
            owner = self.owner,
            repo = self.repo,
            branch = self.branch,
            path = self.path,
        );

        let body = self.get(url).await?;
        let commits: Vec<Commit> = serde_json::from_str(body.as_str())?;
        if let Some(commit) = commits.first() {
            Ok(commit.sha.clone())
        } else {
            Err(Error::new("No commits returned"))
        }
    }

    async fn get_content(&self) -> Result<(String, Vec<u8>)> {
        let url = format!(
            "repos/{owner}/{repo}/contents/{path}?ref={branch}",
            owner = self.owner,
            repo = self.repo,
            path = self.path,
            branch = self.branch,
        );

        let body = self.get(url).await?;
        let resp: Vec<FileContent> = serde_json::from_str(body.as_str())?;
        if let Some(content) = resp.first() {
            if let Some(encoded_content) = content.content.as_ref() {
                let vec = base64::decode(encoded_content)?;
                Ok((content.sha.clone(), vec))
            } else {
                Ok((content.sha.clone(), vec![]))
            }
        } else {
            Err(Error::new("No content returned by GitHub"))
        }
    }

    async fn get(&self, url: String) -> Result<String> {
        let response = self.client.get(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .send().await?;

        if response.status().is_success() {
            Ok(response.text().await?)
        } else {
            Err(Error::new(format!("Request failed with status: {}", response.status())))
        }
    }
}