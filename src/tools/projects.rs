//! Projects and tasks: the catalogue time is declared against, and its
//! administration.
//!
//! The read tools live in their own router because the time toolset needs them:
//! you cannot log time against a task you cannot find.

use rmcp::{ErrorData as McpError, Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};

use crate::{
    error::Error,
    model::{Page, Project, ProjectMetrics, Task},
    server::GryzzlyServer,
    tools::{Body, Deleted, Listing, PageArgs, date, ok, opt_date, require_confirmation},
};

/// The status a project can be in.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    /// Running.
    Active,
    /// Finished, still readable.
    Done,
    /// Out of the way.
    Archived,
}

impl ProjectStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Archived => "archived",
        }
    }
}

/// Who can declare time on a project.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Everyone in the team.
    Public,
    /// Only the listed contributors.
    Private,
}

impl Visibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

/// How a task's budget is expressed.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BudgetType {
    /// No budget at all.
    None,
    /// A budget per contributor and rate.
    Detailed,
    /// One amount for the whole task.
    Global,
    /// A budget expressed in time.
    Time,
}

impl BudgetType {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Detailed => "detailed",
            Self::Global => "global",
            Self::Time => "time",
        }
    }
}

/// Where a task's hourly rate comes from.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HourlyRateMode {
    /// The rate carried by the task.
    Task,
    /// The rate of the contributor's group.
    Group,
    /// The contributor's own rate.
    Contributor,
}

impl HourlyRateMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Group => "group",
            Self::Contributor => "contributor",
        }
    }
}

/// Arguments of `list_projects`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListProjectsArgs {
    /// Keep only these statuses. Defaults to every status.
    pub status: Option<Vec<ProjectStatus>>,
    /// Keep only projects of this customer, by name or id.
    pub customer: Option<String>,
    /// Keep only projects whose name or code contains this text.
    pub query: Option<String>,
    /// Pagination.
    #[serde(flatten)]
    pub page: PageArgs,
}

/// Arguments of a tool that takes one identifier.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ProjectRef {
    /// The project, by name, code or id.
    pub project: String,
}

/// Arguments of `list_tasks`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListTasksArgs {
    /// Keep only tasks of this project, by name, code or id.
    pub project: Option<String>,
    /// Keep only tasks whose name contains this text.
    pub query: Option<String>,
    /// Include container tasks, which hold other tasks and take no time.
    /// Defaults to false.
    pub include_containers: Option<bool>,
    /// Include tasks already completed. Defaults to true.
    pub include_completed: Option<bool>,
    /// Pagination.
    #[serde(flatten)]
    pub page: PageArgs,
}

/// Arguments of `get_task`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct TaskRef {
    /// The task, by name or id.
    pub task: String,
    /// The project the task belongs to, to disambiguate a name.
    pub project: Option<String>,
}

/// Arguments of `list_projects_metrics`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ProjectsMetricsArgs {
    /// The projects to measure, by name, code or id.
    pub projects: Vec<String>,
}

/// Arguments of `create_project`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CreateProjectArgs {
    /// The project's name.
    pub name: String,
    /// When the project starts. Required by Gryzzly. Defaults to today.
    pub start_at: Option<String>,
    /// When the project is supposed to end.
    pub end_at: Option<String>,
    /// The project's short code.
    pub code: Option<String>,
    /// The customer, by name or id.
    pub customer: Option<String>,
    /// Whether the project is billable.
    pub is_billable: Option<bool>,
    /// `public` (everyone) or `private` (contributors only).
    pub visibility: Option<Visibility>,
    /// The contributors, by name, email or id. Must be empty on a public project.
    pub contributors: Option<Vec<String>>,
    /// The managers, by name, email or id.
    pub managers: Option<Vec<String>>,
    /// Group identifiers to assign.
    pub groups: Option<Vec<String>>,
    /// Tag identifiers to attach.
    pub tags: Option<Vec<String>>,
}

