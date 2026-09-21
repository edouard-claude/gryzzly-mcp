//! A Model Context Protocol server for [Gryzzly](https://www.gryzzly.io) time tracking.
//!
//! The crate is split in two halves:
//!
//! * [`client`] is a plain async client for the Gryzzly v1 HTTP API, with no MCP
//!   in sight: rate limiting, retries, and the API's quirks are handled there.
//! * [`server`] and [`tools`] turn that client into an MCP server: tools,
//!   resources and prompts, built on the official [`rmcp`] SDK.
//!
//! Everything is single tenant: one API key, read from the environment, for the
//! whole process.

pub mod cache;
pub mod client;
pub mod config;
pub mod dates;
pub mod duration;
pub mod error;
pub mod model;
pub mod prompts;
pub mod resolve;
pub mod resources;
pub mod server;
pub mod tools;

pub use client::Gryzzly;
pub use config::Config;
pub use error::{Error, Result};
pub use server::GryzzlyServer;

/// The server name advertised during MCP initialization.
pub const SERVER_NAME: &str = "gryzzly";

/// The crate version, advertised during MCP initialization.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
