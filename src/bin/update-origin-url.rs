#![deny(missing_debug_implementations, rust_2018_idioms, unsafe_code)]
//! # update-origin-url
//!
//! > Update origin URLs in git repos
//!
//! ## Usage
//!
//! ```text
//! Usage: update-origin-url [OPTIONS] <--tu|--to <ORG>> <PATH>
//!
//! Arguments:
//!   <PATH>  Path to search for git repos
//!
//! Options:
//!       --glab-host <HOST>  Source GitLab hostname [env: GLAB_HOST=] [default: gitlab.com]
//!       --gh-host <HOST>    Target GitHub hostname [env: GH_HOST=] [default: api.github.com]
//!       --tu                Target GitHub user [aliases: target-user]
//!       --to <ORG>          Target GitHub org [aliases: target-org]
//!   -d, --depth <DEPTH>     Maximum depth to recurse into subdirectories
//!   -f, --follow-links      Follow symbolic links
//!       --gh-token <TOKEN>  GitHub access token [env: GH_TOKEN]
//!   -h, --help              Print help
//! ```
//!
//! ## Example
//!
//! Update origin URLs for all repos within `~/git`:
//!
//! ```sh
//! cargo run --bin update-origin-url -- ~/git --to example-org
//! #                                     └path   └target org
//! ```

use std::{env, path::PathBuf, process};

use anyhow::{Result, anyhow};
use clap::Parser;
use glab_to_gh::cli::{GhNamespace, TargetNamespace, parse_gh_host, parse_glab_host};
use octocrab::OctocrabBuilder;
use tracing::{Level, error, instrument};

/// Command-line arguments.
#[derive(Debug, Parser)]
#[command(about = "Update origin URLs in git repos")]
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

    /// Target GitHub hostname
    #[arg(
        long,
        value_name = "HOST",
        env = "GH_HOST",
        default_value = "api.github.com",
        value_parser = parse_gh_host
    )]
    gh_host: String,

    /// Target GitHub namespace
    #[command(flatten)]
    target: TargetNamespace,

    /// Path to search for git repos
    path: PathBuf,

    /// Maximum depth to recurse into subdirectories
    #[arg(short, long)]
    depth: Option<usize>,

    /// Follow symbolic links
    #[arg(short, long)]
    follow_links: bool,

    /// GitHub access token
    #[arg(long, value_name = "TOKEN", env = "GH_TOKEN", hide_env_values = true)]
    gh_token: Option<String>,
}

/// Parsed and validated command-line arguments.
#[derive(Debug)]
struct Cli {
    glab_host: String,
    gh_host: String,
    target: GhNamespace,
    path: PathBuf,
    depth: Option<usize>,
    follow_links: bool,
    gh_token: String,
}

impl Cli {
    /// Parse from `std::env::args_os()`, exit on error.
    #[instrument(ret(level = Level::DEBUG))]
    fn parse() -> Result<Self> {
        // Parse `clap` arguments.
        let CliArgs {
            glab_host,
            gh_host,
            target,
            path,
            depth,
            follow_links,
            gh_token,
        } = CliArgs::parse();
        let target = match (target.target_user, target.org) {
            (true, _) => GhNamespace::User,
            (_, Some(org)) if !org.is_empty() => GhNamespace::Org(org),
            _ => unreachable!("Should be checked by the command-line argument parser"),
        };
        let gh_token = gh_token.ok_or_else(|| anyhow!("Missing GitHub token"))?;

        Ok(Self {
            glab_host,
            gh_host,
            target,
            path,
            depth,
            follow_links,
            gh_token,
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
        gh_host,
        target,
        path,
        depth,
        follow_links,
        gh_token,
    } = Cli::parse()?;

    octocrab::initialise(
        OctocrabBuilder::default()
            .base_uri(&gh_host)?
            .user_access_token(gh_token.to_string())
            .build()?,
    );
    glab_to_gh::gh::update_origins(glab_host, gh_host, target, path, depth, follow_links).await?;

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
