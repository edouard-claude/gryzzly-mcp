//! What this client promises about Gryzzly's manners.
//!
//! Each test here stands for a line of the README: the empty-list 404, the
//! retry on 429, the message on a bad key, the exact body sent for a week of
//! declarations.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use common::{Fake, config, declaration, env, page};
use mcp_gryzzly::{
    Gryzzly,
    error::Error,
    model::{Customer, Declaration, Page, User},
};
use serde_json::json;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

fn client(fake: &Fake) -> Gryzzly {
    Gryzzly::new(config(&env(fake, &[]))).expect("client")
}

#[tokio::test]
async fn an_empty_list_arrives_as_a_404_and_reads_as_an_empty_page() {
    let fake = Fake::start().await;
    let page: Page<Customer> = client(&fake)
        .list("customers.list", &json!({}))
        .await
        .expect("a 404 on a list is not an error");
    assert_eq!(page.count, 0);
    assert!(page.data.is_empty());
}

#[tokio::test]
async fn a_404_on_a_single_object_stays_an_error() {
    let fake = Fake::start().await;
    fake.mount_status(
        "projects.get",
        404,
        json!({"error_type": "not_found", "error_description": ""}),
    )
    .await;
    let err = client(&fake)
        .call::<_, serde_json::Value>("projects.get", &json!({ "id": "nope" }))
        .await
        .expect_err("an unknown id is an error");
    assert_eq!(err.api_error_type(), Some("not_found"));
}

#[tokio::test]
async fn a_rate_limited_request_is_retried_and_then_succeeds() {
    let fake = Fake::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/tags.list"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error_type": "rate_limits",
            "error_description": "allowed 50 requests per 10 seconds"
        })))
        .up_to_n_times(1)
        .mount(&fake.server)
        .await;
    fake.mount("tags.list", page(vec![])).await;

    let page: Page<serde_json::Value> = client(&fake)
        .list("tags.list", &json!({}))
        .await
        .expect("the retry succeeds");
    assert_eq!(page.count, 0);
    assert_eq!(fake.bodies("tags.list").await.len(), 2, "it retried once");
}

#[tokio::test]
async fn a_retry_budget_of_zero_fails_on_the_first_rate_limit() {
    let fake = Fake::start().await;
    fake.mount_status(
        "tags.list",
        429,
        json!({ "error_type": "rate_limits", "error_description": "" }),
    )
    .await;
    let client =
        Gryzzly::new(config(&env(&fake, &[("GRYZZLY_MAX_RETRIES", "0")]))).expect("client");
    let err = client
        .list::<_, serde_json::Value>("tags.list", &json!({}))
        .await
        .expect_err("no retries left");
    assert!(err.is_transient());
    assert_eq!(fake.bodies("tags.list").await.len(), 1);
}

#[tokio::test]
async fn an_empty_401_body_still_explains_itself() {
    let fake = Fake::start().await;
    fake.mount_status("groups.list", 401, json!({})).await;
    let err = client(&fake)
        .list::<_, serde_json::Value>("groups.list", &json!({}))
        .await
        .expect_err("401");
    let message = err.to_string();
    assert!(message.contains("GRYZZLY_API_KEY"), "{message}");
    assert!(!err.is_transient(), "a bad key is not worth retrying");
}

#[tokio::test]
async fn the_api_key_travels_as_a_bearer_token() {
    let fake = Fake::start().await;
    let _: Page<User> = client(&fake).list("users.list", &json!({})).await.unwrap();
    let requests = fake.server.received_requests().await.unwrap_or_default();
    let authorization = requests
        .iter()
        .find(|r| r.url.path() == "/v1/users.list")
        .and_then(|r| r.headers.get("authorization"))
        .and_then(|value| value.to_str().ok())
        .expect("an Authorization header");
    assert_eq!(authorization, "Bearer test-key");
}

#[tokio::test]
async fn pagination_walks_every_page_and_stops() {
    let fake = Fake::bare().await;
    let first: Vec<serde_json::Value> = (0..1000)
        .map(|i| declaration(&format!("id-{i}"), "2026-09-15", 3600, "work"))
        .collect();
    Mock::given(method("POST"))
        .and(path("/v1/declarations.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0,
            "limit": 1000,
            "count": 1002,
            "data": first,
        })))
        .up_to_n_times(1)
        .mount(&fake.server)
        .await;
    fake.mount(
        "declarations.list",
        json!({
            "offset": 1000,
            "limit": 1000,
            "count": 1002,
            "data": [declaration("id-1000", "2026-09-16", 3600, "work"), declaration("id-1001", "2026-09-16", 3600, "work")],
        }),
    )
    .await;

    let all: Vec<Declaration> = client(&fake)
        .list_all("declarations.list", &json!({ "user_ids": [] }), 20000)
        .await
        .expect("both pages");
    assert_eq!(all.len(), 1002);
    let bodies = fake.bodies("declarations.list").await;
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[1]["offset"], 1000);
}

#[tokio::test]
async fn a_2xx_that_is_not_the_expected_shape_is_a_decode_error_naming_the_action() {
    let fake = Fake::start().await;
    fake.mount("tasks.get", json!({ "unexpected": true })).await;
    let err = client(&fake)
        .call::<_, mcp_gryzzly::model::Task>("tasks.get", &json!({ "id": "x" }))
        .await
        .expect_err("no id in the answer");
    match err {
        Error::Decode { action, .. } => assert_eq!(action, "tasks.get"),
        other => panic!("expected a decode error, got {other:?}"),
    }
}
