use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use silverbond::{
    api,
    app::{AppPaths, AppState, PaneStreamRegistry, SecurityConfig, sha256_unlock_password_hash},
    runtime::{PersistedRun, RuntimeContext, RuntimeStatus},
    storage::{Database, TemplateStore, WorkflowStore},
};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tower::ServiceExt;

const SEC_FETCH_SITE: &str = "sec-fetch-site";

async fn test_router() -> (TempDir, Router) {
    let (temp, router, _) = test_router_with_db().await;
    (temp, router)
}

async fn test_router_with_db() -> (TempDir, Router, Database) {
    test_router_with_security(SecurityConfig {
        agent_user: None,
        unlock_password_hash: Some(sha256_unlock_password_hash("test-unlock")),
    })
    .await
}

async fn test_router_with_security(security: SecurityConfig) -> (TempDir, Router, Database) {
    let temp = TempDir::new().unwrap();
    let paths = AppPaths::from_root(temp.path());
    std::fs::create_dir_all(&paths.workflows_dir).unwrap();
    std::fs::create_dir_all(&paths.templates_dir).unwrap();
    std::fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    let db = Database::new(paths.database_path.clone());
    db.init().await.unwrap();
    let state = AppState {
        paths: paths.clone(),
        workflows: WorkflowStore::new(paths.workflows_dir.clone()),
        templates: TemplateStore::new(paths.templates_dir.clone()),
        runtime: RuntimeContext::new(db.clone()),
        pane_streams: PaneStreamRegistry::default(),
        security,
    };
    (temp, api::router(state), db)
}

async fn json_response(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, value)
}

async fn response_status(router: &Router, request: Request<Body>) -> StatusCode {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let _ = response.into_body().collect().await.unwrap();
    status
}

async fn wait_for_run<F>(db: &Database, run_id: &str, mut predicate: F) -> PersistedRun
where
    F: FnMut(&PersistedRun) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(persisted) = db.get_run(run_id).await.unwrap() {
            if predicate(&persisted) {
                return persisted;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for run {run_id}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn echo_workflow() -> Value {
    json!({
        "version": 4,
        "name": "Echo Finish",
        "goal": "Finish",
        "cwd": "",
        "useOrchestrator": false,
        "entryNodeId": "n1",
        "variables": [],
        "limits": { "maxTotalSteps": 5, "maxVisitsPerNode": 5 },
        "nodes": [
            {
                "id": "n1",
                "name": "Echo",
                "agent": "echo",
                "prompt": "done",
                "kind": { "type": "task" }
            }
        ],
        "edges": []
    })
}

async fn create_terminal_echo_run(router: &Router, db: &Database) -> (String, String) {
    let (status, create) = json_response(
        router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "workflow": echo_workflow(),
                    "unlockSecret": "test-unlock"
                }))
                .unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{create:?}");
    let run_id = create["runId"].as_str().unwrap().to_string();
    let stream_token = create["streamToken"].as_str().unwrap().to_string();
    assert!(stream_token.len() >= 32);

    let persisted = wait_for_run(db, &run_id, |persisted| {
        matches!(
            persisted.checkpoint.status,
            RuntimeStatus::Completed | RuntimeStatus::Failed | RuntimeStatus::Aborted
        )
    })
    .await;
    assert_eq!(persisted.stream_token, stream_token);
    (run_id, stream_token)
}

fn assert_body_omits(body: &Value, fragments: &[&str]) {
    let text = body.to_string();
    for fragment in fragments {
        assert!(
            !text.contains(fragment),
            "response body {text} should not contain {fragment}"
        );
    }
}

#[tokio::test]
async fn exposes_health() {
    let (_temp, router) = test_router().await;
    let (status, health) = json_response(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["ok"], true);
}

#[tokio::test]
async fn create_run_with_default_security_reports_unlock_not_configured() {
    let (_temp, router, _db) = test_router_with_security(SecurityConfig::default()).await;

    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": echo_workflow() })).unwrap(),
            ))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "unlock_not_configured");
    let message = body["error"].as_str().unwrap();
    assert!(message.contains("SILVERBOND_UNLOCK_PASSWORD_HASH"));
    assert!(message.contains("SILVERBOND_AGENT_USER"));
}

