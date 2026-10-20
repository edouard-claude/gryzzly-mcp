//! The MCP tools, one module per toolset.
//!
//! Every Gryzzly v1 endpoint is reachable: 50 actions, plus three tools
//! (`whoami`, `log_time_bulk`, `timesheet`) that exist because the API has no
//! `/me`, no batch, and no aggregate, and a person filling a timesheet needs
//! all three.
//!
//! Naming follows the same rule everywhere: `verb_noun`, and the doc comment of
//! the function is the description a model reads.

pub mod budget;
pub mod directory;
pub mod exports;
pub mod leaves;
pub mod projects;
pub mod time;

use rmcp::{ErrorData as McpError, Json, handler::server::router::tool::ToolRouter};
use serde::Serialize;

use crate::{
    config::{Config, Toolset},
    error::Error,
    server::GryzzlyServer,
};

/// Assemble the tool router the configuration asks for.
///
/// Read-only mode is not a list of tool names to keep in sync: it drops every
/// route that does not annotate itself `readOnlyHint`, so a new write tool is
/// excluded by construction.
pub fn router(config: &Config) -> ToolRouter<GryzzlyServer> {
    let mut router = ToolRouter::new();
    if config.has(Toolset::Time) {
        router += GryzzlyServer::time_router();
    }
    // Reading the catalogue is what makes time loggable, so it comes with the
    // time toolset as much as with the project one.
    if config.has(Toolset::Time) || config.has(Toolset::Projects) {
        router += GryzzlyServer::catalog_router();
    }
    if config.has(Toolset::Projects) {
        router += GryzzlyServer::projects_router();
    }
    if config.has(Toolset::Directory) {
        router += GryzzlyServer::directory_router();
    }
    if config.has(Toolset::Leaves) {
        router += GryzzlyServer::leaves_router();
    }
    if config.has(Toolset::Budget) {
        router += GryzzlyServer::budget_router();
    }
    if config.has(Toolset::Exports) {
        router += GryzzlyServer::exports_router();
    }

    if config.read_only {
        let writers: Vec<String> = router
            .list_all()
            .into_iter()
            .filter(|tool| {
                tool.annotations
                    .as_ref()
                    .and_then(|a| a.read_only_hint)
                    .unwrap_or(false)
                    .eq(&false)
            })
            .map(|tool| tool.name.to_string())
            .collect();
        for name in writers {
            router.remove_route(&name);
        }
    }

    router
}

/// What a `*.list` tool answers with.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Listing<T> {
    /// How many objects match the filters upstream, beyond this page.
    pub count: u32,
    /// How many objects this answer carries.
    pub returned: usize,
    /// The objects.
    pub items: Vec<T>,
    /// Set when the answer is worth a word of explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl<T> Listing<T> {
    /// Wrap a page of objects.
    pub fn new(count: u32, items: Vec<T>) -> Self {
        Self {
            count,
            returned: items.len(),
            items,
            note: None,
        }
    }

    /// Wrap objects that were not paginated upstream.
    pub fn all(items: Vec<T>) -> Self {
        let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
        Self::new(count, items)
    }

    /// Wrap objects that a local filter has been applied to.
    ///
    /// Gryzzly's `count` answers the filters it knows about; a filter applied
    /// here would make that number a lie, so it is replaced and the upstream
    /// figure moves into the note.
    pub fn filtered(upstream_count: u32, upstream_len: usize, items: Vec<T>) -> Self {
        if items.len() == upstream_len {
            return Self::new(upstream_count, items);
        }
        let kept = items.len();
        Self::new(u32::try_from(kept).unwrap_or(u32::MAX), items).with_note(format!(
            "filtered here: {kept} of the {upstream_len} objects on this page, out of {upstream_count} upstream"
        ))
    }

    /// Add the word of explanation.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// What a delete tool answers with.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Deleted {
    /// Always true: a failure is an error, not a `false`.
    pub deleted: bool,
    /// What was deleted.
    pub id: String,
    /// The kind of object that was deleted.
    pub kind: String,
}

impl Deleted {
    /// Report a successful delete.
    pub fn new(kind: &str, id: impl Into<String>) -> Self {
        Self {
            deleted: true,
            id: id.into(),
            kind: kind.to_owned(),
        }
    }
}

