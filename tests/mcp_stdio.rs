//! The protocol, end to end.
//!
//! These tests spawn the real binary, speak JSON-RPC to its stdin and read its
//! stdout, with a fake Gryzzly behind it. If the handshake, the tool list, the
//! resources, the prompts or a tool call ever stop being what the MCP
//! specification asks for, this file fails.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use std::process::Stdio;

use common::{Fake, USER_ID, env};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// A connected MCP client, over the binary's stdio.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl Mcp {
    /// Spawn the server and complete the handshake.
    async fn start(vars: &[(String, String)]) -> (Self, Value) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-gryzzly"));
        command
            .env_clear()
            .env("GRYZZLY_LOG", "warn")
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (key, value) in vars {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("the binary runs");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let mut mcp = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 0,
        };
        let initialized = mcp
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "mcp-gryzzly-tests", "version": "0" }
                }),
            )
            .await;
        mcp.notify("notifications/initialized", json!({})).await;
        (mcp, initialized)
    }

    /// Send a request and wait for the answer carrying its id.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;
        loop {
            let message = self.read().await;
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    /// Send a notification, which expects no answer.
    async fn notify(&mut self, method: &str, params: Value) {
        self.write(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await;
    }

    /// Call a tool and return the whole JSON-RPC message, error or result.
    async fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
        .await
    }

    /// Call a tool that is expected to succeed, and return its structured
    /// content.
    async fn structured(&mut self, name: &str, arguments: Value) -> Value {
        let message = self.call_tool(name, arguments).await;
        let result = message
            .get("result")
            .unwrap_or_else(|| panic!("{name} failed: {message}"));
        assert_ne!(
            result.get("isError").and_then(Value::as_bool),
            Some(true),
            "{name} answered an error: {result}"
        );
        result
            .get("structuredContent")
            .cloned()
            .unwrap_or_else(|| panic!("{name} answered without structured content: {result}"))
    }

    async fn write(&mut self, value: Value) {
        let mut line = serde_json::to_vec(&value).expect("serialize");
        line.push(b'\n');
        self.stdin.write_all(&line).await.expect("write");
        self.stdin.flush().await.expect("flush");
    }

    async fn read(&mut self) -> Value {
        let line = tokio::time::timeout(std::time::Duration::from_secs(20), self.lines.next_line())
            .await
            .expect("the server answers within twenty seconds")
            .expect("stdout is readable")
            .expect("the server did not close stdout");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON: {line:?} ({e})"))
    }

    /// Close the connection and reap the process.
    async fn shutdown(mut self) {
        drop(self.stdin);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await;
        let _ = self.child.kill().await;
    }
}

#[tokio::test]
async fn the_handshake_advertises_tools_resources_prompts_and_instructions() {
    let fake = Fake::start().await;
    let (mcp, initialized) = Mcp::start(&env(&fake, &[])).await;
    let result = &initialized["result"];

    assert_eq!(result["protocolVersion"], "2025-11-25");
    assert_eq!(result["serverInfo"]["name"], "gryzzly");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["prompts"].is_object());
    assert!(result["capabilities"]["resources"].is_object());

    let instructions = result["instructions"]
        .as_str()
        .expect("the server explains itself");
    assert!(
        instructions.contains("task"),
        "the instructions must say time hangs on tasks: {instructions}"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn the_negotiated_version_is_the_one_the_client_asked_for() {
    let fake = Fake::start().await;
    let (mcp, initialized) = Mcp::start(&env(&fake, &[])).await;
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    mcp.shutdown().await;
}

#[tokio::test]
async fn every_tool_is_listed_with_a_schema_and_an_annotation() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let listed = mcp.request("tools/list", json!({})).await;
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("a list of tools");
    assert_eq!(tools.len(), 53, "the whole API plus the three composites");

    for tool in tools {
        let name = tool["name"].as_str().expect("a name");
        assert!(
            tool["description"].as_str().is_some_and(|d| d.len() > 30),
            "{name} has no usable description"
        );
        assert_eq!(
            tool["inputSchema"]["type"], "object",
            "{name} has no object input schema"
        );
        assert!(
            tool["outputSchema"].is_object(),
            "{name} has no output schema"
        );
        assert!(
            tool["annotations"]["title"].as_str().is_some(),
            "{name} has no title"
        );
    }

    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "whoami",
        "log_time",
        "log_time_bulk",
        "list_time_entries",
        "update_time_entry",
        "delete_time_entry",
        "timesheet",
        "list_projects",
        "list_tasks",
        "import_leave_periods",
        "export_times_and_leaves",
    ] {
        assert!(names.contains(&expected), "{expected} is missing");
    }
    mcp.shutdown().await;
}

