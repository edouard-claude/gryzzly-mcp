//! Configuration, read once from the environment at startup.
//!
//! Nothing here reads the environment lazily: a missing or malformed variable
//! stops the process with a message that names the variable, rather than
//! surfacing as a confusing tool failure three calls later.

use std::{collections::BTreeSet, fmt, net::SocketAddr, str::FromStr, time::Duration};

use crate::error::{Error, Result};

/// The default Gryzzly API root, versioned prefix included.
pub const DEFAULT_BASE_URL: &str = "https://api.gryzzly.io/v1";

/// Gryzzly's documented budget: 50 requests per 10 seconds, per team.
pub const RATE_LIMIT_REQUESTS: u32 = 50;
/// The window [`RATE_LIMIT_REQUESTS`] applies to.
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);

/// A string that never shows up in a log or a panic message.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a value so it is redacted when formatted.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself. Only the HTTP layer should need this.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// A group of tools, so a deployment can expose only what it needs.
///
/// Tool count is context: a client that only ever logs time has no use for the
/// twenty administrative tools, and every unused schema is tokens spent on
/// every request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Toolset {
    /// Declarations, timesheets, and the projects and tasks needed to fill them.
    Time,
    /// Project and task administration, plus project metrics.
    Projects,
    /// Users, customers, groups, groupsets, tags and tagsets.
    Directory,
    /// Leave periods, including the file import.
    Leaves,
    /// Purchases and discounts, which Gryzzly gates behind its budget offer.
    Budget,
    /// The two export endpoints.
    Exports,
}

impl Toolset {
    /// Every toolset, in the order they are documented.
    pub const ALL: [Self; 6] = [
        Self::Time,
        Self::Projects,
        Self::Directory,
        Self::Leaves,
        Self::Budget,
        Self::Exports,
    ];

    /// The name used in `GRYZZLY_TOOLSETS`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Projects => "projects",
            Self::Directory => "directory",
            Self::Leaves => "leaves",
            Self::Budget => "budget",
            Self::Exports => "exports",
        }
    }
}

impl FromStr for Toolset {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "time" | "declarations" => Ok(Self::Time),
            "projects" | "tasks" => Ok(Self::Projects),
            "directory" | "users" | "admin" => Ok(Self::Directory),
            "leaves" | "leave" => Ok(Self::Leaves),
            "budget" | "purchases" | "discounts" => Ok(Self::Budget),
            "exports" | "export" => Ok(Self::Exports),
            other => Err(Error::config(format!(
                "GRYZZLY_TOOLSETS: unknown toolset {other:?}, expected one of time, projects, directory, leaves, budget, exports, or all"
            ))),
        }
    }
}

impl fmt::Display for Toolset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything the process needs to know, resolved from the environment.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Config {
    /// The Gryzzly API key, sent as `Authorization: Bearer …`.
    pub api_key: Secret,
    /// The API root, prefix included. No trailing slash.
    pub base_url: String,
    /// The identifier of the user the server acts as, when known.
    pub user_id: Option<String>,
    /// The email of the user the server acts as, resolved to an id at startup.
    pub user_email: Option<String>,
    /// A project name, code or id used when a tool omits one.
    pub default_project: Option<String>,
    /// The toolsets to expose.
    pub toolsets: BTreeSet<Toolset>,
    /// When true, no write tool is registered at all.
    pub read_only: bool,
    /// Per-request timeout.
    pub timeout: Duration,
    /// How long lists of projects, tasks and users are reused for name lookups.
    pub cache_ttl: Duration,
    /// How many times a transient failure is retried.
    pub max_retries: u32,
    /// How far back a declaration is looked for when only its id is known.
    pub lookback_days: i64,
    /// Listen address for the Streamable HTTP transport, when not stdio.
    pub http_bind: Option<SocketAddr>,
    /// Bearer token required by the HTTP transport.
    pub http_token: Option<Secret>,
}

