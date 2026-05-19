use std::path::PathBuf;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Config {
    pub default_profile: String,
    pub poll_interval_seconds: u64,
    pub auto_update_check: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: "sandbox".into(),
            poll_interval_seconds: 5,
            auto_update_check: true,
        }
    }
}

pub const POLL_MIN: u64 = 5;
pub const POLL_MAX: u64 = 60;
pub const POLL_DEFAULT: u64 = 5;
pub const POLL_BACKOFF_SECS: u64 = 20;

#[derive(Debug, Clone)]
pub struct ValidatedPoll {
    pub seconds: u64,
    pub warning: Option<String>,
}

pub fn validate_poll_interval(raw: u64) -> ValidatedPoll {
    if (POLL_MIN..=POLL_MAX).contains(&raw) {
        ValidatedPoll {
            seconds: raw,
            warning: None,
        }
    } else {
        ValidatedPoll {
            seconds: POLL_DEFAULT,
            warning: Some(format!(
                "poll_interval_seconds={raw} is outside {POLL_MIN}-{POLL_MAX}; defaulting to {POLL_DEFAULT}"
            )),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flute")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load_or_default() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub api_base_url: String,
    pub oauth_url: String,
}

impl Profile {
    pub fn sandbox() -> Self {
        Self {
            name: "sandbox".into(),
            // Canonical API path: requests to /v2/... go through the Kestrel
            // gateway (returns proper 401 with WWW-Authenticate: Bearer). The
            // /isv-api/swagger/... prefix only hosts the swagger UI; routing
            // to /isv-api/v2/... directly bypasses the gateway and 404s on
            // every endpoint except the documentation routes.
            //
            // The host names still carry `uat.` — that's the Flute team's
            // canonical hostname for the sandbox environment and is not
            // ours to rename.
            api_base_url: "https://api.uat.arise.risewithaurora.com".into(),
            oauth_url: "https://oauth.uat.arise.risewithaurora.com/oauth2/token".into(),
        }
    }

    pub fn production() -> Self {
        Self {
            name: "production".into(),
            api_base_url: "https://api.arise.risewithaurora.com".into(),
            oauth_url: "https://oauth.arise.risewithaurora.com/oauth2/token".into(),
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "sandbox" => Some(Self::sandbox()),
            "production" | "prod" => Some(Self::production()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_in_range_accepted() {
        let v = validate_poll_interval(10);
        assert_eq!(v.seconds, 10);
        assert!(v.warning.is_none());
    }

    #[test]
    fn poll_below_min_warns_and_defaults() {
        let v = validate_poll_interval(1);
        assert_eq!(v.seconds, POLL_DEFAULT);
        assert!(v.warning.unwrap().contains("outside"));
    }

    #[test]
    fn poll_above_max_warns_and_defaults() {
        let v = validate_poll_interval(120);
        assert_eq!(v.seconds, POLL_DEFAULT);
        assert!(v.warning.is_some());
    }

    #[test]
    fn boundary_min_accepted() {
        assert_eq!(validate_poll_interval(POLL_MIN).seconds, POLL_MIN);
    }

    #[test]
    fn boundary_max_accepted() {
        assert_eq!(validate_poll_interval(POLL_MAX).seconds, POLL_MAX);
    }

    #[test]
    fn config_default_uses_sandbox_profile_and_5s() {
        let c = Config::default();
        assert_eq!(c.default_profile, "sandbox");
        assert_eq!(c.poll_interval_seconds, 5);
        assert!(c.auto_update_check);
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn sandbox_profile_has_uat_hosts() {
        // Profile is named "sandbox" but the hostnames remain uat.* — that's
        // the Flute team's canonical sandbox URL, not ours to rename.
        let p = Profile::sandbox();
        assert_eq!(p.name, "sandbox");
        assert_eq!(p.api_base_url, "https://api.uat.arise.risewithaurora.com");
        assert_eq!(
            p.oauth_url,
            "https://oauth.uat.arise.risewithaurora.com/oauth2/token"
        );
    }

    #[test]
    fn production_profile_has_prod_hosts() {
        let p = Profile::production();
        assert_eq!(p.api_base_url, "https://api.arise.risewithaurora.com");
        assert_eq!(
            p.oauth_url,
            "https://oauth.arise.risewithaurora.com/oauth2/token"
        );
    }

    #[test]
    fn api_base_urls_have_no_path_prefix() {
        // Routing guard: the API is served at the root of the host. Any
        // accidental path suffix on the base URL (e.g. /isv-api, /api) routes
        // requests away from the gateway and yields 404s. Catch that here.
        for p in [Profile::sandbox(), Profile::production()] {
            let trimmed = p.api_base_url.trim_end_matches('/');
            let path_idx = trimmed.find("//").map(|i| i + 2).unwrap_or(0);
            let after_host = &trimmed[path_idx..];
            assert!(
                !after_host.contains('/'),
                "profile {} must have no path on its api_base_url: got {}",
                p.name,
                p.api_base_url
            );
        }
    }

    #[test]
    fn by_name_resolves_known_profiles() {
        assert_eq!(Profile::by_name("sandbox").unwrap().name, "sandbox");
        assert_eq!(Profile::by_name("production").unwrap().name, "production");
        assert_eq!(Profile::by_name("prod").unwrap().name, "production");
        assert!(Profile::by_name("garbage").is_none());
        // "uat" is no longer a valid profile — clean rename, no alias.
        assert!(Profile::by_name("uat").is_none());
    }
}
