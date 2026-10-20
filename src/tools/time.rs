//! Time: declarations, timesheets, and the one question every client asks
//! first, "who am I?".
//!
//! This is the toolset the server exists for. Everything else in Gryzzly is
//! administration around it.

use chrono::{Datelike, Duration as ChronoDuration, NaiveDate};
use rmcp::{ErrorData as McpError, Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    dates, duration,
    error::{Error, Result},
    model::{Declaration, Project, Task, User},
    server::GryzzlyServer,
    tools::{Deleted, PageArgs, ok, require_confirmation},
};

/// A person, trimmed to what a caller needs to recognise them.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct UserRef {
    /// The user's identifier.
    pub id: String,
    /// The user's display name.
    pub name: String,
    /// The user's email address, when the API exposes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl From<&User> for UserRef {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.clone(),
            name: user.label().to_owned(),
            email: user.email.clone(),
        }
    }
}

/// One declaration, with the names its identifiers stand for.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct EntryView {
    /// The declaration's identifier, needed to update or delete it.
    pub id: String,
    /// The day, as `YYYY-MM-DD`.
    pub date: String,
    /// The duration, human readable.
    pub duration: String,
    /// The duration in seconds, which is what Gryzzly stores.
    pub seconds: i64,
    /// What the time was spent on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The task the time is declared against.
    pub task_id: String,
    /// The task's name, when it could be resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The project the task belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The project's name, when it could be resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Whose time it is.
    pub user_id: String,
}

/// A day of a timesheet.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct DayLine {
    /// The day, as `YYYY-MM-DD`.
    pub date: String,
    /// The day of the week, spelled out.
    pub weekday: String,
    /// What was declared that day, in seconds.
    pub seconds: i64,
    /// The same, human readable.
    pub duration: String,
    /// How many declarations make it up.
    pub entries: usize,
}

/// A project's share of a timesheet.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ProjectLine {
    /// The project's identifier, when it could be resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The project's name.
    pub project: String,
    /// Declared time, in seconds.
    pub seconds: i64,
    /// The same, human readable.
    pub duration: String,
    /// The share of the period, in percent, rounded to one decimal.
    pub share_percent: f64,
}

/// Which declarations a report is built from.
///
/// The filter is applied before the totals, not after: a timesheet narrowed to
/// one project has to total that project, not the week.
#[derive(Debug, Clone, Default)]
pub struct EntryFilter {
    /// Keep only declarations whose task belongs to this project.
    pub project_id: Option<String>,
    /// Keep only declarations on this task.
    pub task_id: Option<String>,
}

impl EntryFilter {
    /// True when nothing is filtered out.
    pub fn is_empty(&self) -> bool {
        self.project_id.is_none() && self.task_id.is_none()
    }

    fn keeps(&self, entry: &EntryView) -> bool {
        self.project_id
            .as_ref()
            .is_none_or(|wanted| entry.project_id.as_ref() == Some(wanted))
            && self
                .task_id
                .as_ref()
                .is_none_or(|wanted| &entry.task_id == wanted)
    }
}

/// What `timesheet` and `list_time_entries` report.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Report {
    /// Whose time this is.
    pub user: UserRef,
    /// First day of the period.
    pub from: String,
    /// Last day of the period.
    pub to: String,
    /// The ISO week, when the period is exactly one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week: Option<String>,
    /// Declared time over the period, in seconds.
    pub total_seconds: i64,
    /// The same, human readable.
    pub total: String,
    /// The same, in decimal hours.
    pub total_hours: f64,
    /// One line per day that carries time.
    pub days: Vec<DayLine>,
    /// One line per project, largest first.
    pub projects: Vec<ProjectLine>,
    /// Working days of the period with nothing declared.
    pub missing_workdays: Vec<String>,
    /// The declarations themselves, when the caller asked for them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<EntryView>>,
}

