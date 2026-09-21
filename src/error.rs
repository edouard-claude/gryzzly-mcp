//! The one error type for everything that can go wrong against Gryzzly, and its
//! translation into MCP protocol errors.

use rmcp::ErrorData;
use serde_json::json;

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can fail between the environment and the Gryzzly API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The environment is missing something, or holds something unusable.
    #[error("configuration: {0}")]
    Config(String),

    /// The caller passed something the server refuses before any HTTP call.
    #[error("{0}")]
    Invalid(String),

    /// A name matched several objects and the server will not guess.
    #[error("{message}")]
    Ambiguous {
        /// What was ambiguous, and how to disambiguate it.
        message: String,
        /// The candidates, so the caller can pick one by id.
        candidates: Vec<Candidate>,
    },

    /// A name matched nothing at all.
    #[error("{0}")]
    NotFound(String),

    /// The transport failed: DNS, TLS, timeout, connection reset.
    #[error("transport: {0}")]
    Transport(String),

    /// Gryzzly answered with a 4xx or 5xx.
    #[error("{action} → HTTP {status}{}{}", error_type.as_deref().map(|t| format!(" {t}")).unwrap_or_default(), description.as_deref().map(|d| format!(": {d}")).unwrap_or_default())]
    Api {
        /// The RPC action that failed, for instance `declarations.create`.
        action: String,
        /// The HTTP status code.
        status: u16,
        /// Gryzzly's machine-readable `error_type`, when the body carried one.
        error_type: Option<String>,
        /// Gryzzly's `error_description`, when the body carried one.
        description: Option<String>,
    },

    /// The response was a 2xx but not the JSON this client expected.
    #[error("decoding {action}: {source}")]
    Decode {
        /// The RPC action whose response could not be decoded.
        action: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Reading a local file for an import failed.
    #[error("reading {path}: {source}")]
    Io {
        /// The path that could not be read.
        path: String,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },
}

/// One of several objects a name matched, as reported by [`Error::Ambiguous`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct Candidate {
    /// The object's identifier.
    pub id: String,
    /// The object's human name.
    pub name: String,
}

impl Error {
    /// Shorthand for [`Error::Invalid`].
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    /// Shorthand for [`Error::NotFound`].
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Shorthand for [`Error::Config`].
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// True when retrying the very same request could plausibly succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Api { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// Gryzzly's `error_type`, when this is an API error that carried one.
    pub fn api_error_type(&self) -> Option<&str> {
        match self {
            Self::Api { error_type, .. } => error_type.as_deref(),
            _ => None,
        }
    }
}

/// Translate a crate error into the MCP error the client will see.
///
/// Anything the caller can fix by calling again with different arguments is an
/// `invalid_params`; everything else is an `internal_error` carrying the
/// upstream `error_type` in `data`, so a model can tell a permission problem
/// from a rate limit without parsing prose.
impl From<Error> for ErrorData {
    fn from(err: Error) -> Self {
        let message = err.to_string();
        match err {
            Error::Invalid(_) | Error::NotFound(_) => ErrorData::invalid_params(message, None),
            Error::Ambiguous { candidates, .. } => {
                ErrorData::invalid_params(message, Some(json!({ "candidates": candidates })))
            }
            Error::Config(_) => {
                ErrorData::internal_error(message, Some(json!({ "kind": "configuration" })))
            }
            Error::Api {
                ref action,
                status,
                ref error_type,
                ..
            } => {
                let data = json!({
                    "kind": "gryzzly_api",
                    "action": action,
                    "status": status,
                    "error_type": error_type,
                });
                if status == 400 || status == 404 || status == 422 {
                    ErrorData::invalid_params(message, Some(data))
                } else {
                    ErrorData::internal_error(message, Some(data))
                }
            }
            Error::Transport(_) | Error::Decode { .. } | Error::Io { .. } => {
                ErrorData::internal_error(message, None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    #[test]
    fn ambiguous_carries_candidates_to_the_client() {
        let err = Error::Ambiguous {
            message: "two projects match \"api\"".into(),
            candidates: vec![Candidate {
                id: "a".into(),
                name: "api-one".into(),
            }],
        };
        let data: ErrorData = err.into();
        assert_eq!(data.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        let payload = data.data.unwrap_or_default();
        assert_eq!(payload["candidates"][0]["name"], "api-one");
    }

    #[test]
    fn rate_limits_and_server_errors_are_transient() {
        let rate_limited = Error::Api {
            action: "users.list".into(),
            status: 429,
            error_type: Some("rate_limits".into()),
            description: None,
        };
        assert!(rate_limited.is_transient());
        assert!(!Error::invalid("nope").is_transient());
    }

    #[test]
    fn a_permission_error_stays_an_internal_error_but_names_its_type() {
        let err = Error::Api {
            action: "purchases.list".into(),
            status: 500,
            error_type: Some("no_permission".into()),
            description: Some("permission denied".into()),
        };
        let data: ErrorData = err.into();
        assert_eq!(data.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert_eq!(data.data.unwrap_or_default()["error_type"], "no_permission");
    }
}
