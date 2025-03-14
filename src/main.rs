#![deny(missing_debug_implementations, rust_2018_idioms, unsafe_code)]
#![doc = include_str!("../README.md")]

use std::{env, process, sync::Arc};

use anyhow::Result;
use gitlab::GitlabBuilder;
use glab_to_gh::{cli::Cli, gh, glab};
use octocrab::OctocrabBuilder;
use tracing::{error, info, Level};

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
        gh_host,
        target,
        target_visibility,
        glab_token,
        gh_token,
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
    let projects = glab::projects(glab, &glab_user, glab_host, source)
        .await?
        .into_boxed_slice();
    info!(
        "Found {} project{}",
        projects.len(),
        if projects.len() == 1 { "" } else { "s" },
    );

    if projects.is_empty() {
        return Ok(());
    }

    octocrab::initialise(
        OctocrabBuilder::default()
            .base_uri(gh_host)?
            .user_access_token(gh_token.to_string())
            .build()?,
    );

    // Clone mirrors of all projects and push to GitHub.
    gh::clone_and_push(
        &glab_token,
        &glab_user,
        &gh_token,
        &projects,
        target,
        target_visibility,
    )
    .await?;

    Ok(())
}