/// What `whoami` answers.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct WhoAmI {
    /// The user this server declares time for.
    pub user: UserRef,
    /// The user's role in Gryzzly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The date until which this user's times are approved and locked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timesheet_approved_until: Option<String>,
    /// Which environment variable named this user, if any did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identified_by: Option<String>,
    /// How many users the API key can see.
    pub team_users: usize,
    /// The project used when a tool omits one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_project: Option<String>,
    /// The toolsets this server exposes.
    pub toolsets: Vec<String>,
    /// True when every write tool was withheld.
    pub read_only: bool,
    /// This week, so a client knows where it stands before logging anything.
    pub this_week: Report,
}

/// What `log_time` answers.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct LoggedTime {
    /// The declaration Gryzzly created.
    pub entry: EntryView,
    /// What the whole day now totals, so a double entry is visible at once.
    pub day_total: String,
    /// The same, in seconds.
    pub day_total_seconds: i64,
}

/// One line of a bulk log.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct BulkOutcome {
    /// The position of the entry in the request, zero-based.
    pub index: usize,
    /// Whether Gryzzly accepted it.
    pub created: bool,
    /// The declaration, when it was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryView>,
    /// Why it was refused, when it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `log_time_bulk` answers.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct BulkReport {
    /// How many declarations were created.
    pub created: usize,
    /// How many were refused.
    pub failed: usize,
    /// Total time created, human readable.
    pub total: String,
    /// One line per requested entry, in order.
    pub outcomes: Vec<BulkOutcome>,
}

/// Arguments of `log_time`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct LogTimeArgs {
    /// How long: `2h30`, `90m`, `1.5h`, `2:30`, or a bare number of minutes.
    pub duration: String,
    /// The task, by name or id. Optional when the project has one task only.
    pub task: Option<String>,
    /// The project, by name, code or id. Narrows the task lookup.
    pub project: Option<String>,
    /// The day: `today` (the default), `yesterday`, a weekday, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// What the time was spent on. Worth filling: it is what a human reads later.
    pub description: Option<String>,
    /// Whose time it is. Defaults to the user this server acts as.
    pub user: Option<String>,
}

/// One entry of `log_time_bulk`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct BulkEntry {
    /// How long: `2h30`, `90m`, `1.5h`, `2:30`, or a bare number of minutes.
    pub duration: String,
    /// The task, by name or id.
    pub task: Option<String>,
    /// The project, by name, code or id.
    pub project: Option<String>,
    /// The day. Defaults to today.
    pub date: Option<String>,
    /// What the time was spent on.
    pub description: Option<String>,
    /// Whose time it is. Defaults to the user this server acts as.
    pub user: Option<String>,
}

/// Arguments of `log_time_bulk`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct LogTimeBulkArgs {
    /// The declarations to create, in order.
    pub entries: Vec<BulkEntry>,
    /// Stop at the first refusal instead of carrying on. Defaults to false.
    pub stop_on_error: Option<bool>,
}

/// Arguments of `list_time_entries` and `timesheet`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct PeriodArgs {
    /// A week: `this week`, `last week`, `2026-W38`, or a date inside it.
    pub week: Option<String>,
    /// First day of an explicit period. Ignored when `week` is set.
    pub from: Option<String>,
    /// Last day of an explicit period. Defaults to `from`.
    pub to: Option<String>,
    /// Whose time to read. Defaults to the user this server acts as.
    pub user: Option<String>,
    /// Keep only this project, by name, code or id.
    pub project: Option<String>,
    /// Keep only this task, by name or id.
    pub task: Option<String>,
}

/// Arguments of `update_time_entry`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateTimeArgs {
    /// The declaration's identifier, as returned by `list_time_entries`.
    pub id: String,
    /// The new duration. Left alone when absent.
    pub duration: Option<String>,
    /// The new day. Left alone when absent.
    pub date: Option<String>,
    /// The new task, by name or id. Left alone when absent.
    pub task: Option<String>,
    /// The project the new task belongs to, to narrow the lookup.
    pub project: Option<String>,
    /// The new description. Left alone when absent.
    pub description: Option<String>,
    /// Whose declaration it is, when it is not the current user's.
    pub user: Option<String>,
}

