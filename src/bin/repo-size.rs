#![deny(missing_debug_implementations, rust_2018_idioms, unsafe_code)]
//! # repo-size
//!
//! > Get repo size statistics for GitLab projects
//!
//! ## Usage
//!
//! ```text
//! Get repo size statistics for GitLab projects
//!
//! Usage: repo-size [OPTIONS] <--repo <REPO>|--su|--sg <GROUP>>
//!
//! Options:
//!       --glab-host <HOST>    Source GitLab hostname [env: GLAB_HOST=] [default: gitlab.com]
//!   -r, --repo <REPO>         Source GitLab project path or full URL; can be a list or specified multiple times
//!       --su                  Source GitLab user; migrate all repos [aliases: source-user]
//!       --sg <GROUP>          Source GitLab group or group/subgroup; migrate all repos [aliases: source-group]
//!       --glab-token <TOKEN>  GitLab access token [env: GLAB_TOKEN]
//!   -h, --help                Print help
//! ```
//!
//! ## Example
//!
//! Get repo size statistics for all repos in the `example-group` group:
//!
//! ```sh
//! cargo run --bin repo-size --features readable -- --sg example-group
//! #                                                  └source group
//! ```

use std::{env, process, sync::Arc};

use anyhow::{Result, anyhow};
use clap::Parser;
use gitlab::{
    GitlabBuilder,
    api::{AsyncQuery, projects},
};
use glab_to_gh::{
    cli::{SourceRepoOrNamespace, parse_glab_host},
    glab::{self, Project},
    prompt_continue,
};
use readable::byte::Byte;
use tracing::{Level, error, info, instrument};

/// Command-line arguments.
#[derive(Debug, Parser)]
#[command(about = "Get repo size statistics for GitLab projects")]
struct CliArgs {
    /// Source GitLab hostname
    #[arg(
        long,
        value_name = "HOST",
        env = "GLAB_HOST",
        default_value = "gitlab.com",
        value_parser = parse_glab_host
    )]
    glab_host: String,

    /// Source GitLab project(s)
    #[command(flatten)]
    source: SourceRepoOrNamespace,

    /// GitLab access token
    #[arg(long, value_name = "TOKEN", env = "GLAB_TOKEN", hide_env_values = true)]
    glab_token: Option<String>,
}

/// Parsed and validated command-line arguments.
#[derive(Debug)]
pub struct Cli {
    pub glab_host: String,
    pub source: SourceRepoOrNamespace,
    pub glab_token: String,
}

impl Cli {
    /// Parse from `std::env::args_os()`, exit on error.
    #[instrument(ret(level = Level::DEBUG))]
    pub fn parse() -> Result<Self> {
        // Parse `clap` arguments.
        let CliArgs {
            glab_host,
            source,
            glab_token,
        } = CliArgs::parse();
        let glab_token = glab_token.ok_or_else(|| anyhow!("Missing GitLab token"))?;

        Ok(Self {
            glab_host,
            source,
            glab_token,
        })
    }
}

#[tokio::main]
async fn main() {
    // If an error occurs, log it and exit with a non-zero exit code.
    process::exit(match run().await {
        Ok(()) => 0,
        Err(e) => {
            error!("{e:#}");
            1
        }
    });
}

async fn run() -> Result<()> {
    // Load environment variables from `.env` file if it exists.
    #[cfg(feature = "dotenv")]
    let _ = dotenv::dotenv();

    // Install a global default tracing subscriber.
    tracing_subscriber::fmt()
        .without_time()
        .with_target(cfg!(feature = "debug"))
        .with_line_number(cfg!(feature = "debug"))
        .with_max_level(env::var("GLAB_TO_GH_LOG").map_or(Level::WARN, |lvl| {
            match &*lvl.to_ascii_lowercase() {
                "error" => Level::ERROR,
                "warn" => Level::WARN,
                "debug" => Level::DEBUG,
                "trace" => Level::TRACE,
                _ => Level::INFO,
            }
        }))
        .init();

    // Parse command-line arguments.
    let Cli {
        glab_host,
        source,
        glab_token,
    } = Cli::parse()?;
    let glab = Arc::new(
        GitlabBuilder::new(&glab_host, &glab_token)
            .build_async()
            .await?,
    );
    // Get the current GitLab user, and all projects specified on the command-line.
    let glab_user = glab::user(&glab).await?;
    info!(glab_user = %glab_user.username);
    let source_info = match (&source.repo, &source.source_user, &source.group) {
        (Some(repos), _, _) => format!("Repos({repos:?})"),
        (_, true, _) => format!("User({})", &glab_user.username),
        (_, _, Some(group)) => format!("Group({})", group.clone()),
        _ => unreachable!("Should be checked by the command-line argument parser"),
    };
    info!(source = %source_info);
    let projects = glab::projects(Arc::clone(&glab), &glab_user, glab_host.clone(), source).await?;

    if projects.is_empty() {
        return Ok(());
    }

    let mut count = 0;
    let mut storage_size = 0;
    let mut repo_size = 0;
    let mut lfs_size = 0;

    // Must query each project individually to get project statistics.
    for p in &projects {
        let endpoint = projects::Project::builder()
            .project(&*p.path_with_namespace)
            .statistics(true)
            .build()?;
        let p: Result<Project> = endpoint.query_async(glab.as_ref()).await.map_err(|e| {
            anyhow!(
                "Couldn't find the GitLab project: {}: {e}",
                p.path_with_namespace
            )
        });
        let p = match p {
            Ok(p) => p,
            Err(e) => {
                prompt_continue(e)?;
                continue;
            }
        };

        if let Some(stats) = p.statistics {
            count += 1;
            storage_size += stats.storage_size;
            repo_size += stats.repository_size;
            lfs_size += stats.lfs_objects_size;
        }
    }

    info!(
        "{count}/{} projects retrieved with statistics",
        projects.len(),
    );
    info!(
        "Total storage size: {storage_size} ({})",
        Byte::from(storage_size)
    );
    info!("Total repo size: {repo_size} ({})", Byte::from(repo_size));
    info!(
        "Total LFS objects size: {lfs_size} ({})",
        Byte::from(lfs_size)
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::CliArgs;

    #[test]
    fn test_cli() {
        CliArgs::command().debug_assert();
    }
}
