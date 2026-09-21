//! Exercise logout against a shared in-memory keyring, never the OS keychain.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use keyring::mock::MockCredential;
use keyring::{Entry, Error};

#[derive(Default)]
struct MemoryKeyring {
    entries: Mutex<HashMap<(String, String), Arc<MockCredential>>>,
}

struct SharedCredential(Arc<MockCredential>);

impl CredentialApi for SharedCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        self.0.set_secret(secret)
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.0.get_secret()
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        self.0.delete_credential()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CredentialBuilderApi for MemoryKeyring {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        let credential = self
            .entries
            .lock()
            .unwrap()
            .entry((service.to_string(), user.to_string()))
            .or_default()
            .clone();
        Ok(Box::new(SharedCredential(credential)))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct RestoreKeyring;

impl Drop for RestoreKeyring {
    fn drop(&mut self) {
        keyring::set_default_credential_builder(keyring::default::default_credential_builder());
    }
}

fn saved_entry(service: &str, user: &str) -> Entry {
    let entry = Entry::new(service, user).unwrap();
    entry.set_password("test-credential").unwrap();
    entry
}

fn deny_next_deletion(entry: &Entry) {
    entry
        .get_credential()
        .downcast_ref::<SharedCredential>()
        .unwrap()
        .0
        .set_error(Error::NoStorageAccess(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "test keychain locked",
        ))));
}

#[test]
fn logout_cleans_selected_profile_and_surfaces_keychain_failures() {
    // One test owns the process-wide builder for all scenarios. No other
    // library tests access the keyring, so parallel tests cannot race it.
    keyring::set_default_credential_builder(Box::<MemoryKeyring>::default());
    let _restore = RestoreKeyring;
    let service = "flute-webhooks-cli";
    let production = saved_entry(service, "production");
    let sibling = saved_entry("flute-cli", "sandbox");

    // Cover current-only, legacy-only, and mixed layouts. In particular,
    // legacy-only credentials must not be re-migrated after logout.
    for users in [
        vec!["sandbox"],
        vec!["sandbox:client_id", "sandbox:client_secret"],
        vec!["sandbox", "sandbox:client_id", "sandbox:client_secret"],
    ] {
        let entries: Vec<_> = users
            .iter()
            .map(|user| saved_entry(service, user))
            .collect();
        super::auth_logout("sandbox").unwrap();
        for entry in entries {
            assert!(matches!(entry.get_password(), Err(Error::NoEntry)));
        }
        assert!(
            crate::auth::keychain::load_client_credentials("sandbox")
                .unwrap()
                .is_none()
        );
        super::auth_logout("sandbox").expect("repeated logout should succeed");
        assert_eq!(production.get_password().unwrap(), "test-credential");
        assert_eq!(sibling.get_password().unwrap(), "test-credential");
    }

    let current = saved_entry(service, "sandbox");
    deny_next_deletion(&current);
    let error = super::auth_logout("sandbox").unwrap_err();
    let envelope = crate::cli::output::ErrorJson::from_anyhow(&error);
    assert_eq!(envelope.kind, "auth");
    assert!(
        envelope
            .message
            .contains("deleting keychain entry for profile sandbox")
    );
    assert!(envelope.message.contains("test keychain locked"));
    assert_eq!(current.get_password().unwrap(), "test-credential");

    let legacy_id = saved_entry(service, "sandbox:client_id");
    let legacy_secret = saved_entry(service, "sandbox:client_secret");
    deny_next_deletion(&legacy_id);
    let error = super::auth_logout("sandbox").unwrap_err();
    let envelope = crate::cli::output::ErrorJson::from_anyhow(&error);
    assert_eq!(envelope.kind, "auth");
    assert!(envelope.message.contains("keychain cleanup incomplete"));
    assert!(matches!(current.get_password(), Err(Error::NoEntry)));
    assert!(matches!(legacy_secret.get_password(), Err(Error::NoEntry)));
    assert_eq!(legacy_id.get_password().unwrap(), "test-credential");
    super::auth_logout("sandbox").unwrap();
    assert!(matches!(legacy_id.get_password(), Err(Error::NoEntry)));
}