/// Arguments of `delete_time_entry`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteTimeArgs {
    /// The declaration's identifier.
    pub id: String,
    /// Must be `true`. Without it nothing is deleted.
    pub confirm: Option<bool>,
}

/// Arguments of `timesheet`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct TimesheetArgs {
    /// The period to report on.
    #[serde(flatten)]
    pub period: PeriodArgs,
    /// Hours expected on a working day, used to list the days that fall short.
    /// Defaults to 7.
    pub expected_hours_per_day: Option<f64>,
}

/// Arguments of `list_time_entries`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListTimeArgs {
    /// The period to read.
    #[serde(flatten)]
    pub period: PeriodArgs,
    /// Pagination, for a period long enough to need it.
    #[serde(flatten)]
    pub page: PageArgs,
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = time_router, vis = "pub")]
impl GryzzlyServer {
    /// Who this server declares time for, what it is allowed to do, and how much
    /// time is already logged this week. Call it first: Gryzzly API keys belong
    /// to a team, not to a person, so the person has to be named by
    /// configuration, and this is where you see which one was.
    #[rmcp::tool(annotations(title = "Who am I", read_only_hint = true, open_world_hint = true))]
    pub async fn whoami(&self) -> std::result::Result<Json<WhoAmI>, McpError> {
        let directory = self.directory();
        let user = directory.current_user().await?;
        let team = directory.users().await?;
        let (monday, sunday) = dates::parse_week("this week")?;
        let week = report(
            self,
            &user,
            monday,
            sunday,
            7.0,
            false,
            &EntryFilter::default(),
        )
        .await?;
        let config = self.config();
        ok(WhoAmI {
            user: UserRef::from(&*user),
            role: user.role.clone(),
            timesheet_approved_until: user.timesheet_approved_until.clone(),
            identified_by: config.identity_hint(),
            team_users: team.len(),
            default_project: config.default_project.clone(),
            toolsets: config.toolsets.iter().map(ToString::to_string).collect(),
            read_only: config.read_only,
            this_week: week,
        })
    }

    /// Declare time. Gryzzly attaches time to a task, so name a task, or a
    /// project whose tasks can be narrowed down to one. Durations are human:
    /// `2h30`, `90m`, `1.5h`. The answer repeats the day's new total, which is
    /// how you notice you have logged the same afternoon twice.
    #[rmcp::tool(annotations(
        title = "Log time",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn log_time(
        &self,
        Parameters(args): Parameters<LogTimeArgs>,
    ) -> std::result::Result<Json<LoggedTime>, McpError> {
        let created = self.create_declaration(&args).await?;
        let entry = self.view_of(&created).await;
        let date = entry.date.clone();
        let user_id = entry.user_id.clone();
        let day_total = self
            .day_total(&user_id, &date)
            .await
            .unwrap_or(entry.seconds);
        ok(LoggedTime {
            entry,
            day_total: duration::format(u32::try_from(day_total.max(0)).unwrap_or(u32::MAX)),
            day_total_seconds: day_total,
        })
    }

