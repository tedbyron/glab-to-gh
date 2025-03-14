//! Git functions.

use std::{env, ffi::OsStr, path::Path, process::Command};

use anyhow::{anyhow, bail, Result};
use tracing::{instrument, Level};

/// Execute a git command, returning its stdout or stderr. Stdin is piped.
#[instrument(skip_all, ret(level = Level::DEBUG))]
fn git<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S> + Send,
    S: AsRef<OsStr>,
{
    let cmd = Command::new("git").args(args).output()?;

    if cmd.status.success() {
        Ok(String::from_utf8_lossy(&cmd.stdout).into_owned())
    } else {
        bail!(
            "Git command failed: {}",
            String::from_utf8_lossy(&cmd.stderr).into_owned()
        );
    }
}

/// Clone a mirror of a GitLab project into a path.
#[instrument(skip_all)]
pub fn clone(url: impl AsRef<str>, path: impl AsRef<Path>) -> Result<()> {
    let url = url.as_ref();

    env::set_current_dir(path)?;
    git(["clone", "--mirror", url, "."])?;
    git(["lfs", "fetch", "--all"])?;

    Ok(())
}

/// Mirror push a local repo.
#[instrument(skip_all)]
pub fn push(path: impl AsRef<Path>, url: impl AsRef<str>) -> Result<()> {
    let url = url.as_ref();

    env::set_current_dir(path)?;
    git(["push", "--mirror", url])?;
    git(["lfs", "push", "--all", url])?;

    Ok(())
}

/// Query origin for the default branch name.
#[instrument(skip_all)]
pub fn default_branch(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let pat = "HEAD branch: ";

    env::set_current_dir(path)?;
    git(["remote", "show", "origin"])?
        .lines()
        .find_map(|l| {
            l.trim()
                .starts_with(pat)
                .then(|| l.trim_start_matches(pat).to_string())
        })
        .ok_or_else(|| anyhow!("Couldn't find the default branch in {}", path.display()))
}

/// Get the URL of the origin remote for a local repo.
#[instrument(skip_all)]
pub fn get_origin_url(path: impl AsRef<Path>) -> Result<String> {
    env::set_current_dir(path)?;
    git(["remote", "get-url", "origin"]).map(|s| s.trim().to_string())
}

/// Update the URL of the origin remote for a local repo.
#[instrument(skip_all)]
pub fn set_origin_url(path: impl AsRef<Path>, url: impl AsRef<str>) -> Result<()> {
    let url = url.as_ref();

    env::set_current_dir(path)?;
    git(["remote", "set-url", "origin", url])?;

    Ok(())
}
