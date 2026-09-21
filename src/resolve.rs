//! Names to identifiers.
//!
//! Gryzzly speaks UUIDs; people say "Alyxa" or "the redesign task". Every tool
//! that takes a project, a task, a user or a customer accepts either, and this
//! module is where the second becomes the first. It never guesses: a name that
//! matches two objects comes back as [`Error::Ambiguous`] with the candidates,
//! so the caller (or the model driving it) picks.

use std::sync::Arc;

use serde_json::json;

use crate::{
    cache::{TtlCell, TtlMap},
    client::Gryzzly,
    config::looks_like_uuid,
    error::{Candidate, Error, Result},
    model::{Customer, Project, Task, User},
};

/// How many candidates an ambiguity report carries at most.
const MAX_CANDIDATES: usize = 12;

/// A cached view of the team, used to turn names into identifiers.
#[derive(Debug)]
pub struct Directory {
    client: Arc<Gryzzly>,
    projects: TtlCell<Vec<Project>>,
    tasks: TtlMap<String, Vec<Task>>,
    all_tasks: TtlCell<Vec<Task>>,
    users: TtlCell<Vec<User>>,
    customers: TtlCell<Vec<Customer>>,
    current_user: TtlCell<User>,
}

impl Directory {
    /// Build a directory over a client, using the client's cache TTL.
    pub fn new(client: Arc<Gryzzly>) -> Self {
        let ttl = client.config().cache_ttl;
        Self {
            client,
            projects: TtlCell::new(ttl),
            tasks: TtlMap::new(ttl),
            all_tasks: TtlCell::new(ttl),
            users: TtlCell::new(ttl),
            customers: TtlCell::new(ttl),
            current_user: TtlCell::new(ttl),
        }
    }

    /// The underlying client.
    pub fn client(&self) -> &Arc<Gryzzly> {
        &self.client
    }

    /// Forget what a write just invalidated.
    pub async fn invalidate_projects(&self) {
        self.projects.clear().await;
    }

    /// Forget cached tasks, for every project.
    pub async fn invalidate_tasks(&self) {
        self.tasks.clear().await;
        self.all_tasks.clear().await;
    }

    /// Forget cached users, including the current one.
    pub async fn invalidate_users(&self) {
        self.users.clear().await;
        self.current_user.clear().await;
    }

    /// Forget cached customers.
    pub async fn invalidate_customers(&self) {
        self.customers.clear().await;
    }

    /// Every project, whatever its status.
    pub async fn projects(&self) -> Result<Arc<Vec<Project>>> {
        if let Some(cached) = self.projects.get().await {
            return Ok(cached);
        }
        let projects: Vec<Project> = self
            .client
            .list_all("projects.list", &json!({}), 5000)
            .await?;
        Ok(self.projects.set(projects).await)
    }

    /// Every task of one project.
    pub async fn tasks_of(&self, project_id: &str) -> Result<Arc<Vec<Task>>> {
        let key = project_id.to_owned();
        if let Some(cached) = self.tasks.get(&key).await {
            return Ok(cached);
        }
        let tasks: Vec<Task> = self
            .client
            .list_all("tasks.list", &json!({ "project_ids": [project_id] }), 5000)
            .await?;
        Ok(self.tasks.set(key, tasks).await)
    }

    /// Every task of the team, which is how a task id alone is resolved.
    pub async fn all_tasks(&self) -> Result<Arc<Vec<Task>>> {
        if let Some(cached) = self.all_tasks.get().await {
            return Ok(cached);
        }
        let tasks: Vec<Task> = self
            .client
            .list_all("tasks.list", &json!({}), 20000)
            .await?;
        Ok(self.all_tasks.set(tasks).await)
    }

    /// Every user of the team.
    pub async fn users(&self) -> Result<Arc<Vec<User>>> {
        if let Some(cached) = self.users.get().await {
            return Ok(cached);
        }
        let users: Vec<User> = self.client.list_all("users.list", &json!({}), 5000).await?;
        Ok(self.users.set(users).await)
    }

    /// Every customer of the team.
    pub async fn customers(&self) -> Result<Arc<Vec<Customer>>> {
        if let Some(cached) = self.customers.get().await {
            return Ok(cached);
        }
        let customers: Vec<Customer> = self
            .client
            .list_all("customers.list", &json!({}), 5000)
            .await?;
        Ok(self.customers.set(customers).await)
    }