    /// Declare several time entries in one call, which is what filling a week
    /// looks like: each entry takes the same arguments as log_time. Entries are
    /// created in order; by default a refusal does not stop the others, and
    /// every outcome is reported with its position. Read the week first with
    /// timesheet: Gryzzly accepts duplicates, so calling this twice logs twice.
    #[rmcp::tool(annotations(
        title = "Log time in bulk",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn log_time_bulk(
        &self,
        Parameters(args): Parameters<LogTimeBulkArgs>,
    ) -> std::result::Result<Json<BulkReport>, McpError> {
        if args.entries.is_empty() {
            return Err(Error::invalid("entries is empty").into());
        }
        if args.entries.len() > 200 {
            return Err(Error::invalid(
                "entries holds more than 200 declarations, which is more than a quarter of a year of work: split the call",
            )
            .into());
        }
        let stop = args.stop_on_error.unwrap_or(false);
        let mut outcomes = Vec::with_capacity(args.entries.len());
        let mut created = 0_usize;
        let mut failed = 0_usize;
        let mut total = 0_i64;

        for (index, entry) in args.entries.into_iter().enumerate() {
            let request = LogTimeArgs {
                duration: entry.duration,
                task: entry.task,
                project: entry.project,
                date: entry.date,
                description: entry.description,
                user: entry.user,
            };
            match self.create_declaration(&request).await {
                Ok(declaration) => {
                    let view = self.view_of(&declaration).await;
                    total += view.seconds;
                    created += 1;
                    outcomes.push(BulkOutcome {
                        index,
                        created: true,
                        entry: Some(view),
                        error: None,
                    });
                }
                Err(err) => {
                    failed += 1;
                    outcomes.push(BulkOutcome {
                        index,
                        created: false,
                        entry: None,
                        error: Some(err.to_string()),
                    });
                    if stop {
                        break;
                    }
                }
            }
        }

        ok(BulkReport {
            created,
            failed,
            total: duration::format(u32::try_from(total.max(0)).unwrap_or(u32::MAX)),
            outcomes,
        })
    }

    /// Read declared time over a week or a period, with task and project names
    /// filled in, per-day and per-project totals, and the identifiers needed to
    /// change an entry. Read before you write.
    #[rmcp::tool(annotations(
        title = "List time entries",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_time_entries(
        &self,
        Parameters(args): Parameters<ListTimeArgs>,
    ) -> std::result::Result<Json<Report>, McpError> {
        let (user, from, to) = self.resolve_period(&args.period).await?;
        let filter = self.filter_of(&args.period).await?;
        let mut report = report(self, &user, from, to, 7.0, true, &filter).await?;
        if let Some(entries) = report.entries.as_mut() {
            let offset = args.page.offset.unwrap_or(0) as usize;
            let limit = args.page.limit.unwrap_or(crate::tools::MAX_LIMIT) as usize;
            if offset > 0 || entries.len() > limit {
                *entries = entries.iter().skip(offset).take(limit).cloned().collect();
            }
        }
        ok(report)
    }

    /// Summarise declared time over a week or a period for one user, the
    /// current one by default: totals per day and per project, each project's
    /// share, and the working days below the expected hours. Use it to answer
    /// how much was logged or what is missing; use list_time_entries when you
    /// need the individual entries and their ids. Read-only. Optional project
    /// and task filters narrow the totals, not just a listing.
    #[rmcp::tool(annotations(title = "Timesheet", read_only_hint = true, open_world_hint = true))]
    pub async fn timesheet(
        &self,
        Parameters(args): Parameters<TimesheetArgs>,
    ) -> std::result::Result<Json<Report>, McpError> {
        let (user, from, to) = self.resolve_period(&args.period).await?;
        let expected = args.expected_hours_per_day.unwrap_or(7.0);
        if !(0.0..=24.0).contains(&expected) {
            return Err(Error::invalid("expected_hours_per_day must be between 0 and 24").into());
        }
        let filter = self.filter_of(&args.period).await?;
        let report = report(self, &user, from, to, expected, false, &filter).await?;
        ok(report)
    }