/// Arguments of `update_project`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateProjectArgs {
    /// The project to change, by name, code or id.
    pub project: String,
    /// A new name.
    pub name: Option<String>,
    /// A new code.
    pub code: Option<String>,
    /// A new status.
    pub status: Option<ProjectStatus>,
    /// A new start date. Gryzzly requires one on every update; the current one
    /// is sent when this is absent.
    pub start_at: Option<String>,
    /// A new end date.
    pub end_at: Option<String>,
    /// A new customer, by name or id.
    pub customer: Option<String>,
    /// Whether the project is billable.
    pub is_billable: Option<bool>,
    /// A new visibility.
    pub visibility: Option<Visibility>,
    /// The contributors, by name, email or id. Replaces the current list.
    pub contributors: Option<Vec<String>>,
    /// The managers, by name, email or id. Replaces the current list.
    pub managers: Option<Vec<String>>,
    /// Group identifiers. Replaces the current list.
    pub groups: Option<Vec<String>>,
    /// Tag identifiers. Replaces the current list.
    pub tags: Option<Vec<String>>,
}

/// Arguments of `delete_project`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteProjectArgs {
    /// The project to delete, by name, code or id.
    pub project: String,
    /// Must be `true`. Without it nothing is deleted.
    pub confirm: Option<bool>,
}

/// Arguments of `create_task`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CreateTaskArgs {
    /// The task's name.
    pub name: String,
    /// The project it belongs to, by name, code or id.
    pub project: String,
    /// When the task starts. Required by Gryzzly. Defaults to today.
    pub start_at: Option<String>,
    /// When the task is supposed to end.
    pub end_at: Option<String>,
    /// The parent task, by name or id, when this is a subtask.
    pub parent: Option<String>,
    /// Whether this task only holds other tasks. A container takes no time.
    pub is_container: Option<bool>,
    /// How the budget is expressed.
    pub budget_type: Option<BudgetType>,
    /// The budget amount, when the type calls for one.
    pub budget_amount: Option<f64>,
    /// The hourly rate, when the type calls for one.
    pub hourly_rate: Option<f64>,
    /// Where the hourly rate comes from.
    pub hourly_rate_mode: Option<HourlyRateMode>,
    /// Planned time, as a duration (`40h`, `3600s`, `90m`).
    pub planned_duration: Option<String>,
    /// Estimated time, as a duration.
    pub estimated_duration: Option<String>,
    /// Group identifiers to assign.
    pub groups: Option<Vec<String>>,
    /// Tag names to attach.
    pub tags: Option<Vec<String>>,
}

/// Arguments of `update_task`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateTaskArgs {
    /// The task to change, by name or id.
    pub task: String,
    /// The project it belongs to, to disambiguate a name.
    pub project: Option<String>,
    /// A new name.
    pub name: Option<String>,
    /// A new start date.
    pub start_at: Option<String>,
    /// A new end date.
    pub end_at: Option<String>,
    /// When the task was completed. Marks it done.
    pub completed_at: Option<String>,
    /// A new budget amount.
    pub budget_amount: Option<f64>,
    /// A new hourly rate.
    pub hourly_rate: Option<f64>,
    /// Where the hourly rate comes from.
    pub hourly_rate_mode: Option<HourlyRateMode>,
    /// Planned time, as a duration (`40h`, `90m`).
    pub planned_duration: Option<String>,
    /// Group identifiers. Replaces the current list.
    pub groups: Option<Vec<String>>,
    /// Tag names. Replaces the current list.
    pub tags: Option<Vec<String>>,
}

/// Arguments of `delete_task`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteTaskArgs {
    /// The task to delete, by name or id.
    pub task: String,
    /// The project it belongs to, to disambiguate a name.
    pub project: Option<String>,
    /// Must be `true`. Without it nothing is deleted.
    pub confirm: Option<bool>,
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = catalog_router, vis = "pub")]
impl GryzzlyServer {
    /// List projects, filtered by status, customer or a fragment of their name
    /// or code. This is the starting point for logging time: a project's name
    /// here is what `log_time` accepts.
    #[rmcp::tool(annotations(
        title = "List projects",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_projects(
        &self,
        Parameters(args): Parameters<ListProjectsArgs>,
    ) -> std::result::Result<Json<Listing<Project>>, McpError> {
        let mut body = Body::new().paged(&args.page);
        if let Some(status) = &args.status {
            body = body.set(
                "status",
                status.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            );
        }
        let page: Page<Project> = self.client().list("projects.list", &body.build()).await?;
        let upstream = page.data.len();

        let customer_id = match args.customer.as_deref() {
            Some(needle) => Some(self.directory().customer(needle).await?.id),
            None => None,
        };
        let query = args.query.as_deref().map(str::to_lowercase);
        let items: Vec<Project> = page
            .data
            .into_iter()
            .filter(|p| {
                customer_id
                    .as_ref()
                    .is_none_or(|wanted| p.customer_id.as_ref() == Some(wanted))
            })
            .filter(|p| {
                query.as_ref().is_none_or(|needle| {
                    p.label().to_lowercase().contains(needle)
                        || p.code
                            .as_deref()
                            .is_some_and(|c| c.to_lowercase().contains(needle))
                })
            })
            .collect();
        ok(Listing::filtered(page.count, upstream, items))
    }