/// Pagination, the same on every list tool.
#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    /// How many objects to return. Upstream maximum is 1000.
    pub limit: Option<u32>,
    /// How many objects to skip, for the next page.
    pub offset: Option<u32>,
}

impl PageArgs {
    /// Fold the pagination into a request body.
    pub fn apply(&self, body: &mut serde_json::Map<String, serde_json::Value>) {
        body.insert(
            "limit".into(),
            self.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT).into(),
        );
        if let Some(offset) = self.offset {
            body.insert("offset".into(), offset.into());
        }
    }
}

/// The page size used when a tool asks for none.
pub const DEFAULT_LIMIT: u32 = 100;
/// The page size Gryzzly refuses to exceed.
pub const MAX_LIMIT: u32 = 1000;

/// A request body under construction.
///
/// Gryzzly reads an explicit `null` as "reset to the default", so an absent
/// argument must be absent from the body, not null. This builder only ever adds
/// what it was given.
#[derive(Debug, Default)]
pub struct Body(serde_json::Map<String, serde_json::Value>);

impl Body {
    /// An empty body, which is what an action with no parameters wants.
    pub fn new() -> Self {
        Self(serde_json::Map::new())
    }

    /// Set a field.
    pub fn set(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.0.insert(key.to_owned(), value.into());
        self
    }

    /// Set a field, when there is something to set.
    pub fn set_opt<V: Into<serde_json::Value>>(mut self, key: &str, value: Option<V>) -> Self {
        if let Some(value) = value {
            self.0.insert(key.to_owned(), value.into());
        }
        self
    }

    /// Set a list of strings, when it is not empty.
    pub fn set_list(mut self, key: &str, values: Option<Vec<String>>) -> Self {
        if let Some(values) = values {
            self.0.insert(key.to_owned(), values.into());
        }
        self
    }

    /// Fold pagination in.
    #[must_use]
    pub fn paged(mut self, page: &PageArgs) -> Self {
        page.apply(&mut self.0);
        self
    }

    /// The body, ready to send.
    pub fn build(self) -> serde_json::Value {
        serde_json::Value::Object(self.0)
    }
}

/// Parse an optional date argument into the `YYYY-MM-DD` the API wants.
pub fn opt_date(raw: Option<&str>) -> Result<Option<String>, Error> {
    match raw.map(str::trim).filter(|d| !d.is_empty()) {
        Some(raw) => Ok(Some(crate::dates::api_date(crate::dates::parse_date(raw)?))),
        None => Ok(None),
    }
}

/// Parse a required date argument.
pub fn date(raw: &str) -> Result<String, Error> {
    Ok(crate::dates::api_date(crate::dates::parse_date(raw)?))
}

/// Wrap a value as a structured tool result.
pub fn ok<T>(value: T) -> Result<Json<T>, McpError>
where
    T: Serialize + schemars::JsonSchema + 'static,
{
    Ok(Json(value))
}