#[tokio::test]
async fn streaming_routes_allow_same_origin_sse_and_require_ws_origin() {
    let (_temp, router, db) = test_router_with_db().await;
    let (run_id, stream_token) = create_terminal_echo_run(&router, &db).await;

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/stream"))
            .header("X-Stream-Token", stream_token.as_str())
            .header(SEC_FETCH_SITE, "same-origin")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/stream"))
            .header("X-Stream-Token", stream_token.as_str())
            .header(SEC_FETCH_SITE, "cross-site")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/stream"))
            .header("X-Stream-Token", stream_token.as_str())
            .header("origin", "https://evil.com")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/stream"))
            .header("X-Stream-Token", stream_token.as_str())
            .header("origin", "http://127.0.0.1:3333")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/runs/missing/panes/active/stream")
            .header("connection", "upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/runs/missing/panes/active/stream")
            .header("origin", "https://evil.com")
            .header("connection", "upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn run_stream_requires_matching_stream_token() {
    let (_temp, router, db) = test_router_with_db().await;
    let (run_id, stream_token) = create_terminal_echo_run(&router, &db).await;

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/stream"))
            .header(SEC_FETCH_SITE, "same-origin")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/stream"))
            .header("X-Stream-Token", "wrong-token")
            .header(SEC_FETCH_SITE, "same-origin")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let status = response_status(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/stream"))
            .header("X-Stream-Token", stream_token)
            .header(SEC_FETCH_SITE, "same-origin")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mutating_run_control_routes_reject_cross_site_requests() {
    let (_temp, router) = test_router().await;

    let status = response_status(
        &router,
        Request::builder()
            .method("POST")
            .uri("/api/runs/missing/abort")
            .header(SEC_FETCH_SITE, "cross-site")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let status = response_status(
        &router,
        Request::builder()
            .method("POST")
            .uri("/api/runs/missing/restart-from/node-1")
            .header("origin", "https://evil.com")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn run_control_routes_return_typed_client_errors() {
    let (_temp, router, db) = test_router_with_db().await;
    let missing_run_id = "run_secret_missing";

    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!("/api/runs/{missing_run_id}/resume"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "run not found");
    assert_body_omits(&body, &[missing_run_id]);

    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!(
                "/api/runs/{missing_run_id}/restart-from/missing-node"
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "run not found");
    assert_body_omits(&body, &[missing_run_id, "missing-node"]);

    let stale_session_id = "session_secret_stale";
    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!("/api/runs/{missing_run_id}/respond-interaction"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "sessionId": stale_session_id,
                    "response": "continue"
                }))
                .unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "no active interaction for this run");
    assert_body_omits(&body, &[missing_run_id, stale_session_id]);

    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!("/api/runs/{missing_run_id}/abort"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);

    let approval_workflow = json!({
        "version": 4,
        "name": "Approval Only",
        "goal": "Approve",
        "cwd": "",
        "useOrchestrator": false,
        "entryNodeId": "a1",
        "variables": [],
        "limits": { "maxTotalSteps": 5, "maxVisitsPerNode": 5 },
        "nodes": [
            {
                "id": "a1",
                "name": "Approval",
                "prompt": "Approve?",
                "kind": { "type": "approval" }
            }
        ],
        "edges": []
    });
    let (status, create) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": approval_workflow })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{create:?}");
    let active_run_id = create["runId"].as_str().unwrap().to_string();
    wait_for_run(&db, &active_run_id, |persisted| {
        persisted.checkpoint.pending_approval.is_some()
    })
    .await;

    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!("/api/runs/{active_run_id}/respond-interaction"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "sessionId": stale_session_id,
                    "response": "continue"
                }))
                .unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "interaction session is stale");
    assert_body_omits(&body, &[&active_run_id, stale_session_id]);

    let echo_workflow = json!({
        "version": 4,
        "name": "Echo Finish",
        "goal": "Finish",
        "cwd": "",
        "useOrchestrator": false,
        "entryNodeId": "n1",
        "variables": [],
        "limits": { "maxTotalSteps": 5, "maxVisitsPerNode": 5 },
        "nodes": [
            {
                "id": "n1",
                "name": "Echo",
                "agent": "echo",
                "prompt": "done",
                "kind": { "type": "task" }
            }
        ],
        "edges": []
    });
    let (status, create) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "workflow": echo_workflow,
                    "unlockSecret": "test-unlock"
                }))
                .unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{create:?}");
    let terminal_run_id = create["runId"].as_str().unwrap().to_string();
    wait_for_run(&db, &terminal_run_id, |persisted| {
        matches!(
            persisted.checkpoint.status,
            RuntimeStatus::Completed | RuntimeStatus::Failed | RuntimeStatus::Aborted
        )
    })
    .await;

    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!("/api/runs/{terminal_run_id}/resume"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "run is in terminal state");
    assert_body_omits(&body, &[&terminal_run_id]);

    let missing_node_id = "secret-node-target";
    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!(
                "/api/runs/{terminal_run_id}/restart-from/{missing_node_id}"
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "node not found in workflow");
    assert_body_omits(&body, &[&terminal_run_id, missing_node_id]);
}

