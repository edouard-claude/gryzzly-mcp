//! The Gryzzly v1 client: RPC over HTTP, with the API's manners respected.
//!
//! Three things are handled here, once, so no tool has to think about them:
//!
//! * **Rate limiting.** Gryzzly allows 50 requests per 10 seconds per team. A
//!   sliding window paces the process rather than discovering the limit through
//!   429s.
//! * **Retries.** 429 and the transient 5xx family are retried with backoff,
//!   honouring `Retry-After` when it is there.
//! * **The empty-list 404.** A `*.list` action that matches nothing answers
//!   `404 not_found` rather than an empty page. [`Gryzzly::list`] turns that
//!   back into the empty page it means, so "no time logged last week" is not an
//!   error.

use std::{collections::VecDeque, time::Duration};

use reqwest::{StatusCode, header};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{sync::Mutex, time::Instant};

use crate::{
    config::{Config, RATE_LIMIT_REQUESTS, RATE_LIMIT_WINDOW},
    error::{Error, Result},
    model::Page,
};

/// The `User-Agent` every request carries.
pub const USER_AGENT: &str = concat!("mcp-gryzzly/", env!("CARGO_PKG_VERSION"));

/// A client for one Gryzzly team, holding one API key.
#[derive(Debug)]
pub struct Gryzzly {
    http: reqwest::Client,
    config: Config,
    limiter: RateLimiter,
}

