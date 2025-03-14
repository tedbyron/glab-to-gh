//! Command-line argument parser and types.

use anyhow::{anyhow, bail, Result};
use clap::{ArgAction, Args, Parser, ValueEnum};
use tracing::{instrument, Level};
use url::Url;

/// Command-line arguments.
#[derive(Debug, Parser)]
#[command(about)]
struct CliArgs {
    /// Source GitLab hostname
    #[arg(
        long,
        value_name = "HOST",
        env,
        default_value = "gitlab.com",
        value_parser = parse_glab_host
    )]
    glab_host: String,

    /// Source GitLab project(s)
    #[command(flatten)]
    source: SourceRepoOrNamespace,

    /// Target GitHub hostname
    #[arg(
        long,
        value_name = "HOST",
        env,
        default_value = "api.github.com",
        value_parser = parse_gh_host
    )]
    gh_host: String,

    /// Target GitHub namespace
    #[command(flatten)]
    target: TargetNamespace,

    /// Migrated repo visibility
    #[arg(
        short = 'v',
        long = "visibility",
        value_enum,
        value_name = "VIS",
        default_value_t = Visibility::Inherit
    )]
    target_visibility: Visibility,

    /// GitLab access token
    #[arg(long, value_name = "TOKEN", env, hide_env_values = true)]
    glab_token: Option<String>,

    /// GitHub access token
    #[arg(long, value_name = "TOKEN", env, hide_env_values = true)]
    gh_token: Option<String>,
}

/// Options for specifying the source GitLab project(s) to migrate from.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct SourceRepoOrNamespace {
    /// Source GitLab project path or full URL; can be a list or specified multiple times
    #[arg(short, long, action = ArgAction::Append, value_parser = validate_project)]
    pub repo: Option<Vec<PathOrUrl>>,

    /// Source GitLab user; migrate all repos
    #[arg(long = "su", visible_alias = "source-user", value_name = "USER")]
    pub source_user: bool,

    /// Source GitLab group or group/subgroup; migrate all repos
    #[arg(long = "sg", visible_alias = "source-group", value_parser = validate_namespace)]
    pub group: Option<String>,
}

#[derive(Debug)]
pub enum GhNamespace {
    User,
    Org(String),
}

/// Options for specifying the target GitHub project(s) to migrate to.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct TargetNamespace {
    /// Target GitHub user
    #[arg(long = "tu", visible_alias = "target-user", value_name = "USER")]
    pub target_user: bool,

    /// Target GitHub org
    #[arg(
        long = "to",
        visible_alias = "target-org",
        value_parser = validate_namespace
    )]
    pub org: Option<String>,
}

/// Path or full URL of a repo to migrate.
#[derive(Debug, Clone)]
pub enum PathOrUrl {
    Path(String),
    Url(String),
}

/// Target project visibility.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Visibility {
    Inherit,
    Public,
    Internal,
    Private,
}

/// Parse a GitLab hostname, removing the URL protocol if it exists.
#[allow(clippy::unnecessary_wraps)] // `clap` requires this function to return a `Result`.
pub fn parse_glab_host(mut host: &str) -> Result<String> {
    if host.is_empty() {
        host = "gitlab.com";
    }

    Ok(host
        .find("://")
        .and_then(|i| host.get(i + 3..))
        .unwrap_or(host)
        .to_string())
}

/// Parse a GitHub hostname, prefixing it with `https://` if it doesn't have a URL protocol.
#[allow(clippy::unnecessary_wraps)] // `clap` requires this function to return a `Result`.
pub fn parse_gh_host(mut host: &str) -> Result<String> {
    if host.is_empty() {
        host = "api.github.com";
    }

    if host.to_string().contains("://") {
        Ok(host.to_string())
    } else {
        Ok(format!("https://{host}"))
    }
}

/// Validate a namespace.
fn validate_namespace(namespace: &str) -> Result<String> {
    namespace
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || ['_', '.', '-', '/'].contains(&c))
        .then_some(namespace.to_string())
        .ok_or_else(|| {
            anyhow!(
                "can contain only letters `a-zA-Z`, digits `0-9`, underscores `_`, dots `.`, \
                dashes `-`, or slashes `/` (for GitLab subgroups)",
            )
        })
}

/// Validate a project path or a project's full URL.
fn validate_project(repo: &str) -> Result<PathOrUrl> {
    match Url::parse(repo) {
        Ok(_) => Ok(PathOrUrl::Url(repo.to_string())),
        Err(_) => {
            if repo
                .chars()
                .skip(1)
                .take(repo.len().saturating_sub(2))
                .filter(|c| c == &'/')
                .count()
                > 0
                && Url::parse("https://gitlab.com")?.join(repo).is_ok()
            {
                Ok(PathOrUrl::Path(repo.to_string()))
            } else {
                Err(anyhow!("must be a path or URL"))
            }
        }
    }
}

/// Parsed and validated command-line arguments.
#[derive(Debug)]
pub struct Cli {
    pub glab_host: String,
    pub source: SourceRepoOrNamespace,
    pub gh_host: String,
    pub target: GhNamespace,
    pub target_visibility: Visibility,
    pub glab_token: String,
    pub gh_token: String,
}

impl Cli {
    /// Parse from `std::env::args_os()`, exit on error.
    #[instrument(ret(level = Level::DEBUG))]
    pub fn parse() -> Result<Self> {
        // Parse `clap` arguments.
        let CliArgs {
            glab_host,
            source,
            gh_host,
            target,
            target_visibility,
            glab_token,
            gh_token,
        } = CliArgs::parse();
        let target = match (target.target_user, target.org) {
            (true, _) => GhNamespace::User,
            (_, Some(org)) if !org.is_empty() => GhNamespace::Org(org),
            _ => unreachable!("Should be checked by the command-line argument parser"),
        };
        let (glab_token, gh_token) = match (glab_token, gh_token) {
            (Some(glab_token), Some(gh_token)) => (glab_token, gh_token),
            (None, None) => bail!("Missing GitLab and GitHub tokens"),
            (None, _) => bail!("Missing GitLab token"),
            (_, None) => bail!("Missing GitHub token"),
        };

        Ok(Self {
            glab_host,
            source,
            gh_host,
            target,
            target_visibility,
            glab_token,
            gh_token,
        })
    }
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
