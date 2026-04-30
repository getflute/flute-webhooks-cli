use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "flute-webhook";

fn entry(profile: &str, kind: &str) -> Result<Entry> {
    Entry::new(SERVICE, &format!("{profile}:{kind}"))
        .with_context(|| format!("creating keyring entry {profile}:{kind}"))
}

pub fn store_client_credentials(profile: &str, client_id: &str, client_secret: &str) -> Result<()> {
    entry(profile, "client_id")?.set_password(client_id)?;
    entry(profile, "client_secret")?.set_password(client_secret)?;
    Ok(())
}

pub fn load_client_credentials(profile: &str) -> Result<Option<(String, String)>> {
    let id_entry = entry(profile, "client_id")?;
    let secret_entry = entry(profile, "client_secret")?;
    match (id_entry.get_password(), secret_entry.get_password()) {
        (Ok(id), Ok(secret)) => Ok(Some((id, secret))),
        (Err(keyring::Error::NoEntry), _) | (_, Err(keyring::Error::NoEntry)) => Ok(None),
        (Err(e), _) | (_, Err(e)) => Err(e.into()),
    }
}

pub fn delete_client_credentials(profile: &str) -> Result<()> {
    let _ = entry(profile, "client_id")?.delete_credential();
    let _ = entry(profile, "client_secret")?.delete_credential();
    Ok(())
}

pub fn load_with_env_fallback(profile: &str) -> Result<Option<(String, String)>> {
    if let (Ok(id), Ok(secret)) = (std::env::var("FLUTE_CLIENT_ID"), std::env::var("FLUTE_CLIENT_SECRET")) {
        return Ok(Some((id, secret)));
    }
    load_client_credentials(profile)
}
