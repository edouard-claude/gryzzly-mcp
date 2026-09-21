//! The `mcp-gryzzly` binary: one MCP server over one Gryzzly API key.
//!
//! By default it speaks the stdio transport, which is what desktop MCP clients
//! spawn. `--http` serves the Streamable HTTP transport instead, for a private
//! deployment. Logs always go to stderr: stdout belongs to the protocol.

#![expect(
    clippy::print_stdout,
    reason = "--help, --version, --check and --list-tools print to stdout and exit before any transport starts; the transports themselves never print"
)]

use std::{net::SocketAddr, sync::Arc};

use axum::response::IntoResponse as _;

use mcp_gryzzly::{
    Config, GryzzlyServer, SERVER_NAME, SERVER_VERSION,
    config::Toolset,
    error::{Error, Result},
};
use rmcp::{
    ServiceExt,
    transport::{
        io::stdio,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use tracing_subscriber::{EnvFilter, fmt};

/// What the command line asked for.
#[derive(Debug, Default)]
struct Args {
    http: Option<String>,
    check: bool,
    list_tools: bool,
    help: bool,
    version: bool,
}

const USAGE: &str = "\
mcp-gryzzly — a Model Context Protocol server for Gryzzly time tracking

USAGE:
    mcp-gryzzly [OPTIONS]

OPTIONS:
    --http <ADDR>     Serve the Streamable HTTP transport on ADDR (e.g. 127.0.0.1:8080)
                      instead of stdio. Same as GRYZZLY_HTTP_BIND.
    --check           Verify the configuration against the API and print a report.
    --list-tools      Print the tools this configuration exposes, then exit.
    -h, --help        Print this help.
    -V, --version     Print the version.

ENVIRONMENT:
    GRYZZLY_API_KEY          Required. Created in Gryzzly: Administration → API keys.
    GRYZZLY_API_KEY_FILE     A file holding the key, for Docker secrets.
    GRYZZLY_BASE_URL         Default https://api.gryzzly.io/v1
    GRYZZLY_USER_ID          The UUID of the user this server acts as.
    GRYZZLY_USER_EMAIL       That user's email, resolved to an id at startup.
    GRYZZLY_DEFAULT_PROJECT  A project name, code or id used when a tool omits one.
    GRYZZLY_TOOLSETS         time,projects,directory,leaves,budget,exports or all
    GRYZZLY_READ_ONLY        1 to withhold every write tool.
    GRYZZLY_TIMEOUT_SECS     Per-request timeout. Default 30.
    GRYZZLY_CACHE_TTL_SECS   How long name lookups are cached. Default 60.
    GRYZZLY_MAX_RETRIES      Retries on 429 and transient 5xx. Default 3.
    GRYZZLY_LOOKBACK_DAYS    How far back a declaration is searched by id. Default 365.
    GRYZZLY_HTTP_BIND        Listen address for the HTTP transport.
    GRYZZLY_HTTP_TOKEN       Bearer token the HTTP transport requires.
    GRYZZLY_LOG              Log filter, e.g. info, debug, mcp_gryzzly=debug.

Documentation: https://github.com/edouard-claude/gryzzly-mcp";

fn main() -> std::process::ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("mcp-gryzzly: {err}");
            return std::process::ExitCode::from(2);
        }
    };

    if args.help {
        println!("{USAGE}");
        return std::process::ExitCode::SUCCESS;
    }
    if args.version {
        println!("{SERVER_NAME} {SERVER_VERSION}");
        return std::process::ExitCode::SUCCESS;
    }

    init_tracing();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("mcp-gryzzly: could not start the async runtime: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(args)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "fatal");
            eprintln!("mcp-gryzzly: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<()> {
    let mut config = Config::from_env()?;
    if let Some(addr) = &args.http {
        config.http_bind = Some(addr.parse::<SocketAddr>().map_err(|e| {
            Error::config(format!(
                "--http expects an address like 127.0.0.1:8080: {e}"
            ))
        })?);
        if let Some(bind) = config
            .http_bind
            .filter(|b| !b.ip().is_loopback() && config.http_token.is_none())
        {
            return Err(Error::config(format!(
                "--http {bind} is reachable from outside this machine: set GRYZZLY_HTTP_TOKEN, or bind to 127.0.0.1"
            )));
        }
    }

    if args.list_tools {
        let server = GryzzlyServer::new(config)?;
        print_tools(&server);
        return Ok(());
    }

    if args.check {
        return check(config).await;
    }

    match config.http_bind {
        Some(bind) => serve_http(config, bind).await,
        None => serve_stdio(config).await,
    }
}

/// Serve the stdio transport, which is what a desktop client spawns.
async fn serve_stdio(config: Config) -> Result<()> {
    let toolsets: Vec<&str> = config.toolsets.iter().map(|t| t.as_str()).collect();
    let server = GryzzlyServer::new(config)?;
    tracing::info!(
        version = SERVER_VERSION,
        tools = server.tools().len(),
        toolsets = toolsets.join(","),
        read_only = server.config().read_only,
        "serving Gryzzly over stdio"
    );
    let running = server
        .serve(stdio())
        .await
        .map_err(|e| Error::Transport(format!("stdio transport: {e}")))?;
    running
        .waiting()
        .await
        .map_err(|e| Error::Transport(format!("stdio session: {e}")))?;
    Ok(())
}

/// Serve the Streamable HTTP transport on `bind`.
async fn serve_http(config: Config, bind: SocketAddr) -> Result<()> {
    let token = config.http_token.clone();
    let factory_config = config.clone();
    let service = StreamableHttpService::new(
        move || {
            GryzzlyServer::new(factory_config.clone())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))
        },
        Arc::new(LocalSessionManager::default()),
        // Host validation guards a local server against DNS rebinding. Behind a
        // reverse proxy the Host header is the public domain, which this process
        // cannot know, so off loopback the bearer token is the guard instead —
        // and the configuration refuses that bind without one.
        if bind.ip().is_loopback() {
            StreamableHttpServerConfig::default().with_allowed_hosts([
                "localhost",
                "127.0.0.1",
                "::1",
                &bind.to_string(),
            ])
        } else {
            StreamableHttpServerConfig::default().disable_allowed_hosts()
        },
    );

    let app = axum::Router::new()
        .route(
            "/healthz",
            axum::routing::get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route_service("/mcp", service)
        .layer(axum::middleware::from_fn(move |request, next| {
            let token = token.clone();
            async move { authorize(token, request, next).await }
        }));

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| Error::config(format!("cannot listen on {bind}: {e}")))?;
    tracing::info!(%bind, "serving Gryzzly over Streamable HTTP at /mcp");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await
        .map_err(|e| Error::Transport(format!("http server: {e}")))?;
    Ok(())
}

