use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "flute-webhook";

/// Stored credentials are kept in a single keychain entry per profile, so that
/// each app launch only triggers a single Keychain authorisation prompt instead
/// of one per field. The value is JSON-encoded `{client_id, client_secret}`.
#[derive(Debug, Serialize, Deserialize)]
struct StoredCreds {
    client_id: String,
    client_secret: String,
}

fn entry(profile: &str) -> Result<Entry> {
    Entry::new(SERVICE, profile).with_context(|| format!("creating keyring entry for profile {profile}"))
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

fn delete_legacy_pair(profile: &str) {
    if let Ok(e) = Entry::new(SERVICE, &format!("{profile}:client_id")) {
        let _ = e.delete_credential();
    }
    if let Ok(e) = Entry::new(SERVICE, &format!("{profile}:client_secret")) {
        let _ = e.delete_credential();
    }
}

pub fn store_client_credentials(profile: &str, client_id: &str, client_secret: &str) -> Result<()> {
    let creds = StoredCreds {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
    };
    let json = serde_json::to_string(&creds).context("serialising credentials")?;
    entry(profile)?.set_password(&json)?;
    // Clean up any pre-existing two-entry layout so we never read from it again.
    delete_legacy_pair(profile);
    Ok(())
}

pub fn load_client_credentials(profile: &str) -> Result<Option<(String, String)>> {
    let e = entry(profile)?;
    match e.get_password() {
        Ok(json) => {
            let creds: StoredCreds = serde_json::from_str(&json)
                .context("decoding credentials JSON from keychain")?;
            Ok(Some((creds.client_id, creds.client_secret)))
        }
        Err(keyring::Error::NoEntry) => {
            // Migration path: older builds saved a separate entry per field.
            // Read those, write them into the single-entry layout, return.
            match try_load_legacy_pair(profile)? {
                Some((id, secret)) => {
                    let _ = store_client_credentials(profile, &id, &secret);
                    Ok(Some((id, secret)))
                }
                None => Ok(None),
            }
        }
        Err(e) => Err(e.into()),
    }
}

pub fn delete_client_credentials(profile: &str) -> Result<()> {
    let _ = entry(profile)?.delete_credential();
    delete_legacy_pair(profile);
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
