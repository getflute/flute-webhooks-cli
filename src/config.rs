use std::path::PathBuf;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Config {
    pub default_profile: String,
    pub poll_interval_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: "uat".into(),
            poll_interval_seconds: 5,
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
        ValidatedPoll { seconds: raw, warning: None }
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
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".flute")
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
    fn config_default_uses_uat_profile_and_5s() {
        let c = Config::default();
        assert_eq!(c.default_profile, "uat");
        assert_eq!(c.poll_interval_seconds, 5);
    }
}
