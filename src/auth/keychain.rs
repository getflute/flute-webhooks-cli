use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use tracing::warn;

const SERVICE: &str = "flute-webhooks-cli";

/// Stored credentials are kept in a single keychain entry per profile, so that
/// each app launch only triggers a single Keychain authorisation prompt instead
/// of one per field. The value is JSON-encoded `{client_id, client_secret}`.
#[derive(Debug, Serialize, Deserialize)]
struct StoredCreds {
    client_id: String,
    client_secret: String,
}

fn entry(profile: &str) -> Result<Entry> {
    Entry::new(SERVICE, profile)
        .with_context(|| format!("creating keyring entry for profile {profile}"))
}

/// Older builds stored credentials as two separate keychain entries
/// (`<profile>:client_id` and `<profile>:client_secret`). Reading from those
/// would double the OS Keychain prompts on every launch, so we migrate to the
/// single-entry layout the first time we see them.
fn try_load_legacy_pair(profile: &str) -> Result<Option<(String, String)>> {
    let id_entry = Entry::new(SERVICE, &format!("{profile}:client_id"))?;
    let secret_entry = Entry::new(SERVICE, &format!("{profile}:client_secret"))?;
    match (id_entry.get_password(), secret_entry.get_password()) {
        (Ok(id), Ok(secret)) => Ok(Some((id, secret))),
        (Err(keyring::Error::NoEntry), _) | (_, Err(keyring::Error::NoEntry)) => Ok(None),
        (Err(e), _) | (_, Err(e)) => Err(e.into()),
    }
}

/// Best-effort cleanup of the legacy two-entry layout. Attempts *both* entries
/// before returning so a failure on the first doesn't leave the second on
/// disk. Missing entries (`NoEntry`) are the expected case and don't count as
/// a failure. Every non-`NoEntry` error is warned via `tracing::warn!`, and
/// the caller gets a single summary error if any deletion failed.
fn delete_legacy_pair(profile: &str) -> Result<()> {
    let mut failures = 0usize;
    for suffix in ["client_id", "client_secret"] {
        let target = format!("{profile}:{suffix}");
        match Entry::new(SERVICE, &target) {
            Ok(e) => match e.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(err) => {
                    failures += 1;
                    warn!(
                        target = %target,
                        error = %err,
                        "failed to delete legacy keychain entry"
                    );
                }
            },
            Err(err) => {
                failures += 1;
                warn!(
                    target = %target,
                    error = %err,
                    "failed to open legacy keychain entry for deletion"
                );
            }
        }
    }
    if failures > 0 {
        anyhow::bail!(
            "keychain cleanup incomplete: {failures} legacy entries for profile {profile} could not be deleted (see logs)"
        );
    }
    Ok(())
}

pub fn store_client_credentials(profile: &str, client_id: &str, client_secret: &str) -> Result<()> {
    let creds = StoredCreds {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
    };
    let json = serde_json::to_string(&creds).context("serialising credentials")?;
    entry(profile)?.set_password(&json)?;
    // Clean up any pre-existing two-entry layout so we never read from it again.
    // The primary write already succeeded so we don't fail the whole store on a
    // cleanup problem; just warn (the delete_legacy_pair body already logged).
    if let Err(err) = delete_legacy_pair(profile) {
        warn!(profile = %profile, error = %err, "legacy keychain cleanup after store failed");
    }
    Ok(())
}

pub fn load_client_credentials(profile: &str) -> Result<Option<(String, String)>> {
    let e = entry(profile)?;
    match e.get_password() {
        Ok(json) => {
            let creds: StoredCreds =
                serde_json::from_str(&json).context("decoding credentials JSON from keychain")?;
            Ok(Some((creds.client_id, creds.client_secret)))
        }
        Err(keyring::Error::NoEntry) => {
            // Migration path: older builds saved a separate entry per field.
            // Read those, write them into the single-entry layout, return.
            match try_load_legacy_pair(profile)? {
                Some((id, secret)) => {
                    // A migration write failure would mean the next launch
                    // re-migrates (double OS prompt) with no signal. Surface it
                    // as a warning but still return the loaded credentials —
                    // the user's current call should not be blocked by a
                    // persistence hiccup.
                    if let Err(err) = store_client_credentials(profile, &id, &secret) {
                        warn!(
                            profile = %profile,
                            error = %err,
                            "failed to migrate legacy keychain layout; will re-attempt on next launch"
                        );
                    }
                    Ok(Some((id, secret)))
                }
                None => Ok(None),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Explicit credential deletion. Errors propagate to the caller so
/// operator-driven `logout`-style flows surface failures instead of leaving
/// stale material on disk. `NoEntry` is treated as idempotent success — the
/// end state matches the desired one.
pub fn delete_client_credentials(profile: &str) -> Result<()> {
    match entry(profile)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(err) => {
            return Err(anyhow::Error::from(err))
                .with_context(|| format!("deleting keychain entry for profile {profile}"));
        }
    }
    delete_legacy_pair(profile)
        .with_context(|| format!("deleting legacy keychain entries for profile {profile}"))?;
    Ok(())
}

pub fn load_with_env_fallback(profile: &str) -> Result<Option<(String, String)>> {
    if let (Ok(id), Ok(secret)) = (
        std::env::var("FLUTE_CLIENT_ID"),
        std::env::var("FLUTE_CLIENT_SECRET"),
    ) {
        return Ok(Some((id, secret)));
    }
    load_client_credentials(profile)
}