    /// Change one time entry (a declaration), by the id list_time_entries
    /// returns: its duration, its day, its task, its description. Gryzzly's
    /// update requires all of them, so whatever is left out is read back from
    /// the existing entry first. Use log_time to add time and delete_time_entry
    /// to remove an entry.
    #[rmcp::tool(annotations(
        title = "Update a time entry",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_time_entry(
        &self,
        Parameters(args): Parameters<UpdateTimeArgs>,
    ) -> std::result::Result<Json<EntryView>, McpError> {
        let directory = self.directory();
        let user = directory.user(args.user.as_deref()).await?;
        let current = self.find_declaration(&args.id, &user).await?;

        let date = match args.date.as_deref() {
            Some(raw) => dates::api_date(dates::parse_date(raw)?),
            None => current
                .date
                .clone()
                .ok_or_else(|| Error::invalid("the existing declaration has no date to keep"))?,
        };
        let seconds = match args.duration.as_deref() {
            Some(raw) => i64::from(duration::parse(raw)?),
            None => current.duration.ok_or_else(|| {
                Error::invalid("the existing declaration has no duration to keep")
            })?,
        };
        let task_id = match args.task.as_deref() {
            Some(raw) => {
                let project_id = match args.project.as_deref() {
                    Some(project) => Some(directory.project(project).await?.id),
                    None => None,
                };
                directory.task(raw, project_id.as_deref()).await?.id
            }
            None => current
                .task_id
                .clone()
                .ok_or_else(|| Error::invalid("the existing declaration has no task to keep"))?,
        };
        let description = args
            .description
            .clone()
            .or_else(|| current.description.clone());

        let mut body = json!({
            "id": args.id,
            "date": date,
            "task_id": task_id,
            "duration": seconds,
        });
        if let (Some(description), Some(object)) = (description, body.as_object_mut()) {
            object.insert("description".into(), description.into());
        }
        let updated: Declaration = self.client().call("declarations.update", &body).await?;
        ok(self.view_of(&updated).await)
    }

    /// Delete one time declaration by its id, as returned by list_time_entries.
    /// Irreversible: Gryzzly has no undo, so the call refuses to run without
    /// confirm true, and the person should see the entry and agree first. Use
    /// update_time_entry instead to correct a duration, day, task or
    /// description.
    #[rmcp::tool(annotations(
        title = "Delete a time entry",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_time_entry(
        &self,
        Parameters(args): Parameters<DeleteTimeArgs>,
    ) -> std::result::Result<Json<Deleted>, McpError> {
        require_confirmation(args.confirm.unwrap_or(false), "this declaration")?;
        let _: serde_json::Value = self
            .client()
            .call("declarations.delete", &json!({ "id": args.id }))
            .await?;
        ok(Deleted::new("declaration", args.id))
    }
}

impl GryzzlyServer {
    /// Create one declaration from the ergonomic arguments.
    async fn create_declaration(&self, args: &LogTimeArgs) -> Result<Declaration> {
        let directory = self.directory();
        let seconds = duration::parse(&args.duration)?;
        let date = dates::api_date(match args.date.as_deref() {
            Some(raw) => dates::parse_date(raw)?,
            None => dates::today(),
        });
        let user = directory.user(args.user.as_deref()).await?;
        let task = self
            .resolve_task_for_write(args.task.as_deref(), args.project.as_deref())
            .await?;

        let mut body = json!({
            "date": date,
            "duration": seconds,
            "task_id": task.id,
            "user_id": user.id,
        });
        if let (Some(description), Some(object)) = (
            args.description.as_deref().filter(|d| !d.trim().is_empty()),
            body.as_object_mut(),
        ) {
            object.insert("description".into(), description.into());
        }
        self.client().call("declarations.create", &body).await
    }