#[tokio::test]
async fn resources_and_templates_are_listed_and_readable() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;

    let listed = mcp.request("resources/list", json!({})).await;
    let resources = listed["result"]["resources"].as_array().expect("resources");
    assert_eq!(resources.len(), 4);

    let templates = mcp.request("resources/templates/list", json!({})).await;
    assert_eq!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    let read = mcp
        .request("resources/read", json!({ "uri": "gryzzly://projects" }))
        .await;
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("json text");
    let parsed: Value = serde_json::from_str(text).expect("the resource is JSON");
    assert_eq!(parsed["count"], 3);

    let templated = mcp
        .request(
            "resources/read",
            json!({ "uri": "gryzzly://projects/Alyxa/tasks" }),
        )
        .await;
    let text = templated["result"]["contents"][0]["text"]
        .as_str()
        .expect("json text");
    assert!(text.contains("Development"), "{text}");

    let missing = mcp
        .request("resources/read", json!({ "uri": "gryzzly://nope" }))
        .await;
    assert!(
        missing["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("gryzzly://me")),
        "an unknown resource lists what exists: {missing}"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn the_three_prompts_are_listed_and_rendered() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;

    let listed = mcp.request("prompts/list", json!({})).await;
    let prompts = listed["result"]["prompts"].as_array().expect("prompts");
    assert_eq!(prompts.len(), 3);

    let rendered = mcp
        .request(
            "prompts/get",
            json!({ "name": "fill_my_week", "arguments": { "week": "2026-W38" } }),
        )
        .await;
    let text = rendered["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("a message");
    assert!(text.contains("2026-W38"), "{text}");
    assert!(
        text.contains("wait for my explicit agreement"),
        "the prompt must stop before writing: {text}"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn whoami_names_the_user_and_the_week() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let who = mcp.structured("whoami", json!({})).await;

    assert_eq!(who["user"]["id"], USER_ID);
    assert_eq!(who["user"]["email"], "edouard@example.test");
    assert_eq!(who["identified_by"], format!("GRYZZLY_USER_ID={USER_ID}"));
    assert_eq!(who["team_users"], 2);
    assert_eq!(who["read_only"], false);
    assert!(who["this_week"]["total"].is_string());
    mcp.shutdown().await;
}

#[tokio::test]
async fn logging_time_resolves_a_project_name_and_a_human_duration() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;

    let logged = mcp
        .structured(
            "log_time",
            json!({
                "project": "Alyxa",
                "duration": "2h30",
                "date": "2026-09-16",
                "description": "Pairing"
            }),
        )
        .await;
    assert_eq!(logged["entry"]["duration"], "2h30");
    assert_eq!(logged["entry"]["project"], "Alyxa");
    assert_eq!(logged["entry"]["task"], "Development");

    let bodies = fake.bodies("declarations.create").await;
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["duration"], 9000, "2h30 is nine thousand seconds");
    assert_eq!(bodies[0]["date"], "2026-09-16");
    assert_eq!(bodies[0]["user_id"], USER_ID);
    assert_eq!(
        bodies[0]["task_id"],
        common::TASK_ID,
        "the project's only declarable task was chosen, not its container"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn an_ambiguous_name_comes_back_with_its_candidates() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let message = mcp
        .call_tool("log_time", json!({ "project": "aly", "duration": "1h" }))
        .await;
    let error = &message["error"];
    assert_eq!(
        error["code"], -32602,
        "an ambiguity is an invalid parameter"
    );
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains("match project")),
        "{error}"
    );
    let candidates = error["data"]["candidates"]
        .as_array()
        .expect("the candidates are handed back");
    assert_eq!(candidates.len(), 2);
    assert!(
        fake.bodies("declarations.create").await.is_empty(),
        "nothing was written"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_delete_without_confirmation_writes_nothing() {
    let fake = Fake::start().await;
    fake.mount("declarations.delete", json!({ "ok": true }))
        .await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;

    let refused = mcp
        .call_tool("delete_time_entry", json!({ "id": common::DECLARATION_ID }))
        .await;
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("confirm: true")),
        "{refused}"
    );
    assert!(fake.bodies("declarations.delete").await.is_empty());

    let deleted = mcp
        .structured(
            "delete_time_entry",
            json!({ "id": common::DECLARATION_ID, "confirm": true }),
        )
        .await;
    assert_eq!(deleted["deleted"], true);
    assert_eq!(fake.bodies("declarations.delete").await.len(), 1);
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_week_of_declarations_is_asked_for_the_way_the_api_wants_it() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let report = mcp
        .structured("timesheet", json!({ "week": "2026-W38" }))
        .await;

    assert_eq!(report["from"], "2026-09-14");
    assert_eq!(report["to"], "2026-09-20");
    assert_eq!(report["week"], "2026-W38");
    assert_eq!(report["total"], "5h");
    assert_eq!(report["days"][0]["weekday"], "Tuesday");
    assert_eq!(report["projects"][0]["project"], "Alyxa");
    assert_eq!(report["projects"][0]["share_percent"], 100.0);

    let body = fake
        .bodies("declarations.list")
        .await
        .into_iter()
        .next()
        .expect("the list was called");
    assert_eq!(body["user_ids"][0], USER_ID);
    assert!(
        body["task_ids"].is_array(),
        "task_ids must be present, even empty: Gryzzly 404s otherwise ({body})"
    );
    assert_eq!(body["from"], "2026-09-14");
    assert_eq!(body["to"], "2026-09-20");
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_project_filter_narrows_the_totals_not_just_the_entries() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;

    let kept = mcp
        .structured(
            "timesheet",
            json!({ "week": "2026-W38", "project": "Alyxa Mobile" }),
        )
        .await;
    assert_eq!(
        kept["total"], "0s",
        "the week's time belongs to another project, so the total is zero: {kept}"
    );
    assert_eq!(kept["projects"].as_array().map(Vec::len), Some(0));

    let matching = mcp
        .structured(
            "timesheet",
            json!({ "week": "2026-W38", "project": "Alyxa" }),
        )
        .await;
    assert_eq!(matching["total"], "5h");
    assert_eq!(matching["projects"][0]["share_percent"], 100.0);

    let entries = mcp
        .structured(
            "list_time_entries",
            json!({ "week": "2026-W38", "project": "Alyxa" }),
        )
        .await;
    assert_eq!(entries["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(entries["entries"][0]["duration"], "5h");
    assert_eq!(entries["entries"][0]["task"], "Development");
    mcp.shutdown().await;
}

#[tokio::test]
async fn an_empty_upstream_list_is_an_empty_listing_not_a_failure() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let listing = mcp.structured("list_customers", json!({})).await;
    assert_eq!(listing["count"], 0);
    assert_eq!(listing["returned"], 0);
    assert_eq!(listing["items"].as_array().map(Vec::len), Some(0));
    mcp.shutdown().await;
}

#[tokio::test]
async fn read_only_mode_hides_every_write_tool_from_the_list() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[("GRYZZLY_READ_ONLY", "1")])).await;
    let listed = mcp.request("tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"timesheet"));
    assert!(!names.contains(&"log_time"));

    let refused = mcp.call_tool("log_time", json!({ "duration": "1h" })).await;
    assert!(
        refused["error"].is_object(),
        "a hidden tool cannot be called: {refused}"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_toolset_selection_shrinks_the_surface() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[("GRYZZLY_TOOLSETS", "time")])).await;
    let listed = mcp.request("tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names.len(), 13, "{names:?}");
    assert!(names.contains(&"log_time"));
    assert!(names.contains(&"list_tasks"));
    assert!(!names.contains(&"invite_user"));
    mcp.shutdown().await;
}