    /// Read one project in full: dates, visibility, contributors, managers,
    /// tags, customer.
    #[rmcp::tool(annotations(
        title = "Get a project",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_project(
        &self,
        Parameters(args): Parameters<ProjectRef>,
    ) -> std::result::Result<Json<Project>, McpError> {
        ok(self.directory().project(&args.project).await?)
    }

    /// List the tasks of a project, which is what time is actually declared
    /// against. Container tasks and completed tasks are filtered out by default,
    /// since neither is a place to log time.
    #[rmcp::tool(annotations(title = "List tasks", read_only_hint = true, open_world_hint = true))]
    pub async fn list_tasks(
        &self,
        Parameters(args): Parameters<ListTasksArgs>,
    ) -> std::result::Result<Json<Listing<Task>>, McpError> {
        let project_id = match args.project.as_deref() {
            Some(needle) => Some(self.directory().project(needle).await?.id),
            None => None,
        };
        let mut body = Body::new().paged(&args.page);
        if let Some(project_id) = &project_id {
            body = body.set("project_ids", vec![project_id.clone()]);
        }
        let page: Page<Task> = self.client().list("tasks.list", &body.build()).await?;
        let upstream = page.data.len();

        let query = args.query.as_deref().map(str::to_lowercase);
        let include_containers = args.include_containers.unwrap_or(false);
        let include_completed = args.include_completed.unwrap_or(true);
        let items: Vec<Task> = page
            .data
            .into_iter()
            .filter(|t| include_containers || t.is_container != Some(true))
            .filter(|t| include_completed || t.completed_at.is_none())
            .filter(|t| {
                query
                    .as_ref()
                    .is_none_or(|needle| t.label().to_lowercase().contains(needle))
            })
            .collect();
        ok(Listing::filtered(page.count, upstream, items))
    }

    /// Read one task in full: its budget, its rate, its planned time, its place
    /// in the project's hierarchy.
    #[rmcp::tool(annotations(title = "Get a task", read_only_hint = true, open_world_hint = true))]
    pub async fn get_task(
        &self,
        Parameters(args): Parameters<TaskRef>,
    ) -> std::result::Result<Json<Task>, McpError> {
        let project_id = match args.project.as_deref() {
            Some(needle) => Some(self.directory().project(needle).await?.id),
            None => None,
        };
        ok(self
            .directory()
            .task(&args.task, project_id.as_deref())
            .await?)
    }

    /// Budget and time counters of one project: budget, spent, planned time,
    /// elapsed time. Durations are seconds.
    #[rmcp::tool(annotations(
        title = "Project metrics",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_project_metrics(
        &self,
        Parameters(args): Parameters<ProjectRef>,
    ) -> std::result::Result<Json<ProjectMetrics>, McpError> {
        let project = self.directory().project(&args.project).await?;
        let metrics: ProjectMetrics = self
            .client()
            .call(
                "projects.getMetrics",
                &Body::new().set("project_id", project.id).build(),
            )
            .await?;
        ok(metrics)
    }