    /// The user the server acts as.
    ///
    /// Gryzzly has no `/me`: a key is a team key, not a person. So the person is
    /// named by `GRYZZLY_USER_ID` or `GRYZZLY_USER_EMAIL`; a team of exactly one
    /// user needs neither.
    pub async fn current_user(&self) -> Result<Arc<User>> {
        if let Some(cached) = self.current_user.get().await {
            return Ok(cached);
        }
        let config = self.client.config();
        let user = if let Some(id) = &config.user_id {
            self.client
                .call::<_, User>("users.get", &json!({ "id": id }))
                .await
                .map_err(|e| match e {
                    Error::Api { status, .. } if status == 404 || status == 500 => {
                        Error::config(format!("GRYZZLY_USER_ID={id} is not a user of this team"))
                    }
                    other => other,
                })?
        } else if let Some(email) = &config.user_email {
            let users = self.users().await?;
            users
                .iter()
                .find(|u| {
                    u.email
                        .as_deref()
                        .is_some_and(|e| e.eq_ignore_ascii_case(email))
                })
                .cloned()
                .ok_or_else(|| {
                    Error::config(format!(
                        "GRYZZLY_USER_EMAIL={email} matches no user of this team"
                    ))
                })?
        } else {
            let users = self.users().await?;
            let active: Vec<&User> = users
                .iter()
                .filter(|u| u.disabled_at.is_none() && u.is_disabled != Some(true))
                .collect();
            match active.as_slice() {
                [only] => (*only).clone(),
                _ => {
                    return Err(Error::config(format!(
                        "this API key covers {} users, so the server cannot tell which one you are: set GRYZZLY_USER_ID (a UUID) or GRYZZLY_USER_EMAIL",
                        active.len()
                    )));
                }
            }
        };
        Ok(self.current_user.set(user).await)
    }

    /// Resolve a project by id, name, code, or unique fragment of either.
    pub async fn project(&self, needle: &str) -> Result<Project> {
        let needle = needle.trim();
        if needle.is_empty() {
            return Err(Error::invalid("project is empty"));
        }
        let projects = self.projects().await?;
        if looks_like_uuid(needle) {
            if let Some(found) = projects.iter().find(|p| p.id == needle) {
                return Ok(found.clone());
            }
            return self
                .client
                .call::<_, Project>("projects.get", &json!({ "id": needle }))
                .await;
        }
        let candidates: Vec<&Project> = projects.iter().collect();
        pick(
            "project",
            needle,
            &candidates,
            |p| {
                let mut keys = vec![p.label().to_owned()];
                if let Some(code) = p.code.as_deref().filter(|c| !c.is_empty()) {
                    keys.push(code.to_owned());
                }
                keys
            },
            |p| p.id.clone(),
        )
        .cloned()
    }

    /// Resolve a task, within a project when one is given.
    ///
    /// Task names repeat across projects ("Development" exists ten times), so a
    /// name without a project is only accepted when it matches exactly once in
    /// the whole team.
    pub async fn task(&self, needle: &str, project_id: Option<&str>) -> Result<Task> {
        let needle = needle.trim();
        if needle.is_empty() {
            return Err(Error::invalid("task is empty"));
        }
        if looks_like_uuid(needle) {
            if let Some(project_id) = project_id
                && let Some(found) = self
                    .tasks_of(project_id)
                    .await?
                    .iter()
                    .find(|t| t.id == needle)
            {
                return Ok(found.clone());
            }
            return self
                .client
                .call::<_, Task>("tasks.get", &json!({ "id": needle }))
                .await;
        }
        let tasks = match project_id {
            Some(project_id) => self.tasks_of(project_id).await?,
            None => self.all_tasks().await?,
        };
        // A container holds other tasks and takes no time, so it is not a
        // candidate unless nothing else matches.
        let declarable: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.is_container != Some(true))
            .collect();
        let searched: Vec<&Task> = if declarable.is_empty() {
            tasks.iter().collect()
        } else {
            declarable
        };
        pick(
            "task",
            needle,
            &searched,
            |t| vec![t.label().to_owned()],
            |t| t.id.clone(),
        )
        .cloned()
    }

    /// Resolve a user by id, email, name, or unique fragment. `None` is the
    /// current user.
    pub async fn user(&self, needle: Option<&str>) -> Result<User> {
        let Some(needle) = needle.map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok((*self.current_user().await?).clone());
        };
        if needle.eq_ignore_ascii_case("me") || needle.eq_ignore_ascii_case("self") {
            return Ok((*self.current_user().await?).clone());
        }
        let users = self.users().await?;
        if looks_like_uuid(needle) {
            if let Some(found) = users.iter().find(|u| u.id == needle) {
                return Ok(found.clone());
            }
            return self
                .client
                .call::<_, User>("users.get", &json!({ "id": needle }))
                .await;
        }
        let candidates: Vec<&User> = users.iter().collect();
        pick(
            "user",
            needle,
            &candidates,
            |u| {
                let mut keys = Vec::new();
                if let Some(name) = u.name.as_deref().filter(|n| !n.is_empty()) {
                    keys.push(name.to_owned());
                }
                if let Some(email) = u.email.as_deref().filter(|e| !e.is_empty()) {
                    keys.push(email.to_owned());
                }
                keys
            },
            |u| u.id.clone(),
        )
        .cloned()
    }

    /// Resolve a customer by id, name, or unique fragment.
    pub async fn customer(&self, needle: &str) -> Result<Customer> {
        let needle = needle.trim();
        if needle.is_empty() {
            return Err(Error::invalid("customer is empty"));
        }
        let customers = self.customers().await?;
        if looks_like_uuid(needle) {
            if let Some(found) = customers.iter().find(|c| c.id == needle) {
                return Ok(found.clone());
            }
            return self
                .client
                .call::<_, Customer>("customers.get", &json!({ "id": needle }))
                .await;
        }
        let candidates: Vec<&Customer> = customers.iter().collect();
        pick(
            "customer",
            needle,
            &candidates,
            |c| vec![c.name.clone().unwrap_or_default()],
            |c| c.id.clone(),
        )
        .cloned()
    }

    /// The project a task belongs to, from the cache when possible.
    pub async fn project_of_task(&self, task_id: &str) -> Option<Project> {
        let tasks = self.all_tasks().await.ok()?;
        let project_id = tasks.iter().find(|t| t.id == task_id)?.project_id.clone()?;
        let projects = self.projects().await.ok()?;
        projects.iter().find(|p| p.id == project_id).cloned()
    }
}

