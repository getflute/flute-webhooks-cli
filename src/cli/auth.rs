//! Live authentication status for `flute-webhooks auth status`.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use crate::api::{ApiClient, error::ApiError};
use crate::auth::{
    keychain,
    token::{OAuth2Fetcher, TokenStore},
};
use crate::cli::OutputFormat;
use crate::config::Profile;

#[derive(serde::Serialize)]
struct AuthStatus {
    profile: String,
    api_base_url: String,
    authenticated: bool,
    client_id: Option<String>,
    merchant_id: Option<String>,
}

/// Report the selected profile and a live authentication check. A failed
/// OAuth exchange or ping is reported as `authenticated: false`, matching
/// `flute-cli`; profile and keychain errors still fail the command.
pub async fn status(profile: &str, output: OutputFormat) -> anyhow::Result<()> {
    let p =
        Profile::by_name(profile).ok_or_else(|| anyhow::anyhow!("unknown profile: {profile}"))?;
    // Load once: avoid a second keychain prompt and use the same credentials
    // for both the displayed client ID and the OAuth exchange.
    let credentials = keychain::load_with_env_fallback(profile)
        .map_err(|err| ApiError::Auth(format!("{err:#}")))?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let report = check_status(&p, credentials, http).await;
    print_status(&mut std::io::stdout().lock(), output, &report)
}