    /// The same counters for several projects in one call.
    #[rmcp::tool(annotations(
        title = "Projects metrics",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_projects_metrics(
        &self,
        Parameters(args): Parameters<ProjectsMetricsArgs>,
    ) -> std::result::Result<Json<Listing<ProjectMetrics>>, McpError> {
        if args.projects.is_empty() {
            return Err(Error::invalid("projects is empty").into());
        }
        let mut ids = Vec::with_capacity(args.projects.len());
        for needle in &args.projects {
            ids.push(self.directory().project(needle).await?.id);
        }
        let metrics: Vec<ProjectMetrics> = self
            .client()
            .call(
                "projects.listMetrics",
                &Body::new().set("project_ids", ids).build(),
            )
            .await?;
        ok(Listing::all(metrics))
    }
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = projects_router, vis = "pub")]
impl GryzzlyServer {
    /// Create a project. Gryzzly needs a start date, so today is used when none
    /// is given. A private project takes contributors; a public one must not.
    #[rmcp::tool(annotations(
        title = "Create a project",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_project(
        &self,
        Parameters(args): Parameters<CreateProjectArgs>,
    ) -> std::result::Result<Json<Project>, McpError> {
        let customer_id = match args.customer.as_deref() {
            Some(needle) => Some(self.directory().customer(needle).await?.id),
            None => None,
        };
        let body = Body::new()
            .set("name", args.name)
            .set(
                "start_at",
                match args.start_at.as_deref() {
                    Some(raw) => date(raw)?,
                    None => crate::dates::api_date(crate::dates::today()),
                },
            )
            .set_opt("end_at", opt_date(args.end_at.as_deref())?)
            .set_opt("code", args.code)
            .set_opt("customer_id", customer_id)
            .set_opt("is_billable", args.is_billable)
            .set_opt("visibility", args.visibility.map(Visibility::as_str))
            .set_list("contributors", self.user_ids(args.contributors).await?)
            .set_list("managers", self.user_ids(args.managers).await?)
            .set_list("groups", args.groups)
            .set_list("tags", args.tags)
            .build();
        let project: Project = self.client().call("projects.create", &body).await?;
        self.directory().invalidate_projects().await;
        ok(project)
    }

    /// Change a project. Gryzzly requires a start date on every update, so the
    /// current one is sent when none is given.
    #[rmcp::tool(annotations(
        title = "Update a project",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_project(
        &self,
        Parameters(args): Parameters<UpdateProjectArgs>,
    ) -> std::result::Result<Json<Project>, McpError> {
        let current = self.directory().project(&args.project).await?;
        let customer_id = match args.customer.as_deref() {
            Some(needle) => Some(self.directory().customer(needle).await?.id),
            None => None,
        };
        let start_at = match args.start_at.as_deref() {
            Some(raw) => date(raw)?,
            None => current
                .start_at
                .clone()
                .unwrap_or_else(|| crate::dates::api_date(crate::dates::today())),
        };
        let body = Body::new()
            .set("id", current.id.clone())
            .set("start_at", start_at)
            .set_opt("name", args.name)
            .set_opt("code", args.code)
            .set_opt("status", args.status.map(ProjectStatus::as_str))
            .set_opt("end_at", opt_date(args.end_at.as_deref())?)
            .set_opt("customer_id", customer_id)
            .set_opt("is_billable", args.is_billable)
            .set_opt("visibility", args.visibility.map(Visibility::as_str))
            .set_list("contributors", self.user_ids(args.contributors).await?)
            .set_list("managers", self.user_ids(args.managers).await?)
            .set_list("groups", args.groups)
            .set_list("tags", args.tags)
            .build();
        let project: Project = self.client().call("projects.update", &body).await?;
        self.directory().invalidate_projects().await;
        ok(project)
    }

    /// Delete a project. Gryzzly refuses while declarations are attached to it,
    /// which is the safety net; `confirm: true` is this server's.
    #[rmcp::tool(annotations(
        title = "Delete a project",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_project(
        &self,
        Parameters(args): Parameters<DeleteProjectArgs>,
    ) -> std::result::Result<Json<Deleted>, McpError> {
        let project = self.directory().project(&args.project).await?;
        require_confirmation(
            args.confirm.unwrap_or(false),
            &format!("project {:?}", project.label()),
        )?;
        let _: serde_json::Value = self
            .client()
            .call(
                "projects.delete",
                &Body::new().set("id", project.id.clone()).build(),
            )
            .await?;
        self.directory().invalidate_projects().await;
        self.directory().invalidate_tasks().await;
        ok(Deleted::new("project", project.id))
    }

