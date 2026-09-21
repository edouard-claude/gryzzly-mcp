//! The MCP server: state, capabilities, and the glue between the routers.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::{prompt::PromptRouter, tool::ToolRouter},
    model::{
        Implementation, InitializeResult, ListResourceTemplatesResult, ListResourcesResult,
        PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
        ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};

use crate::{client::Gryzzly, config::Config, resolve::Directory, resources};

/// What the server tells a client about itself, once, at initialization.
///
/// This is the only place a model reads before deciding which tool to call, so
/// it carries the two things it cannot guess: that durations are seconds and
/// times are declared against *tasks*, not projects, and that this Gryzzly key
/// belongs to a team rather than to a person.
pub const INSTRUCTIONS: &str = "\
Gryzzly time tracking. Time is declared against a **task**, never directly \
against a project: resolve a project first, then one of its tasks.

Start with `whoami` to see who the server acts as and how much time is already \
logged this week. `list_projects` and `list_tasks` accept names, codes or ids; \
`log_time` accepts a project or task *name* and a human duration (`2h30`, \
`90m`, `1.5h`), so there is rarely a need to handle identifiers by hand.

Dates accept `today`, `yesterday`, a weekday name, or `YYYY-MM-DD`. Weeks \
accept `this week`, `last week`, or `2026-W38`.

Before logging a week of time, read it first with `list_time_entries` or \
`timesheet`: Gryzzly accepts duplicate declarations happily, and it is the \
caller's job not to create them. Ask the person before writing anything they \
did not explicitly ask for.";

/// One MCP server over one Gryzzly team.
pub struct GryzzlyServer {
    directory: Arc<Directory>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl std::fmt::Debug for GryzzlyServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GryzzlyServer")
            .field("tools", &self.tool_router.list_all().len())
            .finish_non_exhaustive()
    }
}

impl GryzzlyServer {
    /// Build a server from a configuration, wiring only the enabled toolsets.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "one server owns one configuration; taking it by value says so"
    )]
    pub fn new(config: Config) -> crate::error::Result<Self> {
        let client = Arc::new(Gryzzly::new(config.clone())?);
        Ok(Self {
            directory: Arc::new(Directory::new(client)),
            tool_router: crate::tools::router(&config),
            prompt_router: crate::prompts::router(),
        })
    }

    /// The cached view of the team, and the name resolver.
    pub fn directory(&self) -> &Arc<Directory> {
        &self.directory
    }

    /// The HTTP client.
    pub fn client(&self) -> &Arc<Gryzzly> {
        self.directory.client()
    }

    /// The configuration in force.
    pub fn config(&self) -> &Config {
        self.client().config()
    }

    /// Every tool this server exposes, which is what `--list-tools` prints.
    pub fn tools(&self) -> Vec<Tool> {
        let mut tools = self.tool_router.list_all();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }
}

#[rmcp::tool_handler(router = self.tool_router)]
#[rmcp::prompt_handler(router = self.prompt_router)]
impl ServerHandler for GryzzlyServer {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(
            Implementation::new(crate::SERVER_NAME, crate::SERVER_VERSION)
                .with_title("Gryzzly")
                .with_website_url("https://github.com/edouard-claude/gryzzly-mcp"),
        )
        .with_instructions(INSTRUCTIONS)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourcesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult::with_all_items(
            resources::catalogue(),
        )))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourceTemplatesResult, McpError>> + Send + '_
    {
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(
            resources::templates(),
        )))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, McpError> {
        resources::read(self, &request.uri)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }
}
