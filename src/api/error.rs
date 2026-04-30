use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("API {status} (correlation_id={correlation_id:?}): {message}")]
    Api { status: u16, correlation_id: Option<String>, message: String },

    #[error("auth error: {0}")]
    Auth(String),

    #[error("invalid response: {0}")]
    Decode(String),
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct AspNetError {
    pub details: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "correlationId")]
    pub correlation_id: Option<String>,
    #[serde(rename = "errorCode")]
    pub error_code: Option<String>,
}

pub fn from_aspnet(status: u16, body: &str) -> ApiError {
    match serde_json::from_str::<AspNetError>(body) {
        Ok(e) => ApiError::Api {
            status,
            correlation_id: e.correlation_id,
            message: e.title.or(e.details).unwrap_or_else(|| body.to_string()),
        },
        Err(_) => ApiError::Api { status, correlation_id: None, message: body.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_correlation_id_from_aspnet_body() {
        let body = r#"{"details":"X","title":"Validation failed","statusCode":400,"correlationId":"abc-123","errorCode":"V0000"}"#;
        match from_aspnet(400, body) {
            ApiError::Api { status, correlation_id, message } => {
                assert_eq!(status, 400);
                assert_eq!(correlation_id.as_deref(), Some("abc-123"));
                assert_eq!(message, "Validation failed");
            }
            _ => panic!("expected Api"),
        }
    }

    #[test]
    fn falls_back_when_body_is_not_aspnet() {
        match from_aspnet(500, "oops") {
            ApiError::Api { status, correlation_id, message } => {
                assert_eq!(status, 500);
                assert!(correlation_id.is_none());
                assert_eq!(message, "oops");
            }
            _ => panic!(),
        }
    }
}