/// The HTTP transport's only guard: a bearer token, when one is configured.
///
/// One API key means one identity, so this is not authentication of a person: it
/// is what keeps a private deployment from being an open proxy to a Gryzzly
/// team.
async fn authorize(
    expected: Option<mcp_gryzzly::config::Secret>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::StatusCode;

    let Some(expected) = expected else {
        return next.run(request).await;
    };
    if request.uri().path() == "/healthz" {
        return next.run(request).await;
    }
    let presented = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim);
    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), expected.expose().as_bytes()) => {
            next.run(request).await
        }
        _ => {
            let mut response = (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": "unauthorized",
                    "error_description": "this endpoint requires the bearer token set in GRYZZLY_HTTP_TOKEN",
                })),
            )
                .into_response();
            response.headers_mut().insert(
                axum::http::header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer realm=\"mcp-gryzzly\""),
            );
            response
        }
    }
}

/// Compare two secrets without leaking their common prefix through timing.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Verify the configuration against the live API and report what was found.
async fn check(config: Config) -> Result<()> {
    let toolsets: Vec<&str> = config.toolsets.iter().map(|t| t.as_str()).collect();
    println!("base url     {}", config.base_url);
    println!("toolsets     {}", toolsets.join(", "));
    println!("read only    {}", config.read_only);

    let server = GryzzlyServer::new(config)?;
    println!("tools        {}", server.tools().len());

    let users = server.directory().users().await?;
    println!("team         {} users visible", users.len());

    match server.directory().current_user().await {
        Ok(user) => println!(
            "acting as    {} <{}>",
            user.label(),
            user.email.as_deref().unwrap_or("no email")
        ),
        Err(err) => {
            println!("acting as    unknown — {err}");
            return Err(err);
        }
    }

    let projects = server.directory().projects().await?;
    let active = projects
        .iter()
        .filter(|p| p.status.as_deref() == Some("active"))
        .count();
    println!("projects     {} ({active} active)", projects.len());

    if let Some(default) = &server.config().default_project {
        match server.directory().project(default).await {
            Ok(project) => println!("default      {} ({})", project.label(), project.id),
            Err(err) => println!("default      GRYZZLY_DEFAULT_PROJECT does not resolve — {err}"),
        }
    }

    println!("\nready.");
    Ok(())
}