async fn check_status(
    profile: &Profile,
    credentials: Option<(String, String)>,
    http: reqwest::Client,
) -> AuthStatus {
    let mut report = AuthStatus {
        profile: profile.name.clone(),
        api_base_url: profile.api_base_url.clone(),
        authenticated: false,
        client_id: credentials.as_ref().map(|(id, _)| id.clone()),
        merchant_id: None,
    };

    if let Some((client_id, client_secret)) = credentials {
        let fetcher = Arc::new(OAuth2Fetcher {
            oauth_url: profile.oauth_url.clone(),
            client_id,
            client_secret,
            http: http.clone(),
        });
        let api = ApiClient {
            base_url: profile.api_base_url.clone(),
            http,
            tokens: TokenStore::new(fetcher),
        };
        if let Ok(body) = api.ping().await {
            // Older ping responses omit `authenticated`; a successful
            // authenticated request is enough in that case, as in flute-cli.
            report.authenticated = body
                .get("authenticated")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if let Some(id) = body.get("clientId").and_then(|v| v.as_str()) {
                report.client_id = Some(id.to_string());
            }
            report.merchant_id = body
                .get("merchantId")
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
    }

    report
}

fn print_status(
    out: &mut impl Write,
    output: OutputFormat,
    report: &AuthStatus,
) -> anyhow::Result<()> {
    match output {
        OutputFormat::Json => writeln!(out, "{}", serde_json::to_string_pretty(report)?)?,
        OutputFormat::Table => {
            writeln!(out, "Profile:       {}", report.profile)?;
            writeln!(out, "API base:      {}", report.api_base_url)?;
            writeln!(out, "Authenticated: {}", report.authenticated)?;
            writeln!(
                out,
                "Client ID:     {}",
                report.client_id.as_deref().unwrap_or("—")
            )?;
            if let Some(id) = &report.merchant_id {
                writeln!(out, "Merchant ID:   {id}")?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string, header, method, path},
    };

    fn profile(server: &MockServer) -> Profile {
        Profile {
            name: "sandbox".into(),
            api_base_url: server.uri(),
            oauth_url: format!("{}/oauth2/token", server.uri()),
        }
    }

    fn credentials() -> Option<(String, String)> {
        Some(("stored-client".into(), "test-secret".into()))
    }

    fn token_response() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test-bearer", "expires_in": 3600
        }))
    }

    async fn mock_oauth(server: &MockServer, response: ResponseTemplate, calls: u64) {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string(
                "grant_type=client_credentials&client_id=stored-client&client_secret=test-secret",
            ))
            .respond_with(response)
            .expect(calls)
            .mount(server)
            .await;
    }

    async fn mock_ping(server: &MockServer, response: ResponseTemplate, calls: u64) {
        Mock::given(method("GET"))
            .and(path("/pay-int-api/ping"))
            .and(header("authorization", "Bearer test-bearer"))
            .and(header("accept", "application/json"))
            .respond_with(response)
            .expect(calls)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn no_credentials_reports_false_without_network_requests() {
        let server = MockServer::start().await;
        let report = check_status(&profile(&server), None, reqwest::Client::new()).await;
        assert!(!report.authenticated);
        assert!(report.client_id.is_none());
        assert!(report.merchant_id.is_none());
        assert!(server.received_requests().await.unwrap().is_empty());

        let mut out = Vec::new();
        print_status(&mut out, OutputFormat::Json, &report).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["authenticated"], false);
        assert!(value["client_id"].is_null());
        assert!(value["merchant_id"].is_null());
        out.clear();
        print_status(&mut out, OutputFormat::Table, &report).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Authenticated: false\nClient ID:     —\n"));
        assert!(!text.contains("Merchant ID:"));
    }

    #[tokio::test]
    async fn live_status_reports_server_identity_without_exposing_secrets() {
        let server = MockServer::start().await;
        mock_oauth(&server, token_response(), 1).await;
        mock_ping(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({
                "authenticated": true,
                "clientId": "server-client",
                "merchantId": "merchant-123"
            })),
            1,
        )
        .await;
        let report = check_status(&profile(&server), credentials(), reqwest::Client::new()).await;

        let mut out = Vec::new();
        print_status(&mut out, OutputFormat::Json, &report).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            value,
            json!({
                "profile": "sandbox",
                "api_base_url": server.uri(),
                "authenticated": true,
                "client_id": "server-client",
                "merchant_id": "merchant-123"
            })
        );
        for output in [OutputFormat::Table, OutputFormat::Json] {
            out.clear();
            print_status(&mut out, output, &report).unwrap();
            let text = String::from_utf8(out.clone()).unwrap();
            assert!(!text.contains("test-secret"));
            assert!(!text.contains("test-bearer"));
        }
        out.clear();
        print_status(&mut out, OutputFormat::Table, &report).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!(
                "Profile:       sandbox\nAPI base:      {}\nAuthenticated: true\nClient ID:     server-client\nMerchant ID:   merchant-123\n",
                server.uri()
            )
        );
    }

    #[tokio::test]
    async fn older_ping_response_uses_stored_client_id() {
        let server = MockServer::start().await;
        mock_oauth(&server, token_response(), 1).await;
        mock_ping(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})),
            1,
        )
        .await;
        let report = check_status(&profile(&server), credentials(), reqwest::Client::new()).await;
        assert!(report.authenticated);
        assert_eq!(report.client_id.as_deref(), Some("stored-client"));
        assert!(report.merchant_id.is_none());
    }

    #[tokio::test]
    async fn explicit_unauthenticated_ping_response_is_respected() {
        let server = MockServer::start().await;
        mock_oauth(&server, token_response(), 1).await;
        mock_ping(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({"authenticated": false})),
            1,
        )
        .await;
        let report = check_status(&profile(&server), credentials(), reqwest::Client::new()).await;
        assert!(!report.authenticated);
        assert_eq!(report.client_id.as_deref(), Some("stored-client"));
    }

    #[tokio::test]
    async fn rejected_oauth_credentials_report_false_without_calling_ping() {
        let server = MockServer::start().await;
        mock_oauth(
            &server,
            ResponseTemplate::new(401).set_body_json(json!({"error": "invalid_client"})),
            1,
        )
        .await;
        mock_ping(&server, ResponseTemplate::new(200), 0).await;
        let report = check_status(&profile(&server), credentials(), reqwest::Client::new()).await;
        assert!(!report.authenticated);
        assert_eq!(report.client_id.as_deref(), Some("stored-client"));
        assert!(report.merchant_id.is_none());
    }

    #[tokio::test]
    async fn failed_or_malformed_ping_reports_false_and_retries_401_once() {
        for status in [401, 403, 500, 200] {
            let server = MockServer::start().await;
            let calls = if status == 401 { 2 } else { 1 };
            mock_oauth(&server, token_response(), calls).await;
            mock_ping(
                &server,
                ResponseTemplate::new(status).set_body_string("not JSON"),
                calls,
            )
            .await;
            let report =
                check_status(&profile(&server), credentials(), reqwest::Client::new()).await;
            assert!(!report.authenticated, "HTTP {status}");
            assert_eq!(report.client_id.as_deref(), Some("stored-client"));
            assert!(report.merchant_id.is_none());
        }
    }

    #[tokio::test]
    async fn oauth_and_ping_timeouts_report_false() {
        for delay_oauth in [true, false] {
            let server = MockServer::start().await;
            let delayed = Duration::from_secs(1);
            let oauth_delay = if delay_oauth { delayed } else { Duration::ZERO };
            mock_oauth(&server, token_response().set_delay(oauth_delay), 1).await;
            mock_ping(
                &server,
                ResponseTemplate::new(200)
                    .set_body_json(json!({"authenticated": true}))
                    .set_delay(delayed),
                if delay_oauth { 0 } else { 1 },
            )
            .await;
            let http = reqwest::Client::builder()
                .timeout(Duration::from_millis(100))
                .build()
                .unwrap();
            let report = check_status(&profile(&server), credentials(), http).await;
            assert!(!report.authenticated);
            assert_eq!(report.client_id.as_deref(), Some("stored-client"));
        }
    }

    #[tokio::test]
    async fn unknown_profile_fails_before_loading_credentials() {
        let error = status("unknown-profile", OutputFormat::Json)
            .await
            .unwrap_err();
        let envelope = crate::cli::output::ErrorJson::from_anyhow(&error);
        assert_eq!(envelope.kind, "client");
        assert_eq!(envelope.message, "unknown profile: unknown-profile");
    }
}
