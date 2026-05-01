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

// The Flute API returns errors in two casings: camelCase from the public-API
// layer and PascalCase from internal exception handlers (e.g. 500s with
// "Title", "CorrelationId" capitalized). Accept both via serde alias so we
// extract the correlation id from either response shape.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct AspNetError {
    #[serde(alias = "Details")]
    pub details: Option<String>,
    #[serde(alias = "Title")]
    pub title: Option<String>,
    #[serde(rename = "correlationId", alias = "CorrelationId")]
    pub correlation_id: Option<String>,
    #[serde(rename = "errorCode", alias = "ErrorCode")]
    #[allow(dead_code)]
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

    #[test]
    fn extracts_correlation_id_from_pascal_case_body() {
        // Internal 500s from the API come back PascalCase.
        let body = r#"{"Details":"Value cannot be null. (Parameter 'uriString')","StatusCode":500,"Source":"IsvApiBff","ExceptionType":"ArgumentNullException","CorrelationId":"45d859f6-dc38-4d8f-8bab-ae4e20036919","ErrorCode":"I0000","Title":"Internal server error"}"#;
        match from_aspnet(500, body) {
            ApiError::Api { status, correlation_id, message } => {
                assert_eq!(status, 500);
                assert_eq!(correlation_id.as_deref(), Some("45d859f6-dc38-4d8f-8bab-ae4e20036919"));
                assert_eq!(message, "Internal server error");
            }
            _ => panic!("expected Api"),
        }
    }
}
