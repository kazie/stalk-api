//! Error responses as RFC 9457 Problem Details (`application/problem+json`).
//!
//! Handlers return [`ApiError`]. The [`problem_details`] middleware turns every 4xx/5xx response
//! into a [`ProblemDetails`] body with `instance` set to the request path, including errors actix
//! produces itself (405, unmatched paths, WebSocket handshake and query string failures).

use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use actix_web::error::JsonPayloadError;
use actix_web::http::StatusCode;
use actix_web::http::header::{self, HeaderValue};
use actix_web::middleware::{ErrorHandlerResponse, ErrorHandlers};
use actix_web::{HttpRequest, HttpResponse, ResponseError};
use log::error;
use serde::Serialize;
use std::borrow::Cow;
use std::fmt;
use utoipa::{ToResponse, ToSchema};

/// Media type of every error body.
pub const PROBLEM_JSON: &str = "application/problem+json";

/// An error response following RFC 9457 (Problem Details for HTTP APIs).
#[derive(Serialize, Debug, ToSchema)]
#[schema(example = json!({
    "type": "about:blank",
    "title": "Not Found",
    "status": 404,
    "detail": "No user named 'kazie'",
    "instance": "/api/coords/kazie"
}))]
pub struct ProblemDetails {
    /// Problem type. Always `about:blank`: the HTTP status says what kind of problem it is.
    #[serde(rename = "type")]
    #[schema(example = "about:blank")]
    pub kind: &'static str,
    /// The HTTP reason phrase for `status`.
    #[schema(example = "Not Found")]
    pub title: &'static str,
    /// The HTTP status code, repeated from the response.
    #[schema(example = 404)]
    pub status: u16,
    /// What went wrong in this particular case, meant for people.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "No user named 'kazie'")]
    pub detail: Option<String>,
    /// Path of the request that failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "/api/coords/kazie")]
    pub instance: Option<String>,
    /// Every invalid field in the request body. Only present on validation errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<FieldError>>,
}

/// One invalid field in a request body.
#[derive(Serialize, Debug, Clone, ToSchema)]
pub struct FieldError {
    /// JSON Pointer (RFC 6901), as a URI fragment, to the invalid field.
    #[schema(example = "#/latitude")]
    pub pointer: String,
    /// Why the value was rejected.
    #[schema(example = "must be between -90 and 90")]
    pub detail: String,
}

impl FieldError {
    pub fn new(field: &str, detail: impl Into<String>) -> Self {
        Self {
            pointer: format!("#/{field}"),
            detail: detail.into(),
        }
    }
}

impl ProblemDetails {
    /// A problem with the standard title for `status` and no detail.
    ///
    /// ```
    /// use actix_web::http::StatusCode;
    /// use stalk_api::problem::ProblemDetails;
    ///
    /// let problem = ProblemDetails::new(StatusCode::METHOD_NOT_ALLOWED);
    /// assert_eq!(problem.kind, "about:blank");
    /// assert_eq!(problem.title, "Method Not Allowed");
    /// assert_eq!(problem.status, 405);
    /// ```
    pub fn new(status: StatusCode) -> Self {
        Self {
            kind: "about:blank",
            title: status.canonical_reason().unwrap_or("Error"),
            status: status.as_u16(),
            detail: None,
            instance: None,
            errors: None,
        }
    }

    fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Render as an `application/problem+json` response with the matching status.
    pub fn into_response(self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .content_type(PROBLEM_JSON)
            .json(self)
    }
}

// Reusable OpenAPI responses, so each endpoint documents its errors in one line

/// Bearer token missing or invalid.
#[derive(ToResponse)]
#[response(
    content_type = "application/problem+json",
    headers(("WWW-Authenticate" = String, description = "`Bearer`, or `Bearer error=\"invalid_token\"` for a wrong token")),
    example = json!({
        "type": "about:blank",
        "title": "Unauthorized",
        "status": 401,
        "detail": "Invalid bearer token",
        "instance": "/api/coords"
    })
)]
pub struct UnauthorizedResponse(pub ProblemDetails);

/// No user with this name.
#[derive(ToResponse)]
#[response(
    content_type = "application/problem+json",
    example = json!({
        "type": "about:blank",
        "title": "Not Found",
        "status": 404,
        "detail": "No user named 'kazie'",
        "instance": "/api/coords/kazie"
    })
)]
pub struct UserNotFoundResponse(pub ProblemDetails);

/// Server error. The cause is logged, never returned.
#[derive(ToResponse)]
#[response(
    content_type = "application/problem+json",
    example = json!({
        "type": "about:blank",
        "title": "Internal Server Error",
        "status": 500,
        "detail": "The server failed to handle the request; it has been logged",
        "instance": "/api/coords"
    })
)]
pub struct ServerErrorResponse(pub ProblemDetails);

/// Everything a handler can fail with. Each variant maps to one HTTP status.
#[derive(Debug)]
pub enum ApiError {
    /// The request body parsed but has invalid values.
    Validation(Vec<FieldError>),
    /// The request could not be parsed: malformed JSON, missing or unknown fields.
    BadRequest(String),
    MissingToken,
    InvalidToken,
    NotFound(String),
    PayloadTooLarge,
    UnsupportedMediaType,
    /// Something failed on the server. Details are logged, never sent to the client.
    Internal,
}

