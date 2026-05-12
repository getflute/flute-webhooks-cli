//! Self-update wrapper around `axoupdater`.
//!
//! Two callers:
//!   * the `update` CLI subcommand, which performs the install
//!   * (Phase 3) a startup check that only fetches the latest version
//!
//! For users who installed via the cargo-dist shell/PowerShell/Homebrew
//! installers, the installer drops an "install receipt" at
//! `~/.config/flute-webhooks/flute-webhooks-receipt.json`. axoupdater reads
//! that file to know which release source + installer to use. For users who
//! built from source (`cargo install`, `cargo build --release`) there is no
//! receipt — we fall back to an explicit GitHub Releases source so the
//! version *check* still works. In that case the actual install can't be
//! performed by axoupdater (no installer metadata), so we surface a clear
//! "reinstall via the shell/brew/powershell installer" message instead of
//! a cryptic error.
//!
//! `FLUTE_GITHUB_TOKEN` is honored as an unauthenticated-rate-limit escape
//! hatch (60/hr → 5000/hr); useful in CI smoke tests.

use anyhow::{Context, Result};
use axoupdater::{AxoUpdater, ReleaseSource, ReleaseSourceType};

const APP_NAME: &str = "flute-webhooks";
const REPO_OWNER: &str = "getflute";
const REPO_NAME: &str = "flute-webhooks";

/// Build an `AxoUpdater` configured for this binary. Returns `(updater,
/// has_receipt)` so callers can decide whether `run()` is even meaningful.
fn make_updater() -> (AxoUpdater, bool) {
    let mut updater = AxoUpdater::new_for(APP_NAME);
    let has_receipt = updater.load_receipt().is_ok();
    if !has_receipt {
        updater.set_release_source(ReleaseSource {
            release_type: ReleaseSourceType::GitHub,
            owner: REPO_OWNER.into(),
            name: REPO_NAME.into(),
            app_name: APP_NAME.into(),
        });
        if let Ok(v) = env!("CARGO_PKG_VERSION").parse() {
            let _ = updater.set_current_version(v);
        }
    }
    if let Ok(token) = std::env::var("FLUTE_GITHUB_TOKEN") {
        updater.set_github_token(&token);
    }
    (updater, has_receipt)
}

/// Drive the `update` subcommand. Always exits Ok unless there is a hard
/// network/auth error — "already on latest" and "no receipt" are reported
/// to the user as informational text, not as failures.
pub async fn run() -> Result<()> {
    let (mut updater, has_receipt) = make_updater();

    if !has_receipt {
        let latest = updater
            .query_new_version()
            .await
            .context("failed to query GitHub Releases for the latest version")?;
        match latest {
            Some(v) if v.to_string() != env!("CARGO_PKG_VERSION") => {
                println!(
                    "A newer version ({v}) is available, but this binary was not installed via a \
                     cargo-dist installer, so `update` cannot replace it in place.\n\
                     Reinstall using one of:\n  \
                     curl -LsSf https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/latest/download/flute-webhooks-installer.sh | sh\n  \
                     brew install {REPO_OWNER}/{REPO_NAME}/flute-webhooks\n  \
                     irm https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/latest/download/flute-webhooks-installer.ps1 | iex",
                );
            }
            _ => println!(
                "Already on the latest version ({}).",
                env!("CARGO_PKG_VERSION")
            ),
        }
        return Ok(());
    }

    println!("Checking for updates…");
    match updater.run().await? {
        Some(result) => {
            println!("Updated to {}.", result.new_version);
        }
        None => {
            println!(
                "Already on the latest version ({}).",
                env!("CARGO_PKG_VERSION")
            );
        }
    }
    Ok(())
}