impl Config {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_getter(|key| std::env::var(key).ok())
    }

    /// Read the configuration from an arbitrary source, which is what the tests
    /// use.
    pub fn from_getter(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get_trimmed = |key: &str| -> Option<String> {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };

        let api_key = match get_trimmed("GRYZZLY_API_KEY") {
            Some(key) => key,
            None => match get_trimmed("GRYZZLY_API_KEY_FILE") {
                Some(path) => std::fs::read_to_string(&path)
                    .map_err(|source| Error::Io { path, source })?
                    .trim()
                    .to_owned(),
                None => {
                    return Err(Error::config(
                        "GRYZZLY_API_KEY is required (or GRYZZLY_API_KEY_FILE pointing at a file holding it). Create a key in Gryzzly: Administration → API keys.",
                    ));
                }
            },
        };
        if api_key.is_empty() {
            return Err(Error::config("GRYZZLY_API_KEY is empty"));
        }

        let base_url = get_trimmed("GRYZZLY_BASE_URL")
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned())
            .trim_end_matches('/')
            .to_owned();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(Error::config(format!(
                "GRYZZLY_BASE_URL must start with http:// or https://, got {base_url:?}"
            )));
        }

        let user_id = get_trimmed("GRYZZLY_USER_ID");
        if let Some(id) = user_id.as_deref().filter(|id| !looks_like_uuid(id)) {
            return Err(Error::config(format!(
                "GRYZZLY_USER_ID must be a UUID, got {id:?}"
            )));
        }

        let toolsets = parse_toolsets(get_trimmed("GRYZZLY_TOOLSETS").as_deref())?;
        let read_only = parse_bool(get_trimmed("GRYZZLY_READ_ONLY").as_deref(), false)?;

        let http_bind = match get_trimmed("GRYZZLY_HTTP_BIND") {
            Some(addr) => Some(addr.parse::<SocketAddr>().map_err(|e| {
                Error::config(format!("GRYZZLY_HTTP_BIND is not a socket address: {e}"))
            })?),
            None => None,
        };
        let http_token = get_trimmed("GRYZZLY_HTTP_TOKEN").map(Secret::new);
        if let Some(addr) = http_bind.filter(|a| !a.ip().is_loopback() && http_token.is_none()) {
            return Err(Error::config(format!(
                "GRYZZLY_HTTP_BIND is {addr}, which is reachable from outside this machine: set GRYZZLY_HTTP_TOKEN so the endpoint requires a bearer token, or bind to 127.0.0.1"
            )));
        }

        Ok(Self {
            api_key: Secret::new(api_key),
            base_url,
            user_id,
            user_email: get_trimmed("GRYZZLY_USER_EMAIL").map(|e| e.to_ascii_lowercase()),
            default_project: get_trimmed("GRYZZLY_DEFAULT_PROJECT"),
            toolsets,
            read_only,
            timeout: Duration::from_secs(parse_u64(
                "GRYZZLY_TIMEOUT_SECS",
                get_trimmed("GRYZZLY_TIMEOUT_SECS").as_deref(),
                30,
            )?),
            cache_ttl: Duration::from_secs(parse_u64(
                "GRYZZLY_CACHE_TTL_SECS",
                get_trimmed("GRYZZLY_CACHE_TTL_SECS").as_deref(),
                60,
            )?),
            max_retries: u32::try_from(parse_u64(
                "GRYZZLY_MAX_RETRIES",
                get_trimmed("GRYZZLY_MAX_RETRIES").as_deref(),
                3,
            )?)
            .map_err(|_| Error::config("GRYZZLY_MAX_RETRIES is too large"))?,
            lookback_days: i64::try_from(parse_u64(
                "GRYZZLY_LOOKBACK_DAYS",
                get_trimmed("GRYZZLY_LOOKBACK_DAYS").as_deref(),
                365,
            )?)
            .map_err(|_| Error::config("GRYZZLY_LOOKBACK_DAYS is too large"))?,
            http_bind,
            http_token,
        })
    }

    /// True when the given toolset is exposed.
    pub fn has(&self, toolset: Toolset) -> bool {
        self.toolsets.contains(&toolset)
    }

    /// How the current user was designated, for messages and for `whoami`.
    pub fn identity_hint(&self) -> Option<String> {
        match (&self.user_id, &self.user_email) {
            (Some(id), _) => Some(format!("GRYZZLY_USER_ID={id}")),
            (None, Some(email)) => Some(format!("GRYZZLY_USER_EMAIL={email}")),
            (None, None) => None,
        }
    }
}