    /// Find the task time should go to, from a task name, a project, or the
    /// configured default project.
    async fn resolve_task_for_write(
        &self,
        task: Option<&str>,
        project: Option<&str>,
    ) -> Result<Task> {
        let directory = self.directory();
        let project = project.or(self.config().default_project.as_deref());
        let project_id = match project {
            Some(needle) => Some(directory.project(needle).await?.id),
            None => None,
        };
        if let Some(task) = task.map(str::trim).filter(|t| !t.is_empty()) {
            return directory.task(task, project_id.as_deref()).await;
        }
        let Some(project_id) = project_id else {
            return Err(Error::invalid(
                "name a task, or a project: Gryzzly attaches time to a task, never to a project directly. Set GRYZZLY_DEFAULT_PROJECT to skip this on every call.",
            ));
        };
        let tasks = directory.tasks_of(&project_id).await?;
        let declarable: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.is_container != Some(true) && t.completed_at.is_none())
            .collect();
        match declarable.as_slice() {
            [only] => Ok((*only).clone()),
            [] => Err(Error::not_found(format!(
                "project {project_id} has no task time can be declared against: create one first"
            ))),
            many => Err(Error::Ambiguous {
                message: format!(
                    "project {project_id} has {} tasks: name the one the time goes to",
                    many.len()
                ),
                candidates: many
                    .iter()
                    .take(12)
                    .map(|t| crate::error::Candidate {
                        id: t.id.clone(),
                        name: t.label().to_owned(),
                    })
                    .collect(),
            }),
        }
    }

    /// Locate a declaration by id.
    ///
    /// Gryzzly has no `declarations.get`, and `declarations.list` cannot filter
    /// by id, so the entry is looked for in the user's declarations over the
    /// configured lookback window.
    async fn find_declaration(&self, id: &str, user: &User) -> Result<Declaration> {
        let today = dates::today();
        let from = today - ChronoDuration::days(self.config().lookback_days);
        let to = today + ChronoDuration::days(370);
        let declarations = self.declarations_of(&user.id, from, to).await?;
        declarations
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| {
                Error::not_found(format!(
                    "no declaration {id} for {} between {} and {}. Gryzzly has no way to fetch one by id, so it is looked for over that window: widen it with GRYZZLY_LOOKBACK_DAYS, or pass the right user.",
                    user.label(),
                    dates::api_date(from),
                    dates::api_date(to)
                ))
            })
    }

    /// Every declaration of one user over a period.
    async fn declarations_of(
        &self,
        user_id: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<Declaration>> {
        let body = json!({
            "user_ids": [user_id],
            "task_ids": [],
            "from": dates::api_date(from),
            "to": dates::api_date(to),
        });
        self.client()
            .list_all("declarations.list", &body, 20000)
            .await
    }

    /// What one user declared on one day, in seconds.
    async fn day_total(&self, user_id: &str, date: &str) -> Result<i64> {
        let day = dates::parse_date(date)?;
        let declarations = self.declarations_of(user_id, day, day).await?;
        Ok(declarations.iter().filter_map(|d| d.duration).sum())
    }

    /// Turn a declaration into the view a caller reads, resolving names.
    async fn view_of(&self, declaration: &Declaration) -> EntryView {
        let directory = self.directory();
        let task_id = declaration.task_id.clone().unwrap_or_default();
        let task = directory
            .all_tasks()
            .await
            .ok()
            .and_then(|tasks| tasks.iter().find(|t| t.id == task_id).cloned());
        let project =
            match &task {
                Some(task) => match &task.project_id {
                    Some(project_id) => directory.projects().await.ok().and_then(|projects| {
                        projects.iter().find(|p| &p.id == project_id).cloned()
                    }),
                    None => None,
                },
                None => None,
            };
        let seconds = declaration.duration.unwrap_or(0);
        EntryView {
            id: declaration.id.clone(),
            date: declaration.date.clone().unwrap_or_default(),
            duration: duration::format(u32::try_from(seconds.max(0)).unwrap_or(u32::MAX)),
            seconds,
            description: declaration
                .description
                .clone()
                .filter(|d| !d.trim().is_empty()),
            task_id,
            task: task.as_ref().map(|t| t.label().to_owned()),
            project_id: project.as_ref().map(|p| p.id.clone()),
            project: project.as_ref().map(|p| p.label().to_owned()),
            user_id: declaration.user_id.clone().unwrap_or_default(),
        }
    }

    /// Resolve the user and the period a read tool was asked for.
    async fn resolve_period(&self, args: &PeriodArgs) -> Result<(User, NaiveDate, NaiveDate)> {
        let user = self.directory().user(args.user.as_deref()).await?;
        let (from, to) = period_of(args)?;
        Ok((user, from, to))
    }

    /// Resolve the project and task filters a read tool was given.
    async fn filter_of(&self, args: &PeriodArgs) -> Result<EntryFilter> {
        let directory = self.directory();
        let project_id = match args.project.as_deref() {
            Some(needle) => Some(directory.project(needle).await?.id),
            None => None,
        };
        let task_id = match args.task.as_deref() {
            Some(needle) => Some(directory.task(needle, project_id.as_deref()).await?.id),
            None => None,
        };
        Ok(EntryFilter {
            project_id,
            task_id,
        })
    }
}

