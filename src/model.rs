//! The Gryzzly objects, as the API returns them.
//!
//! Responses are read leniently: every field is optional and unknown fields are
//! kept out of the way, because an API that grows a field should not break a
//! client that does not know it yet. Request shapes live with their tools,
//! where they can be strict.

use serde::{Deserialize, Serialize};

/// The envelope every `*.list` action answers with.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct Page<T> {
    /// The zero-based offset of the first item returned.
    #[serde(default)]
    pub offset: u32,
    /// The limit that was applied. `0` when the caller sent none.
    #[serde(default)]
    pub limit: u32,
    /// How many items match the filters, beyond this page.
    #[serde(default)]
    pub count: u32,
    /// The page itself.
    #[serde(default)]
    pub data: Vec<T>,
}

impl<T> Page<T> {
    /// An empty page, which is what a list endpoint answering 404 means.
    pub fn empty() -> Self {
        Self {
            offset: 0,
            limit: 0,
            count: 0,
            data: Vec::new(),
        }
    }
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self::empty()
    }
}

/// `{"ok": true}`, which some delete actions answer with.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OkResponse {
    /// Whether the action succeeded.
    #[serde(default)]
    pub ok: bool,
}

/// A member of the Gryzzly team the API key belongs to.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct User {
    /// The user's identifier.
    pub id: String,
    /// The user's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The user's email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// The user's code, as set by the administrators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// One of `contributor`, `collaborator`, `manager`, `director`, `admin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// When the user was disabled, if they were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_at: Option<String>,
    /// Deprecated upstream in favour of `disabled_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_disabled: Option<bool>,
    /// The date until which this user's times are approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timesheet_approved_until: Option<String>,
    /// The user's hourly cost. Only on the budget offer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hourly_cost: Option<f64>,
    /// The user's hourly rate. Only on the budget offer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hourly_rate: Option<f64>,
    /// The groups the user belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    /// When the user was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the user was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A customer, which projects can be attached to.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Customer {
    /// The customer's identifier.
    pub id: String,
    /// The customer's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether the customer is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// The users managing this customer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managers: Option<Vec<String>>,
    /// When the customer was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the customer was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A project: the top of the hierarchy time is declared against.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Project {
    /// The project's identifier.
    pub id: String,
    /// The project's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The project's short code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// One of `active`, `done`, `archived`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Whether the project is billable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_billable: Option<bool>,
    /// When the project starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<String>,
    /// When the project is supposed to end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_at: Option<String>,
    /// `public` or `private`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    /// The users allowed to declare on a private project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contributors: Option<Vec<String>>,
    /// The users managing the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managers: Option<Vec<String>>,
    /// The groups assigned to the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    /// The tags attached to the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// The project note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The customer this project belongs to, when any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<String>,
    /// When the project was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the project was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Budget and time counters of a project.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectMetrics {
    /// The project these metrics belong to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The project's budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_amount: Option<f64>,
    /// How much of the budget is spent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_spent: Option<f64>,
    /// Planned time, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_planned: Option<i64>,
    /// Declared time, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_elapsed: Option<i64>,
}

/// A task: what a declaration actually points at.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Task {
    /// The task's identifier.
    pub id: String,
    /// The task's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The project the task belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The parent task, when this task is a subtask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Whether the task only holds other tasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_container: Option<bool>,
    /// One of `none`, `detailed`, `global`, `time`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_type: Option<String>,
    /// The task's budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_amount: Option<f64>,
    /// The task's hourly rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hourly_rate: Option<f64>,
    /// One of `task`, `group`, `contributor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hourly_rate_mode: Option<String>,
    /// Planned duration, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_duration: Option<i64>,
    /// When the task starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<String>,
    /// When the task is supposed to end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_at: Option<String>,
    /// When the task was completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// The groups assigned to the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    /// The tags attached to the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// When the task was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the task was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A declaration: one person, one task, one day, one duration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Declaration {
    /// The declaration's identifier.
    pub id: String,
    /// The day the time was spent on, as `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// The duration, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<i64>,
    /// What the time was spent on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The task the time is declared against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// The user the time belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// The last user who edited the duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_by: Option<String>,
    /// The user's hourly cost when the declaration was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hourly_cost: Option<f64>,
    /// When the declaration was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the declaration was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A leave period, which blocks time declarations over its days.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LeavePeriod {
    /// The leave period's identifier.
    pub id: String,
    /// The user on leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// The first day of the leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<String>,
    /// The last day of the leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_at: Option<String>,
    /// `full`, `am` or `pm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_type: Option<String>,
    /// `full`, `am` or `pm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_type: Option<String>,
    /// Why the person is away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Who last edited the leave period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_by: Option<String>,
    /// When the leave period was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the leave period was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A purchase charged to a project or a task.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Purchase {
    /// The purchase's identifier.
    pub id: String,
    /// What was bought.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The project it is charged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The task it is charged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// What it cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paid_price: Option<f64>,
    /// When it was paid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paid_at: Option<String>,
    /// What it was billed for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billed_price: Option<f64>,
    /// When it was billed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billed_at: Option<String>,
    /// `add` or `ignore`, how it weighs on the budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_policy: Option<String>,
    /// When the purchase was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the purchase was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A discount applied to a project or a task.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Discount {
    /// The discount's identifier.
    pub id: String,
    /// The discount's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// How much is taken off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    /// The project it applies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The task it applies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// When the discount was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the discount was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A tag, which belongs to a tagset.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Tag {
    /// The tag's identifier.
    pub id: String,
    /// The tag's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The tagset the tag belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagset_id: Option<String>,
    /// When the tag was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the tag was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A set of tags.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Tagset {
    /// The tagset's identifier.
    pub id: String,
    /// The tagset's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The colour shown in the interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Whether one tag of this set at most may be attributed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_tag_unique: Option<bool>,
    /// When the tagset was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the tagset was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A group of users, which belongs to a groupset.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Group {
    /// The group's identifier.
    pub id: String,
    /// The group's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The groupset the group belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groupset_id: Option<String>,
    /// The group's manager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager_id: Option<String>,
    /// When the group was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the group was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A set of groups.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Groupset {
    /// The groupset's identifier.
    pub id: String,
    /// The groupset's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The colour shown in the interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Whether one group of this set at most may be attributed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_group_unique: Option<bool>,
    /// When the groupset was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When the groupset was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// What a leave import did, and what it refused.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImportReport {
    /// Whether the import went through.
    #[serde(default)]
    pub ok: bool,
    /// Every line the importer rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<ImporterError>>,
}

/// One rejected line of a leave import.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImporterError {
    /// The line number in the imported file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    /// The machine-readable error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The human-readable message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Project {
    /// The name to show, falling back to the code and then the id.
    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .filter(|n| !n.is_empty())
            .or(self.code.as_deref().filter(|c| !c.is_empty()))
            .unwrap_or(&self.id)
    }
}

impl Task {
    /// The name to show, falling back to the id.
    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .filter(|n| !n.is_empty())
            .unwrap_or(&self.id)
    }
}

impl User {
    /// The name to show, falling back to the email and then the id.
    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .filter(|n| !n.is_empty())
            .or(self.email.as_deref().filter(|e| !e.is_empty()))
            .unwrap_or(&self.id)
    }
}