impl Gryzzly {
    /// Build a client from a configuration.
    pub fn new(config: Config) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        let mut authorization =
            header::HeaderValue::from_str(&format!("Bearer {}", config.api_key.expose())).map_err(
                |_| Error::config("GRYZZLY_API_KEY contains bytes a header cannot carry"),
            )?;
        authorization.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, authorization);
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );

        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(config.timeout)
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;

        Ok(Self {
            http,
            limiter: RateLimiter::new(RATE_LIMIT_REQUESTS, RATE_LIMIT_WINDOW),
            config,
        })
    }

    /// The configuration this client was built from.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Call an RPC action, for instance `projects.list`, and decode its answer.
    pub async fn call<P, R>(&self, action: &str, params: &P) -> Result<R>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let body = serde_json::to_vec(params).map_err(|source| Error::Decode {
            action: action.to_owned(),
            source,
        })?;
        let bytes = self.send(action, body).await?;
        if bytes.is_empty() {
            return serde_json::from_slice(b"null").map_err(|source| Error::Decode {
                action: action.to_owned(),
                source,
            });
        }
        serde_json::from_slice(&bytes).map_err(|source| Error::Decode {
            action: action.to_owned(),
            source,
        })
    }

    /// Call a `*.list` action, reading its 404 as the empty page it means.
    pub async fn list<P, T>(&self, action: &str, params: &P) -> Result<Page<T>>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        match self.call::<P, Page<T>>(action, params).await {
            Ok(page) => Ok(page),
            Err(Error::Api {
                status: 404,
                ref error_type,
                ..
            }) if error_type.as_deref().unwrap_or("not_found") == "not_found" => {
                tracing::debug!(
                    action,
                    "empty result reported as 404, returning an empty page"
                );
                Ok(Page::empty())
            }
            Err(other) => Err(other),
        }
    }

    /// Fetch every page of a `*.list` action, up to `max` items.
    ///
    /// The API caps `limit` at 1000; anything bigger is walked page by page.
    pub async fn list_all<T>(
        &self,
        action: &str,
        params: &serde_json::Value,
        max: u32,
    ) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        const PAGE: u32 = 1000;
        let mut out = Vec::new();
        let mut offset = 0_u32;
        loop {
            let mut body = params.clone();
            if let Some(object) = body.as_object_mut() {
                object.insert("limit".into(), PAGE.min(max - offset).into());
                object.insert("offset".into(), offset.into());
            }
            let page: Page<T> = self.list(action, &body).await?;
            let received = u32::try_from(page.data.len()).unwrap_or(u32::MAX);
            out.extend(page.data);
            offset += received;
            if received < PAGE || offset >= max || offset >= page.count {
                break;
            }
        }
        Ok(out)
    }

    /// `GET` an export endpoint and return its bytes and content type.
    ///
    /// Gryzzly's two export endpoints authenticate the *browser session*, not
    /// the API key, so with a key they answer `500` with `no cookie` in the
    /// description. That is reported as what it is rather than as a mystery.
    pub async fn download(
        &self,
        action: &str,
        query: &[(&str, String)],
    ) -> Result<(Vec<u8>, String)> {
        let url = format!("{}/{action}", self.config.base_url);
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.limiter.acquire().await;
            let response = self
                .http
                .get(&url)
                .query(query)
                .header(header::ACCEPT, "application/octet-stream, application/json")
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(e) => {
                    let err = Error::Transport(e.to_string());
                    if self.should_retry(&err, attempt) {
                        self.backoff(attempt, None).await;
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = response.status();
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_owned();
            let retry_after = retry_after(&response);
            let bytes = response
                .bytes()
                .await
                .map_err(|e| Error::Transport(e.to_string()))?
                .to_vec();
            if status.is_success() {
                return Ok((bytes, content_type));
            }
            let err = api_error(action, status, &bytes);
            if self.should_retry(&err, attempt) {
                self.backoff(attempt, retry_after).await;
                continue;
            }
            return Err(export_hint(err));
        }
    }

    /// `POST` a raw file body, which is how leave imports are fed.
    pub async fn upload<R>(&self, action: &str, content_type: &str, body: Vec<u8>) -> Result<R>
    where
        R: DeserializeOwned,
    {
        let url = format!("{}/{action}", self.config.base_url);
        self.limiter.acquire().await;
        let response = self
            .http
            .post(&url)
            .header(header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?
            .to_vec();
        if !status.is_success() {
            return Err(api_error(action, status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|source| Error::Decode {
            action: action.to_owned(),
            source,
        })
    }

    async fn send(&self, action: &str, body: Vec<u8>) -> Result<Vec<u8>> {
        let url = format!("{}/{action}", self.config.base_url);
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.limiter.acquire().await;
            tracing::debug!(action, attempt, "gryzzly request");
            let response = self
                .http
                .post(&url)
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.clone())
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(e) => {
                    let err = Error::Transport(e.to_string());
                    if self.should_retry(&err, attempt) {
                        self.backoff(attempt, None).await;
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = response.status();
            let retry_after = retry_after(&response);
            let bytes = response
                .bytes()
                .await
                .map_err(|e| Error::Transport(e.to_string()))?
                .to_vec();
            if status.is_success() {
                return Ok(bytes);
            }
            let err = api_error(action, status, &bytes);
            if self.should_retry(&err, attempt) {
                tracing::warn!(action, attempt, status = status.as_u16(), "retrying");
                self.backoff(attempt, retry_after).await;
                continue;
            }
            return Err(err);
        }
    }

    fn should_retry(&self, err: &Error, attempt: u32) -> bool {
        if attempt > self.config.max_retries {
            return false;
        }
        match err {
            Error::Transport(_) => true,
            Error::Api { status, .. } => matches!(status, 429 | 502 | 503 | 504),
            _ => false,
        }
    }

    async fn backoff(&self, attempt: u32, retry_after: Option<Duration>) {
        let wait = retry_after.unwrap_or_else(|| Duration::from_millis(500 * u64::from(attempt)));
        tokio::time::sleep(wait).await;
    }
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Read Gryzzly's error envelope, or fall back to the raw body.
fn api_error(action: &str, status: StatusCode, body: &[u8]) -> Error {
    #[derive(serde::Deserialize, Default)]
    struct Envelope {
        error_type: Option<String>,
        error_description: Option<String>,
    }
    let envelope: Envelope = serde_json::from_slice(body).unwrap_or_default();
    let description = envelope
        .error_description
        .filter(|d| !d.trim().is_empty())
        .or_else(|| {
            let raw = String::from_utf8_lossy(body).trim().to_owned();
            (!raw.is_empty() && raw != "{}").then(|| truncate(&raw, 300))
        })
        .or_else(|| match status {
            StatusCode::UNAUTHORIZED => Some(
                "invalid or missing API key — check GRYZZLY_API_KEY against Administration → API keys"
                    .to_owned(),
            ),
            _ => None,
        });
    Error::Api {
        action: action.to_owned(),
        status: status.as_u16(),
        error_type: envelope.error_type,
        description,
    }
}

/// Name the upstream limitation instead of letting a 500 look like a bug here.
fn export_hint(err: Error) -> Error {
    match err {
        Error::Api {
            action,
            status,
            error_type,
            description,
        } if description
            .as_deref()
            .is_some_and(|d| d.contains("no cookie")) =>
        {
            Error::Api {
                action,
                status,
                error_type,
                description: Some(
                    "Gryzzly's export endpoints authenticate a browser session, not an API key, so they cannot be called with one. Export from the Gryzzly web app, or read the same data through list_time_entries."
                        .to_owned(),
                ),
            }
        }
        other => other,
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let kept: String = value.chars().take(max).collect();
    format!("{kept}…")
}

/// A sliding window over request timestamps.
#[derive(Debug)]
struct RateLimiter {
    slots: Mutex<VecDeque<Instant>>,
    max: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(max: u32, window: Duration) -> Self {
        Self {
            slots: Mutex::new(VecDeque::with_capacity(max as usize)),
            max: max as usize,
            window,
        }
    }

    /// Wait until sending one more request keeps the process inside the budget.
    async fn acquire(&self) {
        loop {
            let wait = {
                let mut slots = self.slots.lock().await;
                let now = Instant::now();
                while slots
                    .front()
                    .is_some_and(|t| now.duration_since(*t) >= self.window)
                {
                    slots.pop_front();
                }
                if slots.len() < self.max {
                    slots.push_back(now);
                    return;
                }
                slots
                    .front()
                    .map(|oldest| self.window.saturating_sub(now.duration_since(*oldest)))
                    .unwrap_or_default()
            };
            tokio::time::sleep(wait.max(Duration::from_millis(10))).await;
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
    fn an_error_envelope_is_read_into_its_parts() {
        let err = api_error(
            "declarations.create",
            StatusCode::BAD_REQUEST,
            br#"{"error_type":"invalid_argument","error_description":"duration is required"}"#,
        );
        match err {
            Error::Api {
                action,
                status,
                error_type,
                description,
            } => {
                assert_eq!(action, "declarations.create");
                assert_eq!(status, 400);
                assert_eq!(error_type.as_deref(), Some("invalid_argument"));
                assert_eq!(description.as_deref(), Some("duration is required"));
            }
            other => panic!("expected an API error, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_401_body_still_says_what_to_do() {
        let err = api_error("users.list", StatusCode::UNAUTHORIZED, b"{}");
        let message = err.to_string();
        assert!(message.contains("GRYZZLY_API_KEY"), "{message}");
    }

    #[test]
    fn the_export_cookie_failure_is_explained() {
        let err = export_hint(api_error(
            "projects.export",
            StatusCode::INTERNAL_SERVER_ERROR,
            br#"{"error_type":"internal_error","error_description":"unexpected status 401: fail to authenticate: authenticate user: no cookie"}"#,
        ));
        let message = err.to_string();
        assert!(message.contains("browser session"), "{message}");
        assert!(message.contains("list_time_entries"), "{message}");
    }

    #[tokio::test(start_paused = true)]
    async fn the_rate_limiter_paces_the_process_instead_of_being_told_off() {
        let limiter = RateLimiter::new(2, Duration::from_secs(10));
        limiter.acquire().await;
        limiter.acquire().await;
        let started = Instant::now();
        limiter.acquire().await;
        assert!(
            started.elapsed() >= Duration::from_secs(9),
            "the third request waited {:?}",
            started.elapsed()
        );
    }
}