/// The period a read tool was asked for: a week, an explicit range, or a day.
pub fn period_of(args: &PeriodArgs) -> Result<(NaiveDate, NaiveDate)> {
    if let Some(week) = args.week.as_deref().filter(|w| !w.trim().is_empty()) {
        return dates::parse_week(week);
    }
    match (args.from.as_deref(), args.to.as_deref()) {
        (Some(from), Some(to)) => {
            let from = dates::parse_date(from)?;
            let to = dates::parse_date(to)?;
            if to < from {
                return Err(Error::invalid(format!(
                    "the period ends before it starts: {} to {}",
                    dates::api_date(from),
                    dates::api_date(to)
                )));
            }
            Ok((from, to))
        }
        (Some(from), None) => {
            let from = dates::parse_date(from)?;
            Ok((from, from))
        }
        (None, Some(to)) => {
            let to = dates::parse_date(to)?;
            Ok((to, to))
        }
        (None, None) => dates::parse_week("this week"),
    }
}

/// Build a report over a period, shared by `whoami`, `timesheet`,
/// `list_time_entries` and the `gryzzly://timesheet/{week}` resource.
pub async fn report(
    server: &GryzzlyServer,
    user: &User,
    from: NaiveDate,
    to: NaiveDate,
    expected_hours_per_day: f64,
    include_entries: bool,
    filter: &EntryFilter,
) -> Result<Report> {
    let declarations = server.declarations_of(&user.id, from, to).await?;
    let tasks = server.directory().all_tasks().await.unwrap_or_default();
    let projects = server.directory().projects().await.unwrap_or_default();

    let mut views: Vec<EntryView> = declarations
        .iter()
        .map(|d| view(d, &tasks, &projects))
        .filter(|entry| filter.keeps(entry))
        .collect();
    views.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));

    let total_seconds: i64 = views.iter().map(|v| v.seconds).sum();

    let mut days: Vec<DayLine> = Vec::new();
    for date in dates::days_between(from, to) {
        let label = dates::api_date(date);
        let of_day: Vec<&EntryView> = views.iter().filter(|v| v.date == label).collect();
        if of_day.is_empty() {
            continue;
        }
        let seconds: i64 = of_day.iter().map(|v| v.seconds).sum();
        days.push(DayLine {
            date: label,
            weekday: weekday_name(date),
            seconds,
            duration: duration::format(u32::try_from(seconds.max(0)).unwrap_or(u32::MAX)),
            entries: of_day.len(),
        });
    }

    let mut by_project: Vec<ProjectLine> = Vec::new();
    for view in &views {
        let name = view
            .project
            .clone()
            .unwrap_or_else(|| "(unknown project)".to_owned());
        match by_project.iter_mut().find(|line| line.project == name) {
            Some(line) => line.seconds += view.seconds,
            None => by_project.push(ProjectLine {
                project_id: view.project_id.clone(),
                project: name,
                seconds: view.seconds,
                duration: String::new(),
                share_percent: 0.0,
            }),
        }
    }
    by_project.sort_by_key(|line| std::cmp::Reverse(line.seconds));
    for line in &mut by_project {
        line.duration = duration::format(u32::try_from(line.seconds.max(0)).unwrap_or(u32::MAX));
        #[expect(
            clippy::cast_precision_loss,
            reason = "seconds of work fit a f64 exactly"
        )]
        let share = if total_seconds > 0 {
            (line.seconds as f64) * 100.0 / (total_seconds as f64)
        } else {
            0.0
        };
        line.share_percent = (share * 10.0).round() / 10.0;
    }

    let expected_seconds = (expected_hours_per_day * 3600.0).round();
    let missing_workdays: Vec<String> = dates::days_between(from, to)
        .into_iter()
        .filter(|date| !dates::is_weekend(*date))
        .filter(|date| {
            let label = dates::api_date(*date);
            #[expect(
                clippy::cast_precision_loss,
                reason = "seconds of work fit a f64 exactly"
            )]
            let declared = days
                .iter()
                .find(|d| d.date == label)
                .map_or(0.0, |d| d.seconds as f64);
            declared < expected_seconds
        })
        .map(dates::api_date)
        .collect();

    let week = (to - from == ChronoDuration::days(6) && from.weekday() == chrono::Weekday::Mon)
        .then(|| dates::iso_week(from));

    Ok(Report {
        user: UserRef::from(user),
        from: dates::api_date(from),
        to: dates::api_date(to),
        week,
        total_seconds,
        total: duration::format(u32::try_from(total_seconds.max(0)).unwrap_or(u32::MAX)),
        total_hours: (duration::hours(u32::try_from(total_seconds.max(0)).unwrap_or(u32::MAX))
            * 100.0)
            .round()
            / 100.0,
        days,
        projects: by_project,
        missing_workdays,
        entries: include_entries.then_some(views),
    })
}

