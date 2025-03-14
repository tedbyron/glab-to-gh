//! GitLab API types and functions.

use std::sync::Arc;

use anyhow::{anyhow, bail, Error, Result};
use gitlab::{
    api::{common::AccessLevel, groups, projects, users, AsyncQuery, Pagination},
    AsyncGitlab, VisibilityLevel,
};
use serde::Deserialize;
use tokio::task::JoinSet;
use tracing::{instrument, Level};

use crate::cli::{PathOrUrl, SourceRepoOrNamespace};

/// GitLab project.
#[derive(Debug, Deserialize)]
pub struct Project {
    pub description: Option<String>,
    pub default_branch: Option<String>,
    pub path_with_namespace: String,
    pub archived: bool,
    pub visibility: VisibilityLevel,
    pub http_url_to_repo: String,
    pub statistics: Option<ProjectStatistics>,
}

/// GitLab project statistics.
#[derive(Debug, Deserialize)]
pub struct ProjectStatistics {
    pub storage_size: u64,
    pub repository_size: u64,
    pub lfs_objects_size: u64,
}

/// GitLab user.
#[derive(Debug, Deserialize)]
pub struct User {
    pub username: String,
    pub name: String,
    pub id: u64,
}

/// Query for a GitLab project.
#[instrument(skip_all)]
pub async fn project(
    client: &AsyncGitlab,
    host: impl AsRef<str> + Send,
    project: PathOrUrl,
) -> Result<Project> {
    let host = host.as_ref();
    let path = match &project {
        PathOrUrl::Path(path) => path,
        PathOrUrl::Url(url) => {
            // Find the path after the host e.g., the path `user/project` within the URL
            // `https://gitlab.com/user/project`.
            let path_start = url
                .find(host)
                .ok_or_else(|| anyhow!("Repo URL: {url} doesn't contain hostname: {host}"))?
                + host.len()
                + 1;

            url.get(path_start..)
                .ok_or_else(|| anyhow!("Missing path after the repo URL: {url}"))?
                .trim_end_matches(".git")
        }
    };
    let endpoint = projects::Project::builder().project(path).build()?;

    endpoint
        .query_async(client)
        .await
        .map_err(|e| anyhow!("Couldn't find the GitLab project: {path}: {e}"))
}

/// Get the currently authenticated GitLab user.
#[instrument(skip_all, ret(level = Level::DEBUG))]
pub async fn user(client: &AsyncGitlab) -> Result<User> {
    let endpoint = users::CurrentUser::builder().build()?;
    endpoint
        .query_async(client)
        .await
        .map_err(|e| anyhow!("Couldn't get the currently authenticated GitLab user: {e}"))
}

/// Query for GitLab projects.
#[instrument(skip_all, ret(level = Level::DEBUG))]
pub async fn projects(
    client: Arc<AsyncGitlab>,
    user: &User,
    host: String,
    source: SourceRepoOrNamespace,
) -> Result<Vec<Project>> {
    // Check whether a GitLab project or namespace was specified.
    if let Some(repos) = source.repo {
        // Individual projects were specified. If there are multiple, spawn a task for each project
        // to query the projects endpoint.
        if repos.len() > 1 {
            let mut set = JoinSet::new();
            let mut projects = Vec::with_capacity(repos.len());

            for repo in repos {
                let client = Arc::clone(&client);
                let host = host.clone();
                let repo = repo.clone();

                set.spawn(async move { project(&client, host, repo).await });
            }

            while let Some(res) = set.join_next().await {
                // If any request fails, prompt the user to continue.
                match res.map_err(Error::from) {
                    Ok(Ok(project)) => projects.push(project),
                    Ok(Err(e)) | Err(e) => {
                        if let Err(e) = crate::prompt_continue(e) {
                            set.shutdown().await;
                            bail!(e);
                        }
                    }
                }
            }

            if projects.is_empty() {
                bail!("No GitLab projects found");
            }

            Ok(projects)
        } else {
            Ok(vec![project(&client, host, repos[0].clone()).await?])
        }
    } else {
        // A namespace was specified, determine which type it is and query the appropriate endpoint.
        match (source.source_user, source.group) {
            (true, _) => {
                let endpoint = users::UserProjects::builder().user(user.id).build()?;

                gitlab::api::paged(endpoint, Pagination::All)
                    .query_async(&*client)
                    .await
                    .map_err(|e| {
                        anyhow!("Couldn't find GitLab projects for user: {}: {e}", user.name)
                    })
            }
            (_, Some(group)) if !group.is_empty() => {
                let endpoint = groups::projects::GroupProjects::builder()
                    .group(&*group)
                    .include_subgroups(true)
                    .min_access_level(AccessLevel::Developer)
                    .build()?;

                gitlab::api::paged(endpoint, Pagination::All)
                    .query_async(&*client)
                    .await
                    .map_err(|e| anyhow!("Couldn't find GitLab projects for group: {group}: {e}"))
            }
            _ => unreachable!("Should be checked by `clap` and the outer conditional."),
        }
    }
}