#[tokio::test]
async fn an_unknown_tool_is_a_protocol_error_not_a_tool_failure() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let message = mcp.call_tool("log_my_hours_please", json!({})).await;
    assert_eq!(message["error"]["code"], -32602, "{message}");
    assert!(message["result"].is_null(), "{message}");
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_bad_duration_is_refused_before_anything_is_written() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let message = mcp
        .call_tool(
            "log_time",
            json!({ "project": "Alyxa", "duration": "two hours" }),
        )
        .await;
    assert!(
        message["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("2h30")),
        "the error shows the accepted forms: {message}"
    );
    assert!(fake.bodies("declarations.create").await.is_empty());
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_bulk_log_reports_every_entry_in_order() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let report = mcp
        .structured(
            "log_time_bulk",
            json!({
                "entries": [
                    { "project": "Alyxa", "duration": "2h", "date": "2026-09-14" },
                    { "project": "Alyxa", "duration": "nonsense", "date": "2026-09-15" },
                    { "project": "Alyxa", "duration": "1h30", "date": "2026-09-16" }
                ]
            }),
        )
        .await;
    assert_eq!(report["created"], 2);
    assert_eq!(report["failed"], 1);
    assert_eq!(report["outcomes"][1]["created"], false);
    assert_eq!(report["outcomes"][1]["index"], 1);
    assert!(report["outcomes"][1]["error"].is_string());
    assert_eq!(report["outcomes"][2]["created"], true);
    assert_eq!(fake.bodies("declarations.create").await.len(), 2);
    mcp.shutdown().await;
}