impl ApiError {
    fn detail(&self) -> Cow<'_, str> {
        match self {
            ApiError::Validation(errors) if errors.len() == 1 => "1 field is invalid".into(),
            ApiError::Validation(errors) => format!("{} fields are invalid", errors.len()).into(),
            ApiError::BadRequest(detail) | ApiError::NotFound(detail) => detail.into(),
            ApiError::MissingToken => "Missing bearer token".into(),
            ApiError::InvalidToken => "Invalid bearer token".into(),
            ApiError::PayloadTooLarge => "Request body is too large".into(),
            ApiError::UnsupportedMediaType => "Expected Content-Type: application/json".into(),
            ApiError::Internal => {
                "The server failed to handle the request; it has been logged".into()
            }
        }
    }

    pub fn problem(&self) -> ProblemDetails {
        ProblemDetails {
            detail: Some(self.detail().into_owned()),
            errors: match self {
                ApiError::Validation(errors) => Some(errors.clone()),
                _ => None,
            },
            ..ProblemDetails::new(self.status_code())
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail())
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Validation(_) | ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::MissingToken | ApiError::InvalidToken => StatusCode::UNAUTHORIZED,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    // Complete on its own; the middleware re-renders it with `instance` added
    fn error_response(&self) -> HttpResponse {
        let mut res = self.problem().into_response();
        // RFC 6750 section 3: tell the client which scheme to use, and why the token failed
        let challenge = match self {
            ApiError::MissingToken => "Bearer",
            ApiError::InvalidToken => r#"Bearer error="invalid_token""#,
            _ => return res,
        };
        res.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(challenge),
        );
        res
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        error!("Database error: {e:?}");
        ApiError::Internal
    }
}

/// Middleware that gives every 4xx/5xx response a Problem Details body.
///
/// An [`ApiError`] keeps its detail. Any other client error uses the error's message as detail,
/// and any other server error gets none, so internals never leak. `instance` is set to the
/// request path, and headers such as `Allow` and `WWW-Authenticate` are kept.
pub fn problem_details<B: MessageBody + 'static>() -> ErrorHandlers<B> {
    ErrorHandlers::new().default_handler(render_problem)
}

fn render_problem<B: MessageBody + 'static>(
    res: ServiceResponse<B>,
) -> actix_web::Result<ErrorHandlerResponse<B>> {
    let status = res.status();
    let mut problem = match res.response().error() {
        Some(err) => match err.as_error::<ApiError>() {
            Some(api_error) => api_error.problem(),
            None if status.is_client_error() => ProblemDetails {
                detail: Some(err.to_string()),
                ..ProblemDetails::new(status)
            },
            None => ProblemDetails::new(status),
        },
        None => ProblemDetails::new(status),
    };
    problem.instance = Some(res.request().path().to_string());

    let (req, original) = res.into_parts();
    let mut rendered = problem.into_response();
    for (name, value) in original.headers() {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            rendered.headers_mut().append(name.clone(), value.clone());
        }
    }
    Ok(ErrorHandlerResponse::Response(
        ServiceResponse::new(req, rendered).map_into_right_body(),
    ))
}

/// Default service: a 404 for any path no route matches.
pub async fn not_found() -> Result<HttpResponse, ApiError> {
    Err(ApiError::NotFound("No such endpoint".to_string()))
}

/// Error handler for `web::JsonConfig`. Gives a wrong Content-Type its proper 415.
pub fn json_error_handler(err: JsonPayloadError, _req: &HttpRequest) -> actix_web::Error {
    match err {
        JsonPayloadError::ContentType => ApiError::UnsupportedMediaType,
        JsonPayloadError::Overflow { .. } | JsonPayloadError::OverflowKnownLength { .. } => {
            ApiError::PayloadTooLarge
        }
        JsonPayloadError::Deserialize(e) => ApiError::BadRequest(format!("Invalid JSON body: {e}")),
        other => ApiError::BadRequest(other.to_string()),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_type_and_skips_absent_members() {
        let json = serde_json::to_value(ProblemDetails::new(StatusCode::NOT_FOUND)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "about:blank", "title": "Not Found", "status": 404})
        );
    }

    #[test]
    fn validation_lists_every_field() {
        let problem = ApiError::Validation(vec![
            FieldError::new("latitude", "must be between -90 and 90"),
            FieldError::new("heading", "must be between 0 and 360"),
        ])
        .problem();
        let json = serde_json::to_value(problem).unwrap();
        assert_eq!(json["status"], 400);
        assert_eq!(json["title"], "Bad Request");
        assert_eq!(json["detail"], "2 fields are invalid");
        assert_eq!(json["errors"][0]["pointer"], "#/latitude");
        assert_eq!(json["errors"][1]["pointer"], "#/heading");
    }

    #[test]
    fn unauthorized_sets_bearer_challenge() {
        let missing = ApiError::MissingToken.error_response();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            missing.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer"
        );
        assert_eq!(
            missing.headers().get(header::CONTENT_TYPE).unwrap(),
            PROBLEM_JSON
        );

        let invalid = ApiError::InvalidToken.error_response();
        assert_eq!(
            invalid.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            r#"Bearer error="invalid_token""#
        );
    }

    #[test]
    fn internal_hides_details() {
        let err: ApiError = sqlx::Error::RowNotFound.into();
        let problem = err.problem();
        assert_eq!(problem.status, 500);
        assert!(!problem.detail.unwrap().contains("RowNotFound"));
    }
}