/// Resolve one declaration against already-loaded tasks and projects.
fn view(declaration: &Declaration, tasks: &[Task], projects: &[Project]) -> EntryView {
    let task_id = declaration.task_id.clone().unwrap_or_default();
    let task = tasks.iter().find(|t| t.id == task_id);
    let project = task
        .and_then(|t| t.project_id.as_ref())
        .and_then(|project_id| projects.iter().find(|p| &p.id == project_id));
    let seconds = declaration.duration.unwrap_or(0);
    EntryView {
        id: declaration.id.clone(),
        date: declaration.date.clone().unwrap_or_default(),
        duration: duration::format(u32::try_from(seconds.max(0)).unwrap_or(u32::MAX)),
        seconds,
        description: declaration
            .description
            .clone()
            .filter(|d| !d.trim().is_empty()),
        task_id,
        task: task.map(|t| t.label().to_owned()),
        project_id: project.map(|p| p.id.clone()),
        project: project.map(|p| p.label().to_owned()),
        user_id: declaration.user_id.clone().unwrap_or_default(),
    }
}

fn weekday_name(date: NaiveDate) -> String {
    match date.weekday() {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    }
    .to_owned()
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
    fn a_period_defaults_to_this_week() {
        let (from, to) = period_of(&PeriodArgs::default()).unwrap();
        assert_eq!(to - from, ChronoDuration::days(6));
        assert_eq!(from.weekday(), chrono::Weekday::Mon);
    }

    #[test]
    fn one_date_is_one_day() {
        let args = PeriodArgs {
            from: Some("2026-09-16".into()),
            ..PeriodArgs::default()
        };
        let (from, to) = period_of(&args).unwrap();
        assert_eq!(from, to);
    }

    #[test]
    fn a_week_wins_over_an_explicit_range() {
        let args = PeriodArgs {
            week: Some("2026-W38".into()),
            from: Some("2020-01-01".into()),
            to: Some("2020-01-02".into()),
            ..PeriodArgs::default()
        };
        let (from, to) = period_of(&args).unwrap();
        assert_eq!(dates::api_date(from), "2026-09-14");
        assert_eq!(dates::api_date(to), "2026-09-20");
    }

    #[test]
    fn an_inverted_range_is_refused() {
        let args = PeriodArgs {
            from: Some("2026-09-20".into()),
            to: Some("2026-09-14".into()),
            ..PeriodArgs::default()
        };
        let err = period_of(&args).unwrap_err().to_string();
        assert!(err.contains("ends before it starts"), "{err}");
    }
}
