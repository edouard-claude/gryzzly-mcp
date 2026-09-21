//! A fake Gryzzly, so the tests exercise the real client and the real MCP
//! surface without touching anybody's timesheet.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    dead_code,
    reason = "test support code"
)]

use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

/// The user the fake team acts as.
pub const USER_ID: &str = "2d5b507c-d5ff-49c8-9fd3-cb8778bc6611";
/// A colleague, so the team is not a team of one.
pub const OTHER_USER_ID: &str = "8c71abc1-dbdc-4a96-aab9-c3f4e9b546fd";
/// The project every test logs against.
pub const PROJECT_ID: &str = "dbcaff37-05ae-44e2-9b5c-1a917c6a6524";
/// A second project, so a name lookup has something to be wrong about.
pub const OTHER_PROJECT_ID: &str = "d12d2464-8540-4867-bc83-857f10ddf06a";
/// A third project whose name starts like the first, so `aly` is ambiguous and
/// `Alyxa` is not.
pub const SIBLING_PROJECT_ID: &str = "3f5a1c74-1d20-4a9e-9ef1-2b6d9a8c5511";
/// The task every test logs against.
pub const TASK_ID: &str = "92b3868d-d209-4160-8a87-df07817778da";
/// A container task, which must never be proposed for a declaration.
pub const CONTAINER_TASK_ID: &str = "49e2508b-1d5b-47b3-aba2-f94b48718cfd";
/// The declaration the fake already holds.
pub const DECLARATION_ID: &str = "c14b7ffd-d2cc-4b9a-b9ba-4fc5c427d974";

/// A running fake Gryzzly.
pub struct Fake {
    pub server: MockServer,
}

impl Fake {
    /// Start a fake with nothing mounted.
    ///
    /// Wiremock answers with the first mock mounted that matches, so a test that
    /// needs its own answer for an action the standard surface covers starts
    /// from here.
    pub async fn bare() -> Self {
        Self {
            server: MockServer::start().await,
        }
    }

    /// Start a fake with the whole read surface mounted.
    pub async fn start() -> Self {
        let fake = Self::bare().await;
        fake.mount(
            "users.list",
            page(vec![
                user(USER_ID, "Edouard Claude", "edouard@example.test"),
                user(OTHER_USER_ID, "Thomas Lemaitre", "thomas@example.test"),
            ]),
        )
        .await;
        fake.mount(
            "users.get",
            user(USER_ID, "Edouard Claude", "edouard@example.test"),
        )
        .await;
        fake.mount(
            "projects.list",
            page(vec![
                project(PROJECT_ID, "Alyxa", "ALX"),
                project(SIBLING_PROJECT_ID, "Alyxa Mobile", "ALXM"),
                project(OTHER_PROJECT_ID, "Ciné Visite", ""),
            ]),
        )
        .await;
        fake.mount(
            "tasks.list",
            page(vec![
                task(TASK_ID, PROJECT_ID, "Development", false),
                task(CONTAINER_TASK_ID, PROJECT_ID, "Phases", true),
            ]),
        )
        .await;
        fake.mount(
            "declarations.list",
            page(vec![declaration(
                DECLARATION_ID,
                "2026-09-15",
                18000,
                "R&D",
            )]),
        )
        .await;
        fake.mount(
            "declarations.create",
            declaration("new-declaration-id", "2026-09-16", 9000, "Pairing"),
        )
        .await;
        // The documented quirk: nothing to return is a 404, not an empty page.
        fake.mount_status(
            "customers.list",
            404,
            json!({"error_type": "not_found", "error_description": ""}),
        )
        .await;
        fake
    }

    /// Mount a 200 answer for one action.
    pub async fn mount(&self, action: &str, body: Value) {
        self.mount_status(action, 200, body).await;
    }

    /// Mount an arbitrary status for one action.
    pub async fn mount_status(&self, action: &str, status: u16, body: Value) {
        Mock::given(method("POST"))
            .and(path(format!("/v1/{action}")))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    /// The base URL to hand to the server under test.
    pub fn base_url(&self) -> String {
        format!("{}/v1", self.server.uri())
    }

    /// Every body the fake was sent for one action.
    pub async fn bodies(&self, action: &str) -> Vec<Value> {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.url.path() == format!("/v1/{action}"))
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }
}

/// Wrap objects in the envelope every list action answers with.
pub fn page(items: Vec<Value>) -> Value {
    json!({
        "offset": 0,
        "limit": items.len(),
        "count": items.len(),
        "data": items,
    })
}

/// A user, as the API returns one.
pub fn user(id: &str, name: &str, email: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "email": email,
        "code": "",
        "role": "admin",
        "is_disabled": false,
        "disabled_at": null,
        "timesheet_approved_until": null,
        "hourly_cost": null,
        "hourly_rate": null,
        "groups": [],
        "created_at": "2025-11-03T09:09:14.848753Z",
        "updated_at": "2026-09-20T03:33:27.492487Z"
    })
}

/// A project, as the API returns one.
pub fn project(id: &str, name: &str, code: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "code": code,
        "status": "active",
        "is_billable": false,
        "start_at": "2025-11-19",
        "end_at": null,
        "visibility": "public",
        "contributors": null,
        "managers": [],
        "groups": null,
        "tags": null,
        "note": "",
        "customer_id": null,
        "created_at": "2025-11-19T07:35:12.404869Z",
        "updated_at": "2025-11-19T07:35:25.097863Z"
    })
}

/// A task, as the API returns one.
pub fn task(id: &str, project_id: &str, name: &str, is_container: bool) -> Value {
    json!({
        "id": id,
        "project_id": project_id,
        "name": name,
        "is_container": is_container,
        "completed_at": null,
        "parent_id": null,
        "budget_type": "none",
        "start_at": "2025-11-19",
        "end_at": null,
        "tags": [],
        "groups": [],
        "created_at": "2025-07-10T11:53:45.099793Z",
        "updated_at": "2025-11-03T09:11:20.639136Z"
    })
}

/// A declaration, as the API returns one.
pub fn declaration(id: &str, date: &str, duration: i64, description: &str) -> Value {
    json!({
        "id": id,
        "date": date,
        "duration": duration,
        "description": description,
        "task_id": TASK_ID,
        "user_id": USER_ID,
        "edited_by": USER_ID,
        "hourly_cost": null,
        "created_at": "2026-08-31T06:32:20.066038Z",
        "updated_at": "2026-08-31T06:32:20.066038Z"
    })
}

/// The environment a test server runs with.
pub fn env(fake: &Fake, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut vars = vec![
        ("GRYZZLY_API_KEY".to_owned(), "test-key".to_owned()),
        ("GRYZZLY_BASE_URL".to_owned(), fake.base_url()),
        ("GRYZZLY_USER_ID".to_owned(), USER_ID.to_owned()),
    ];
    for (key, value) in extra {
        vars.retain(|(existing, _)| existing != key);
        vars.push(((*key).to_owned(), (*value).to_owned()));
    }
    vars
}

/// Build a configuration from a list of pairs.
pub fn config(vars: &[(String, String)]) -> mcp_gryzzly::Config {
    let owned = vars.to_vec();
    mcp_gryzzly::Config::from_getter(move |key| {
        owned
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value.clone())
    })
    .expect("test configuration")
}