/// Refuse a destructive call that was not confirmed.
///
/// Deletes are the only irreversible thing this server can do, so they are
/// fail-closed: without `confirm: true` nothing leaves the process.
pub fn require_confirmation(confirm: bool, what: &str) -> Result<(), McpError> {
    if confirm {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "{what} would be deleted and Gryzzly has no undo. Show what is about to go, get the person's explicit agreement, then call again with confirm: true."
    ))
    .into())
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
    use crate::config::Config;

    fn config(pairs: &[(&str, &str)]) -> Config {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_getter(|key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
            .expect("test configuration")
    }

    #[test]
    fn every_endpoint_of_the_api_has_a_tool() {
        let router = router(&config(&[("GRYZZLY_API_KEY", "k")]));
        // 50 documented actions, plus whoami, log_time_bulk and timesheet.
        assert_eq!(router.list_all().len(), 53, "{:?}", tool_names(&router));
    }

    #[test]
    fn a_toolset_selection_only_exposes_that_toolset() {
        let router = router(&config(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_TOOLSETS", "time"),
        ]));
        let names = tool_names(&router);
        assert!(names.contains(&"log_time".to_owned()));
        assert!(!names.contains(&"invite_user".to_owned()));
        assert!(!names.contains(&"create_purchase".to_owned()));
    }

    #[test]
    fn read_only_mode_keeps_only_tools_that_declare_themselves_read_only() {
        let router = router(&config(&[
            ("GRYZZLY_API_KEY", "k"),
            ("GRYZZLY_READ_ONLY", "1"),
        ]));
        for tool in router.list_all() {
            assert_eq!(
                tool.annotations.as_ref().and_then(|a| a.read_only_hint),
                Some(true),
                "{} survived read-only mode",
                tool.name
            );
        }
        let names = tool_names(&router);
        assert!(names.contains(&"list_time_entries".to_owned()));
        assert!(!names.contains(&"log_time".to_owned()));
        assert!(!names.contains(&"delete_time_entry".to_owned()));
    }

    #[test]
    fn every_tool_is_named_annotated_and_described() {
        let router = router(&config(&[("GRYZZLY_API_KEY", "k")]));
        for tool in router.list_all() {
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no annotations", tool.name));
            assert!(
                annotations.title.is_some(),
                "{} has no human title",
                tool.name
            );
            assert!(
                annotations.read_only_hint.is_some(),
                "{} does not say whether it writes",
                tool.name
            );
            let description = tool
                .description
                .as_deref()
                .unwrap_or_else(|| panic!("{} has no description", tool.name));
            assert!(
                description.len() > 30,
                "{} has a description too short to be useful: {description:?}",
                tool.name
            );
            assert!(
                tool.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "{} is not snake_case",
                tool.name
            );
            assert!(
                tool.output_schema.is_some(),
                "{} has no output schema",
                tool.name
            );
        }
    }

    #[test]
    fn every_description_belongs_to_its_tool() {
        // A description glued to the wrong function is invisible to the
        // compiler and fatal to a model: `log_time` described as a delete is a
        // tool nobody should call. The opening verb says what a tool does, so
        // it has to agree with the annotation the tool declares.
        const READS: &[&str] = &["List", "Read", "Summarise", "Who", "Download"];
        const WRITES: &[&str] = &[
            "Create", "Record", "Grant", "Declare", "Delete", "Change", "Invite", "Import",
        ];
        let router = router(&config(&[("GRYZZLY_API_KEY", "k")]));
        for tool in router.list_all() {
            let description = tool.description.as_deref().unwrap_or_default();
            let verb = description.split_whitespace().next().unwrap_or_default();
            let read_only = tool
                .annotations
                .as_ref()
                .and_then(|a| a.read_only_hint)
                .unwrap_or(false);
            let destructive = tool
                .annotations
                .as_ref()
                .and_then(|a| a.destructive_hint)
                .unwrap_or(false);
            match (READS.contains(&verb), WRITES.contains(&verb)) {
                (true, false) => assert!(read_only, "{} reads ({verb}) but may write", tool.name),
                (false, true) => assert!(
                    !read_only,
                    "{} writes ({verb}) but says it reads",
                    tool.name
                ),
                _ => panic!(
                    "{} opens with {verb:?}, which says neither read nor write",
                    tool.name
                ),
            }
            assert_eq!(
                destructive,
                verb == "Delete",
                "{}: destructiveHint and a Delete description must go together",
                tool.name
            );
            let subject = tool
                .name
                .split_once('_')
                .map_or(tool.name.as_ref(), |(_, s)| s);
            let stem = subject.split('_').next().unwrap_or(subject);
            let stem = stem.trim_end_matches('s');
            assert!(
                tool.name == "whoami"
                    || tool.name == "timesheet"
                    || description.to_lowercase().contains(stem),
                "{}: the description never mentions {stem:?}: {description:?}",
                tool.name
            );
        }
    }

    #[test]
    fn a_delete_without_confirmation_says_what_to_do() {
        let err = require_confirmation(false, "this declaration").unwrap_err();
        assert!(err.message.contains("confirm: true"), "{}", err.message);
        assert!(require_confirmation(true, "x").is_ok());
    }

    fn tool_names(router: &ToolRouter<GryzzlyServer>) -> Vec<String> {
        router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }
}
