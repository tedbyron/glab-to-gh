#![deny(missing_debug_implementations, rust_2018_idioms, unsafe_code)]
#![doc = include_str!("../README.md")]

use anyhow::{bail, Result};
use inquire::Confirm;
use tracing::{error, instrument, Level};
use url::Url;

pub mod cli;
pub mod gh;
pub mod git;
pub mod glab;

/// Print an error and prompt the user to continue. Returns `Ok(())` if the user chose to continue,
/// or else `Error("Aborted")`.
#[instrument(skip_all)]
pub fn prompt_continue(e: impl std::fmt::Display) -> Result<()> {
    error!(error = %e);

    if Confirm::new("Continue?").with_default(false).prompt()? {
        Ok(())
    } else {
        bail!("Aborted")
    }
}

/// Add a username and password to a URL. Errors if an invalid URL is passed.
#[instrument(skip_all, ret(level = Level::DEBUG))]
pub fn url_with_auth(
    url: impl AsRef<str>,
    username: impl AsRef<str>,
    password: impl AsRef<str>,
) -> Result<Url> {
    let mut url = Url::parse(url.as_ref())?;
    let _ = url.set_username(username.as_ref());
    let _ = url.set_password(Some(password.as_ref()));

    Ok(url)
}