#[tokio::test]
async fn internal_http_errors_use_fixed_client_body() {
    let temp = TempDir::new().unwrap();
    let paths = AppPaths::from_root(temp.path());
    std::fs::write(temp.path().join(".silverbond"), b"not a directory").unwrap();
    let db = Database::new(paths.database_path.clone());
    let state = AppState {
        paths: paths.clone(),
        workflows: WorkflowStore::new(paths.workflows_dir.clone()),
        templates: TemplateStore::new(paths.templates_dir.clone()),
        runtime: RuntimeContext::new(db),
        pane_streams: PaneStreamRegistry::default(),
        security: SecurityConfig::default(),
    };
    let router = api::router(state);
    let run_id = "run_secret_internal";
    let stream_token = "stream_secret_internal";

    let (status, body) = json_response(
        &router,
        Request::builder()
            .method("GET")
            .uri(format!("/api/runs/{run_id}/events"))
            .header("X-Stream-Token", stream_token)
            .header(SEC_FETCH_SITE, "same-origin")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "internal error");
    assert_body_omits(
        &body,
        &[
            run_id,
            stream_token,
            paths.database_path.to_string_lossy().as_ref(),
        ],
    );
}

#[tokio::test]
async fn validates_and_saves_workflows() {
    let (_temp, router) = test_router().await;
    let workflow = json!({
        "version": 4,
        "name": "Example",
        "goal": "Test",
        "cwd": "",
        "useOrchestrator": false,
        "entryNodeId": "n1",
        "variables": [],
        "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
        "nodes": [
            {
                "id": "n1",
                "name": "Node 1",
                "agent": "claude",
                "prompt": "Say hi",
                "kind": { "type": "task" }
            }
        ],
        "edges": [],
        "ui": {
            "canvas": {
                "viewport": { "x": 10.0, "y": 20.0, "zoom": 1.25 },
                "nodes": {
                    "n1": { "x": 128.0, "y": 256.0 }
                }
            }
        }
    });

    let (status, validation) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/validate-workflow")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": workflow.clone() })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(validation["workflow"]["version"], 4);
    assert_eq!(
        validation["workflow"]["ui"]["canvas"]["nodes"]["n1"]["x"],
        128.0
    );

    let (status, save) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/workflows")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "name": "example-workflow",
                    "workflow": workflow
                }))
                .unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(save["success"], true);

    let (status, list) = json_response(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/workflows")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["workflow"]["version"], 4);
    assert_eq!(
        list[0]["workflow"]["ui"]["canvas"]["viewport"]["zoom"],
        1.25
    );
}

#[tokio::test]
async fn rejects_legacy_workflow_payloads() {
    let (_temp, router) = test_router().await;
    let legacy_workflow = json!({
        "_version": 2,
        "steps": []
    });

    let (status, validation) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/validate-workflow")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": legacy_workflow.clone() })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        validation["error"]
            .as_str()
            .unwrap()
            .contains("workflow version is required")
    );

    let (status, run) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": legacy_workflow })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(run["error"].is_string());
}

