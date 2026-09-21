//! MCP resources: the same data as the read tools, for clients that would
//! rather attach context than call a function.
//!
//! Four fixed URIs and two templates. Everything is JSON, everything is
//! read-only, and everything is scoped to the one API key this process holds.

use rmcp::model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate};

use crate::{
    dates,
    error::{Error, Result},
    server::GryzzlyServer,
    tools::time::{EntryFilter, report},
};

/// The URI scheme every resource of this server lives under.
pub const SCHEME: &str = "gryzzly://";

/// The current user and their week.
pub const URI_ME: &str = "gryzzly://me";
/// Every project.
pub const URI_PROJECTS: &str = "gryzzly://projects";
/// Every user of the team.
pub const URI_USERS: &str = "gryzzly://users";
/// Every customer.
pub const URI_CUSTOMERS: &str = "gryzzly://customers";

/// The resources a client can read without filling in anything.
pub fn catalogue() -> Vec<Resource> {
    vec![
        Resource::new(URI_ME, "me")
            .with_title("Me, this week")
            .with_description(
                "The user this server declares time for, and what is already logged this week.",
            )
            .with_mime_type("application/json"),
        Resource::new(URI_PROJECTS, "projects")
            .with_title("Projects")
            .with_description("Every project, with its status, code and customer.")
            .with_mime_type("application/json"),
        Resource::new(URI_USERS, "users")
            .with_title("Users")
            .with_description("Every user this API key can see, with their role.")
            .with_mime_type("application/json"),
        Resource::new(URI_CUSTOMERS, "customers")
            .with_title("Customers")
            .with_description("Every customer, with its managers.")
            .with_mime_type("application/json"),
    ]
}

/// The resources a client reads by filling in a variable.
pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("gryzzly://projects/{project}/tasks", "project-tasks")
            .with_title("Tasks of a project")
            .with_description(
                "The tasks of one project, which is what time is declared against. The variable accepts a project name, code or id.",
            )
            .with_mime_type("application/json"),
        ResourceTemplate::new("gryzzly://timesheet/{week}", "timesheet")
            .with_title("Timesheet of a week")
            .with_description(
                "One week of declared time for the current user, totalled by day and by project. The variable is an ISO week such as 2026-W38, or `this week`.",
            )
            .with_mime_type("application/json"),
    ]
}

/// Read one resource by URI.
pub async fn read(server: &GryzzlyServer, uri: &str) -> Result<ReadResourceResult> {
    let directory = server.directory();
    let json = match uri {
        URI_ME => {
            let user = directory.current_user().await?;
            let (monday, sunday) = dates::parse_week("this week")?;
            let week = report(
                server,
                &user,
                monday,
                sunday,
                7.0,
                false,
                &EntryFilter::default(),
            )
            .await?;
            serde_json::json!({
                "user": crate::tools::time::UserRef::from(&*user),
                "identified_by": server.config().identity_hint(),
                "this_week": week,
            })
        }
        URI_PROJECTS => {
            let projects = directory.projects().await?;
            serde_json::json!({ "count": projects.len(), "projects": &*projects })
        }
        URI_USERS => {
            let users = directory.users().await?;
            serde_json::json!({ "count": users.len(), "users": &*users })
        }
        URI_CUSTOMERS => {
            let customers = directory.customers().await?;
            serde_json::json!({ "count": customers.len(), "customers": &*customers })
        }
        other => {
            let rest = other.strip_prefix(SCHEME).ok_or_else(|| unknown(other))?;
            if let Some(week) = rest.strip_prefix("timesheet/") {
                let user = directory.current_user().await?;
                let (from, to) = dates::parse_week(&decode(week))?;
                let report =
                    report(server, &user, from, to, 7.0, true, &EntryFilter::default()).await?;
                serde_json::to_value(report).map_err(|source| Error::Decode {
                    action: "timesheet".to_owned(),
                    source,
                })?
            } else if let Some(project) = rest
                .strip_prefix("projects/")
                .and_then(|rest| rest.strip_suffix("/tasks"))
            {
                let project = directory.project(&decode(project)).await?;
                let tasks = directory.tasks_of(&project.id).await?;
                serde_json::json!({
                    "project": { "id": project.id, "name": project.label() },
                    "count": tasks.len(),
                    "tasks": &*tasks,
                })
            } else {
                return Err(unknown(other));
            }
        }
    };

    let text = serde_json::to_string_pretty(&json).map_err(|source| Error::Decode {
        action: uri.to_owned(),
        source,
    })?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(text, uri).with_mime_type("application/json"),
    ]))
}

fn unknown(uri: &str) -> Error {
    Error::not_found(format!(
        "no resource at {uri:?}. This server serves {URI_ME}, {URI_PROJECTS}, {URI_USERS}, {URI_CUSTOMERS}, gryzzly://projects/{{project}}/tasks and gryzzly://timesheet/{{week}}."
    ))
}

/// Decode the few percent escapes a URI path can carry.
///
/// Project names hold spaces and accents, so `gryzzly://projects/Cin%C3%A9%20Visite/tasks`
/// has to come back as `Ciné Visite`.
fn decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
    fn the_catalogue_and_the_templates_are_documented() {
        for resource in catalogue() {
            assert!(resource.uri.starts_with(SCHEME), "{}", resource.uri);
            assert!(resource.title.is_some(), "{} has no title", resource.uri);
            assert!(
                resource.description.is_some(),
                "{} has no description",
                resource.uri
            );
            assert_eq!(resource.mime_type.as_deref(), Some("application/json"));
        }
        for template in templates() {
            assert!(template.uri_template.contains('{'), "no variable");
            assert!(template.description.is_some());
        }
    }

    #[test]
    fn an_unknown_uri_lists_what_exists() {
        let message = unknown("gryzzly://nope").to_string();
        assert!(message.contains("gryzzly://me"), "{message}");
        assert!(message.contains("timesheet"), "{message}");
    }

    #[test]
    fn percent_escapes_come_back_as_the_name_they_encode() {
        assert_eq!(decode("Cin%C3%A9%20Visite"), "Ciné Visite");
        assert_eq!(decode("Refonte+du+site"), "Refonte du site");
        assert_eq!(decode("plain"), "plain");
        assert_eq!(decode("100%"), "100%");
    }
}