    /// Create a task in a project. A task with `is_container: true` holds other
    /// tasks and takes no time; a subtask names its parent.
    #[rmcp::tool(annotations(
        title = "Create a task",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_task(
        &self,
        Parameters(args): Parameters<CreateTaskArgs>,
    ) -> std::result::Result<Json<Task>, McpError> {
        let project = self.directory().project(&args.project).await?;
        let parent_id = match args.parent.as_deref() {
            Some(needle) => Some(self.directory().task(needle, Some(&project.id)).await?.id),
            None => None,
        };
        let body = Body::new()
            .set("name", args.name)
            .set("project_id", project.id)
            .set(
                "start_at",
                match args.start_at.as_deref() {
                    Some(raw) => date(raw)?,
                    None => crate::dates::api_date(crate::dates::today()),
                },
            )
            .set_opt("end_at", opt_date(args.end_at.as_deref())?)
            .set_opt("parent_id", parent_id)
            .set_opt("is_container", args.is_container)
            .set_opt("budget_type", args.budget_type.map(BudgetType::as_str))
            .set_opt("budget_amount", args.budget_amount)
            .set_opt("hourly_rate", args.hourly_rate)
            .set_opt(
                "hourly_rate_mode",
                args.hourly_rate_mode.map(HourlyRateMode::as_str),
            )
            .set_opt(
                "planned_duration",
                seconds(args.planned_duration.as_deref())?,
            )
            .set_opt(
                "estimated_duration",
                seconds(args.estimated_duration.as_deref())?,
            )
            .set_list("groups", args.groups)
            .set_list("tags", args.tags)
            .build();
        let task: Task = self.client().call("tasks.create", &body).await?;
        self.directory().invalidate_tasks().await;
        ok(task)
    }

    /// Change a task: rename it, move its dates, set its budget or its rate,
    /// mark it completed.
    #[rmcp::tool(annotations(
        title = "Update a task",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_task(
        &self,
        Parameters(args): Parameters<UpdateTaskArgs>,
    ) -> std::result::Result<Json<Task>, McpError> {
        let project_id = match args.project.as_deref() {
            Some(needle) => Some(self.directory().project(needle).await?.id),
            None => None,
        };
        let task = self
            .directory()
            .task(&args.task, project_id.as_deref())
            .await?;
        let body = Body::new()
            .set("id", task.id.clone())
            .set_opt("name", args.name)
            .set_opt("start_at", opt_date(args.start_at.as_deref())?)
            .set_opt("end_at", opt_date(args.end_at.as_deref())?)
            .set_opt("completed_at", opt_date(args.completed_at.as_deref())?)
            .set_opt("budget_amount", args.budget_amount)
            .set_opt("hourly_rate", args.hourly_rate)
            .set_opt(
                "hourly_rate_mode",
                args.hourly_rate_mode.map(HourlyRateMode::as_str),
            )
            .set_opt(
                "planned_duration",
                seconds(args.planned_duration.as_deref())?,
            )
            .set_list("groups", args.groups)
            .set_list("tags", args.tags)
            .build();
        let updated: Task = self.client().call("tasks.update", &body).await?;
        self.directory().invalidate_tasks().await;
        ok(updated)
    }

    /// Delete a task. Gryzzly refuses while declarations are attached to it.
    #[rmcp::tool(annotations(
        title = "Delete a task",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_task(
        &self,
        Parameters(args): Parameters<DeleteTaskArgs>,
    ) -> std::result::Result<Json<Deleted>, McpError> {
        let project_id = match args.project.as_deref() {
            Some(needle) => Some(self.directory().project(needle).await?.id),
            None => None,
        };
        let task = self
            .directory()
            .task(&args.task, project_id.as_deref())
            .await?;
        require_confirmation(
            args.confirm.unwrap_or(false),
            &format!("task {:?}", task.label()),
        )?;
        let _: serde_json::Value = self
            .client()
            .call(
                "tasks.delete",
                &Body::new().set("id", task.id.clone()).build(),
            )
            .await?;
        self.directory().invalidate_tasks().await;
        ok(Deleted::new("task", task.id))
    }
}

impl GryzzlyServer {
    /// Resolve a list of people to identifiers, accepting names and emails.
    pub(crate) async fn user_ids(
        &self,
        needles: Option<Vec<String>>,
    ) -> std::result::Result<Option<Vec<String>>, Error> {
        let Some(needles) = needles else {
            return Ok(None);
        };
        let mut ids = Vec::with_capacity(needles.len());
        for needle in &needles {
            ids.push(self.directory().user(Some(needle)).await?.id);
        }
        Ok(Some(ids))
    }
}

/// Parse an optional duration argument into seconds.
fn seconds(raw: Option<&str>) -> std::result::Result<Option<i64>, Error> {
    match raw.map(str::trim).filter(|d| !d.is_empty()) {
        Some(raw) => Ok(Some(i64::from(crate::duration::parse(raw)?))),
        None => Ok(None),
    }
}