#[tokio::test]
async fn test_node_accepts_v3_task_node() {
    let (temp, router) = test_router().await;
    let task_node = json!({
        "id": "preview-task",
        "name": "Preview Task",
        "agent": "echo",
        "prompt": "Preview task",
        "kind": { "type": "task" }
    });

    let (status, preview) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/test-node")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "node": task_node,
                    "cwd": temp.path().to_string_lossy()
                }))
                .unwrap(),
            ))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{preview:?}");
    assert_eq!(preview["success"], true);
    assert_eq!(preview["agent"], "echo");
    assert_eq!(preview["resolvedPrompt"], "Preview task");
}

#[tokio::test]
async fn test_node_rejects_non_preview_node_payloads() {
    let (_temp, router) = test_router().await;
    let split_node = json!({
        "id": "split-node",
        "name": "Split Node",
        "prompt": "",
        "kind": { "type": "split" }
    });

    let (status, rejection) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/test-node")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "node": split_node })).unwrap(),
            ))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        rejection["error"]
            .as_str()
            .unwrap()
            .contains("task or approval kind types")
    );

    let workflow_payload = json!({
        "version": 4,
        "name": "Workflow Payload",
        "entryNodeId": "preview-task",
        "nodes": [
            {
                "id": "preview-task",
                "name": "Preview Task",
                "agent": "echo",
                "prompt": "Preview task",
                "kind": { "type": "task" }
            }
        ],
        "edges": []
    });

    let (status, rejection) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/test-node")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "node": workflow_payload })).unwrap(),
            ))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        rejection["error"]
            .as_str()
            .unwrap()
            .contains("provide a single node")
    );
}

#[tokio::test]
async fn validates_workflow_with_agent_config() {
    let (_temp, router) = test_router().await;
    let workflow = json!({
        "version": 4,
        "name": "Agent Config Test",
        "goal": "Test agent configuration",
        "cwd": "/work",
        "useOrchestrator": false,
        "entryNodeId": "n1",
        "variables": [],
        "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
        "agentDefaults": {
            "claude": {
                "model": "sonnet",
                "maxTurns": 5,
                "accessMode": "edit",
                "toolToggles": { "webSearch": false }
            }
        },
        "nodes": [
            {
                "id": "n1",
                "name": "Analysis",
                "agent": "claude",
                "prompt": "Analyze code",
                "kind": {
                    "type": "task",
                    "agentConfig": {
                        "maxBudgetUsd": 1.5,
                        "systemPrompt": "Be thorough."
                    }
                },
                "cwd": "/project"
            },
            {
                "id": "n2",
                "name": "Follow-up",
                "agent": "claude",
                "prompt": "Continue analysis",
                "continueSessionFrom": "n1",
                "kind": { "type": "task" }
            }
        ],
        "edges": [
            { "id": "e1", "from": "n1", "to": "n2", "outcome": "success" }
        ]
    });

    let (status, validation) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/validate-workflow")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": workflow })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Verify agent defaults preserved in round-trip
    let w = &validation["workflow"];
    assert_eq!(w["agentDefaults"]["claude"]["model"], "sonnet");
    assert_eq!(w["agentDefaults"]["claude"]["maxTurns"], 5);
    assert_eq!(w["agentDefaults"]["claude"]["accessMode"], "edit");

    // Verify node config preserved
    assert_eq!(w["nodes"][0]["kind"]["agentConfig"]["maxBudgetUsd"], 1.5);
    assert_eq!(
        w["nodes"][0]["kind"]["agentConfig"]["systemPrompt"],
        "Be thorough."
    );
    assert_eq!(w["nodes"][0]["cwd"], "/project");
    assert_eq!(w["nodes"][1]["continueSessionFrom"], "n1");
}

