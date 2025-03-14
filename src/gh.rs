//! GitHub API functions.

use std::path::Path;

use anyhow::{anyhow, Result};
use gitlab::VisibilityLevel;
use ignore::{WalkBuilder, WalkState};
use serde::Deserialize;
use serde_json::json;
use tempfile::TempDir;
use tracing::{debug, error, info, instrument, warn, Level};
use url::Url;

use crate::{
    cli::{GhNamespace, Visibility},
    git,
    glab::{Project, User},
};

/// GitHub repository.
#[derive(Debug, Deserialize)]
struct Repository {
    name: String,
    clone_url: Option<Url>,
}

/// GitHub user.
#[derive(Debug, Deserialize)]
struct Author {
    login: String,
}

/// Convert a GitLab project path to a GitHub repo name.
///
/// # Examples
///
/// ```
/// # use glab_to_gh::gh::glab_path_to_gh_name;
/// let gh_name = glab_path_to_gh_name("group/subgroup/project");
/// assert_eq!(gh_name, "subgroup_project");
/// ```
#[instrument(skip_all)]
pub fn glab_path_to_gh_name(path: impl AsRef<str>) -> String {
    let name = path
        .as_ref()
        .trim_matches('/')
        .replace('_', "-")
        .split('/')
        .skip(1) // Skip the namespace and only include subgroups.
        .collect::<Box<_>>()
        .join("_");

    // Restrict to 100 bytes.
    match name.len() {
        ..=100 => name,
        _ => name[..100].to_string(),
    }
}

/// Get a GitHub repo.
#[instrument(skip_all)]
async fn get_repo(full_name: impl std::fmt::Display + Send) -> Result<Repository> {
    octocrab::instance()
        .get(format!("repos/{full_name}"), None::<&()>)
        .await
        .map_err(Into::into)
}

/// Create a new GitHub repo using the specified endpoint.
#[instrument(skip_all, ret(level = Level::DEBUG))]
async fn create_repo(
    route: impl AsRef<str> + Send,
    project: &Project,
    name: &str,
    vis: VisibilityLevel,
) -> Result<Repository> {
    let description = project.description.as_deref().unwrap_or_default();
    // Restrict to 350 bytes.
    let description = match description.len() {
        ..=350 => description,
        _ => &description[..350],
    };
    let private = (vis == VisibilityLevel::Private).then_some(true);

    octocrab::instance()
        .post(
            route,
            Some(&json!({
                "name": name,
                "description": description,
                "private": private,
                "visibility": vis,
                "has_projects": false,
                "has_wiki": false,
                "has_downloads": false,
            })),
        )
        .await
        .map_err(Into::into)
}

/// Update a GitHub repo.
#[instrument(skip_all)]
async fn update_repo(
    full_name: impl AsRef<str> + Send,
    default_branch: impl AsRef<str> + Send,
    archived: bool,
) -> Result<Repository> {
    octocrab::instance()
        .patch(
            format!("/repos/{}", full_name.as_ref()),
            Some(&json!({
                "default_branch": default_branch.as_ref(),
                "archived": archived,
            })),
        )
        .await
        .map_err(Into::into)
}

/// Get the currently authenticated GitHub user.
#[instrument(skip_all, ret(level = Level::DEBUG))]
async fn user() -> Result<Author> {
    octocrab::instance()
        .get("/user", None::<&()>)
        .await
        .map_err(Into::into)
}