/// The matching ladder: exact, then prefix, then substring, then give up.
///
/// Every rung is tried across the whole set before the next one, so an exact
/// name always wins over another object that merely contains it.
fn pick<'a, T>(
    kind: &str,
    needle: &str,
    haystack: &[&'a T],
    keys: impl Fn(&T) -> Vec<String>,
    id_of: impl Fn(&T) -> String,
) -> Result<&'a T> {
    let wanted = needle.to_lowercase();
    let indexed: Vec<(&'a T, Vec<String>)> = haystack
        .iter()
        .map(|item| (*item, keys(item).iter().map(|k| k.to_lowercase()).collect()))
        .collect();

    for rung in [Rung::Exact, Rung::Prefix, Rung::Contains] {
        let hits: Vec<&'a T> = indexed
            .iter()
            .filter(|(_, keys)| keys.iter().any(|key| rung.matches(key, &wanted)))
            .map(|(item, _)| *item)
            .collect();
        match hits.as_slice() {
            [] => {}
            [only] => return Ok(only),
            many => {
                return Err(Error::Ambiguous {
                    message: format!(
                        "{} {} match {kind} {needle:?}: pass an id, or a name that matches only one",
                        many.len(),
                        if many.len() == 1 { "object" } else { "objects" }
                    ),
                    candidates: many
                        .iter()
                        .take(MAX_CANDIDATES)
                        .map(|item| Candidate {
                            id: id_of(item),
                            name: keys(item).first().cloned().unwrap_or_default(),
                        })
                        .collect(),
                });
            }
        }
    }

    Err(Error::not_found(format!(
        "no {kind} matches {needle:?}. List them first, then pass an id or an exact name."
    )))
}

#[derive(Copy, Clone)]
enum Rung {
    Exact,
    Prefix,
    Contains,
}

impl Rung {
    fn matches(self, key: &str, wanted: &str) -> bool {
        match self {
            Self::Exact => key == wanted,
            Self::Prefix => key.starts_with(wanted),
            Self::Contains => key.contains(wanted),
        }
    }
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

    #[derive(Debug, PartialEq)]
    struct Thing(&'static str);

    fn things(names: &[&'static str]) -> Vec<Thing> {
        names.iter().map(|n| Thing(n)).collect()
    }

    fn pick_name<'a>(needle: &str, items: &'a [Thing]) -> Result<&'a Thing> {
        let refs: Vec<&Thing> = items.iter().collect();
        pick(
            "thing",
            needle,
            &refs,
            |t| vec![t.0.to_owned()],
            |t| t.0.to_owned(),
        )
    }

    #[test]
    fn an_exact_name_wins_over_a_longer_one_that_contains_it() {
        let items = things(&["API", "API gateway", "internal API"]);
        assert_eq!(pick_name("api", &items).unwrap(), &Thing("API"));
    }

    #[test]
    fn a_unique_prefix_is_enough() {
        let items = things(&["Alyxa", "Beehive"]);
        assert_eq!(pick_name("aly", &items).unwrap(), &Thing("Alyxa"));
    }

    #[test]
    fn a_unique_fragment_in_the_middle_is_enough() {
        let items = things(&["Refonte du site", "Beehive"]);
        assert_eq!(
            pick_name("site", &items).unwrap(),
            &Thing("Refonte du site")
        );
    }

    #[test]
    fn two_matches_never_become_a_guess() {
        let items = things(&["API gateway", "API worker"]);
        let err = pick_name("api", &items).unwrap_err();
        match err {
            Error::Ambiguous { candidates, .. } => assert_eq!(candidates.len(), 2),
            other => panic!("expected an ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn nothing_matching_says_what_to_do_next() {
        let items = things(&["Alyxa"]);
        let err = pick_name("zzz", &items).unwrap_err().to_string();
        assert!(err.contains("List them first"), "{err}");
    }

    #[test]
    fn matching_ignores_case_on_both_sides() {
        let items = things(&["Ciné Visite"]);
        assert_eq!(
            pick_name("CINÉ visite", &items).unwrap(),
            &Thing("Ciné Visite")
        );
    }
}