#[tokio::test]
async fn exposes_capabilities() {
    let (_temp, router) = test_router().await;
    let (status, capabilities) = json_response(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/capabilities")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(capabilities["workflowVersion"], 4);
    assert_eq!(
        capabilities["supportedNodeTypes"],
        json!([
            "task",
            "approval",
            "split",
            "collector",
            "decide",
            "parallel_batch",
            "subflow",
            "call",
            "spawn",
            "send",
            "wait",
            "capture",
            "kill",
            "run_agent"
        ])
    );
    assert_eq!(capabilities["features"]["split"], true);
    assert_eq!(capabilities["features"]["collector"], true);
    assert_eq!(capabilities["features"]["subflow"], true);
    assert_eq!(capabilities["features"]["runAs"], true);

    // Verify capability flags match expected values for claude
    let claude_caps = &capabilities["agents"]["claude"]["capabilities"];
    assert_eq!(claude_caps["workerExecution"], true);
    assert_eq!(claude_caps["structuredOutput"], true);
    assert_eq!(claude_caps["nativeJsonSchema"], true);
    assert_eq!(claude_caps["sessionReuse"], true);
    assert_eq!(claude_caps["modelSelection"], true);
    assert_eq!(claude_caps["reasoningConfig"], false);
    assert_eq!(claude_caps["systemPrompt"], true);
    assert_eq!(claude_caps["budgetLimit"], true);
    assert_eq!(claude_caps["turnLimit"], true);
    assert_eq!(claude_caps["costReporting"], true);
    assert_eq!(claude_caps["toolAllowlist"], true);
    assert_eq!(claude_caps["webSearch"], true);

    // Gemini was removed (replaced by the Antigravity CLI) — it must not appear.
    assert!(capabilities["agents"].get("gemini").is_none());

    // Verify Cursor is listed (registry-driven worker agent).
    assert!(capabilities["agents"]["cursor"].is_object());
    let cursor_caps = &capabilities["agents"]["cursor"]["capabilities"];
    assert_eq!(cursor_caps["workerExecution"], true);

    // Verify Antigravity (agy) is listed.
    assert!(capabilities["agents"]["agy"].is_object());
    let agy_caps = &capabilities["agents"]["agy"]["capabilities"];
    assert_eq!(agy_caps["workerExecution"], true);
}

#[tokio::test]
async fn lists_templates_without_failing_on_invalid_files() {
    let (temp, router) = test_router().await;
    std::fs::write(
        temp.path().join("templates").join("valid.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 4,
            "name": "Valid Template",
            "description": "A valid workflow template",
            "goal": "goal",
            "cwd": "",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [
                {
                    "id": "n1",
                    "name": "Node 1",
                    "agent": "claude",
                    "prompt": "Say hi",
                    "kind": { "type": "task" }
                }
            ],
            "edges": []
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("templates").join("invalid.json"),
        br#"{ "_version": 2, "steps": [] }"#,
    )
    .unwrap();

    let (status, list) = json_response(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/templates")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "Valid Template");
    assert_eq!(list[0]["workflow"]["version"], 4);
}

#[tokio::test]
async fn creates_and_approves_runs() {
    let (_temp, router) = test_router().await;
    let approval_workflow = json!({
        "version": 4,
        "name": "Approval Only",
        "goal": "Approve",
        "cwd": "",
        "useOrchestrator": false,
        "entryNodeId": "a1",
        "variables": [],
        "limits": { "maxTotalSteps": 5, "maxVisitsPerNode": 5 },
        "nodes": [
            {
                "id": "a1",
                "name": "Approval",
                "prompt": "Approve?",
                "kind": { "type": "approval" }
            }
        ],
        "edges": []
    });

    let (status, create) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri("/api/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": approval_workflow })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let run_id = create["runId"].as_str().unwrap().to_string();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (status, interrupted) = json_response(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/interrupted-runs")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        interrupted
            .as_array()
            .unwrap()
            .iter()
            .all(|run| run["runId"] != run_id)
    );

    let (status, approved) = json_response(
        &router,
        Request::builder()
            .method("POST")
            .header(SEC_FETCH_SITE, "same-origin")
            .uri(format!("/api/runs/{}/approve", run_id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "approved": true, "userInput": "yes" })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(approved["success"], true);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (status, logs) = json_response(
        &router,
        Request::builder()
            .method("GET")
            .uri("/api/logs")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(logs.as_array().unwrap().len(), 1);
    assert!(logs[0]["nodeExecutionCount"].is_number());
}