/// Print the tool table, which is also how the README's table is kept honest.
fn print_tools(server: &GryzzlyServer) {
    let tools = server.tools();
    println!("{} tools", tools.len());
    for tool in tools {
        let write = tool
            .annotations
            .as_ref()
            .and_then(|a| a.read_only_hint)
            .unwrap_or(false);
        let destructive = tool
            .annotations
            .as_ref()
            .and_then(|a| a.destructive_hint)
            .unwrap_or(false);
        let kind = match (write, destructive) {
            (true, _) => "read ",
            (false, true) => "DEL  ",
            (false, false) => "write",
        };
        let description = tool
            .description
            .as_deref()
            .unwrap_or("")
            .split('.')
            .next()
            .unwrap_or("")
            .replace('\n', " ");
        println!("  {kind} {:<26} {}", tool.name, description.trim());
    }
}

/// Logs go to stderr, never to stdout: stdout is the protocol's channel.
fn init_tracing() {
    let filter = EnvFilter::try_from_env("GRYZZLY_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_ansi(false)
        .try_init();
}

fn parse_args(args: impl Iterator<Item = String>) -> std::result::Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--http" => {
                parsed.http = Some(
                    args.next()
                        .ok_or_else(|| "--http needs an address like 127.0.0.1:8080".to_owned())?,
                );
            }
            other if other.starts_with("--http=") => {
                parsed.http = Some(other.trim_start_matches("--http=").to_owned());
            }
            "--check" => parsed.check = true,
            "--list-tools" => parsed.list_tools = true,
            "-h" | "--help" => parsed.help = true,
            "-V" | "--version" => parsed.version = true,
            other => {
                return Err(format!(
                    "unknown argument {other:?}. Run --help for the options."
                ));
            }
        }
    }
    Ok(parsed)
}

/// Keep the toolset names in one place: the help text above is generated from
/// the same list the configuration parses.
#[allow(dead_code, reason = "used by the test below")]
fn toolset_names() -> Vec<&'static str> {
    Toolset::ALL.iter().map(|t| t.as_str()).collect()
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
    fn arguments_parse_in_both_spellings() {
        let args = parse_args(["--http".to_owned(), "127.0.0.1:9000".to_owned()].into_iter())
            .expect("parse");
        assert_eq!(args.http.as_deref(), Some("127.0.0.1:9000"));
        let args = parse_args(["--http=0.0.0.0:80".to_owned()].into_iter()).expect("parse");
        assert_eq!(args.http.as_deref(), Some("0.0.0.0:80"));
    }

    #[test]
    fn an_unknown_argument_points_at_the_help() {
        let err = parse_args(["--nope".to_owned()].into_iter()).unwrap_err();
        assert!(err.contains("--help"), "{err}");
    }

    #[test]
    fn the_help_text_documents_every_toolset() {
        for name in toolset_names() {
            assert!(USAGE.contains(name), "the help text omits {name}");
        }
    }

    #[test]
    fn secrets_are_compared_without_a_timing_shortcut() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