/// Clone local mirrors of multiple GitLab projects and upload them to GitHub.
#[instrument(skip_all)]
pub async fn clone_and_push(
    glab_token: &str,
    glab_user: &User,
    gh_token: &str,
    projects: &[Project],
    target: GhNamespace,
    visibility: Visibility,
) -> Result<()> {
    let mut error = None;
    let mut temp_dir: Option<TempDir> = None;

    let user = user().await?;
    info!(gh_user = %user.login);

    // Clone GitLab projects and upload to GitHub.
    // Synchronously loop over projects to reduce the amount of storage space used when cloning.
    for project in projects {
        if let Some(e) = error.take() {
            if let Some(dir) = temp_dir.take() {
                if let Err(e) = dir.close() {
                    error!(error = %e);
                }
            }
            crate::prompt_continue(e)?;
        }

        temp_dir = Some(tempfile::tempdir()?);
        // Unwrap: always assigned to `Some` above.
        let temp_path = temp_dir.as_ref().unwrap().path();
        debug!(temp_path = %temp_path.display());
        let glab_url = match crate::url_with_auth(
            &project.http_url_to_repo,
            &glab_user.username,
            glab_token,
        ) {
            Ok(url) => url,
            Err(e) => {
                error = Some(e);
                continue;
            }
        };

        // Clone the GitLab repo.
        if let Err(e) = git::clone(glab_url, temp_path) {
            error = Some(e);
            continue;
        }
        info!("Cloned GitLab project: {}", project.path_with_namespace);

        let gh_name = glab_path_to_gh_name(&project.path_with_namespace);
        let vis = match visibility {
            Visibility::Inherit => project.visibility,
            Visibility::Public => VisibilityLevel::Public,
            Visibility::Internal => VisibilityLevel::Internal,
            Visibility::Private => VisibilityLevel::Private,
        };
        // Create a new GitHub repo if one doesn't already exist with the same name.
        let (full_name, create_repo) = match &target {
            GhNamespace::User => {
                let full_name = format!("{}/{}", user.login, &gh_name);
                let create_repo = if get_repo(&full_name).await.is_ok() {
                    Err(anyhow!("GitHub repo already exists: {full_name}"))
                } else {
                    create_repo("/user/repos", project, &gh_name, vis).await
                };

                (full_name, create_repo)
            }
            GhNamespace::Org(org) => {
                let full_name = format!("{}/{}", org, &gh_name);
                let create_repo = if get_repo(&full_name).await.is_ok() {
                    Err(anyhow!("GitHub repo already exists: {full_name}"))
                } else {
                    create_repo(format!("/orgs/{org}/repos"), project, &gh_name, vis).await
                };

                (full_name, create_repo)
            }
        };
        let repo = match create_repo {
            Ok(repo) => repo,
            Err(e) => {
                error = Some(e);
                continue;
            }
        };
        // Unwrap: checked within `create_repo`.
        info!("Created GitHub repo: {full_name}");
        let gh_url = match repo
            .clone_url
            .as_ref()
            .ok_or_else(|| anyhow!("No repo `clone_url` for: {}", repo.name))
        {
            Ok(url) => match crate::url_with_auth(url, &user.login, gh_token) {
                Ok(url) => url,
                Err(e) => {
                    error = Some(e);
                    continue;
                }
            },
            Err(e) => {
                error = Some(e);
                continue;
            }
        };

        // Push the local repo to the newly created GitHub repo.
        if let Err(e) = git::push(temp_path, gh_url) {
            error = Some(e);
            continue;
        }
        info!("Pushed repo to GitHub");

        // Update the default branch and archive the repo if it was archived on GitLab.
        if project.default_branch.is_none() {
            warn!(
                "Couldn't retrieve default branch for: {}, skipping updating GitHub default branch",
                project.path_with_namespace
            );
        }

        let default_branch = match &project.default_branch {
            Some(b) => b.clone(),
            None => match git::default_branch(temp_path) {
                Ok(b) => b,
                Err(e) => {
                    error = Some(e);
                    continue;
                }
            },
        };

        if let Err(e) = update_repo(full_name, default_branch, project.archived).await {
            error = Some(e);
            continue;
        }
        info!("Updated GitHub repo");

        // Unwrap: always assigned to `Some` at the beginning of the loop.
        if let Err(e) = temp_dir.take().unwrap().close() {
            error = Some(e.into());
        }
    }

    error.map_or(Ok(()), Err)
}

/// Update the URL of the origin remote for all git repos in a path.
#[instrument(skip_all)]
pub async fn update_origins(
    glab_host: impl AsRef<str> + Send,
    gh_host: impl AsRef<str> + Send,
    target: GhNamespace,
    path: impl AsRef<Path> + Send,
    depth: Option<usize>,
    follow_links: bool,
) -> Result<()> {
    let glab_host = glab_host.as_ref();
    let gh_host = &gh_host.as_ref().replace("api.", "");
    let target = &match target {
        GhNamespace::User => user().await?.login,
        GhNamespace::Org(org) => org,
    };

    WalkBuilder::new(path)
        .max_depth(depth)
        .follow_links(follow_links)
        .build_parallel()
        .run(|| {
            Box::new(move |res| {
                let Ok(entry) = res else {
                    return WalkState::Continue;
                };
                let path = entry.path();

                if !(path.is_dir() && matches!(path.join(".git").try_exists(), Ok(true))) {
                    return WalkState::Continue;
                }

                let Ok(origin) = git::get_origin_url(path) else {
                    return WalkState::Continue;
                };

                if !origin.contains(glab_host) {
                    return WalkState::Continue;
                }

                let Ok(url) = Url::parse(&origin) else {
                    return WalkState::Continue;
                };
                let gh_name = glab_path_to_gh_name(url.path());
                let Ok(mut gh_url) = Url::parse(gh_host) else {
                    error!("Couldn't parse GitHub host: {gh_host}");
                    return WalkState::Continue;
                };

                gh_url.set_path(&format!("{target}/{gh_name}"));

                if git::set_origin_url(path, &gh_url).is_ok() {
                    info!("Updated origin URL for {}: {gh_url}", path.display());
                } else {
                    error!("Couldn't update origin URL for: {}", path.display());
                }

                WalkState::Continue
            })
        });

    Ok(())
}