fn parse_toolsets(raw: Option<&str>) -> Result<BTreeSet<Toolset>> {
    let Some(raw) = raw else {
        return Ok(Toolset::ALL.into_iter().collect());
    };
    if raw.eq_ignore_ascii_case("all") {
        return Ok(Toolset::ALL.into_iter().collect());
    }
    let mut set = BTreeSet::new();
    for part in raw.split(',').filter(|p| !p.trim().is_empty()) {
        if part.trim().eq_ignore_ascii_case("all") {
            set.extend(Toolset::ALL);
            continue;
        }
        set.insert(part.parse::<Toolset>()?);
    }
    if set.is_empty() {
        return Err(Error::config(
            "GRYZZLY_TOOLSETS is set but selects no toolset",
        ));
    }
    Ok(set)
}

fn parse_bool(raw: Option<&str>, default: bool) -> Result<bool> {
    match raw {
        None => Ok(default),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            other => Err(Error::config(format!(
                "expected a boolean (1/0, true/false), got {other:?}"
            ))),
        },
    }
}

fn parse_u64(name: &str, raw: Option<&str>, default: u64) -> Result<u64> {
    match raw {
        None => Ok(default),
        Some(v) => v
            .parse::<u64>()
            .map_err(|_| Error::config(format!("{name} must be a positive integer, got {v:?}"))),
    }
}

/// A shape check, not a parse: the server never interprets the bytes of an id.
pub fn looks_like_uuid(value: &str) -> bool {
    value.len() == 36
        && value.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
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

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn a_missing_api_key_names_the_variable_and_where_to_get_one() {
        let err = Config::from_getter(env(&[])).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("GRYZZLY_API_KEY"), "{message}");
        assert!(message.contains("API keys"), "{message}");
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let cfg = Config::from_getter(env(&[("GRYZZLY_API_KEY", "k")])).unwrap();
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
        assert_eq!(cfg.toolsets.len(), Toolset::ALL.len());
        assert!(!cfg.read_only);
        assert_eq!(cfg.timeout, Duration::from_secs(30));
    }

    #[test]
    fn the_api_key_is_never_printed() {
        let cfg = Config::from_getter(env(&[("GRYZZLY_API_KEY", "super-secret")])).unwrap();
        let printed = format!("{cfg:?}");
        assert!(!printed.contains("super-secret"), "{printed}");
        assert_eq!(cfg.api_key.expose(), "super-secret");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_dropped() {
        let cfg = Config::from_getter(env(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_BASE_URL", "https://example.test/v1/"),
        ]))
        .unwrap();
        assert_eq!(cfg.base_url, "https://example.test/v1");
    }

    #[test]
    fn a_base_url_without_a_scheme_is_refused() {
        let err = Config::from_getter(env(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_BASE_URL", "api.gryzzly.io"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("http://"), "{err}");
    }

    #[test]
    fn toolsets_accept_aliases_and_all() {
        let cfg = Config::from_getter(env(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_TOOLSETS", "time, admin"),
        ]))
        .unwrap();
        assert!(cfg.has(Toolset::Time));
        assert!(cfg.has(Toolset::Directory));
        assert!(!cfg.has(Toolset::Budget));
    }

    #[test]
    fn an_unknown_toolset_lists_the_valid_ones() {
        let err = Config::from_getter(env(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_TOOLSETS", "timesheets"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("directory"), "{err}");
    }

    #[test]
    fn a_non_uuid_user_id_is_refused_at_startup() {
        let err = Config::from_getter(env(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_USER_ID", "edouard"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("UUID"), "{err}");
    }

    #[test]
    fn binding_http_off_loopback_without_a_token_is_refused() {
        let err = Config::from_getter(env(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_HTTP_BIND", "0.0.0.0:8080"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("GRYZZLY_HTTP_TOKEN"), "{err}");

        let ok = Config::from_getter(env(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_HTTP_BIND", "127.0.0.1:8080"),
        ]));
        assert!(ok.is_ok());
    }

    #[test]
    fn uuid_shape_check() {
        assert!(looks_like_uuid("2d5b507c-d5ff-49c8-9fd3-cb8778bc6611"));
        assert!(!looks_like_uuid("2d5b507c-d5ff-49c8-9fd3-cb8778bc661"));
        assert!(!looks_like_uuid("2d5b507cxd5ff-49c8-9fd3-cb8778bc6611"));
    }
}
