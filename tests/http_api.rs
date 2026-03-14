use silverbond::{
    api,
    app::{AppPaths, AppState},
    runtime::RuntimeContext,
    storage::{Database, TemplateStore, WorkflowStore},
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

async fn test_router() -> (TempDir, Router) {
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
        runtime: RuntimeContext::new(db),
    };
    (temp, api::router(state))
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
async fn validates_and_saves_workflows() {
    let (_temp, router) = test_router().await;
    let workflow = json!({
        "version": 3,
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
                "type": "task",
                "agent": "claude",
                "prompt": "Say hi"
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
            .uri("/api/validate-workflow")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "workflow": workflow.clone() })).unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(validation["workflow"]["version"], 3);
    assert_eq!(
        validation["workflow"]["ui"]["canvas"]["nodes"]["n1"]["x"],
        128.0
    );

    let (status, save) = json_response(
        &router,
        Request::builder()
            .method("POST")
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
    assert_eq!(list[0]["workflow"]["version"], 3);
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
async fn validates_workflow_with_agent_config() {
    let (_temp, router) = test_router().await;
    let workflow = json!({
        "version": 3,
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
                "type": "task",
                "agent": "claude",
                "prompt": "Analyze code",
                "agentConfig": {
                    "maxBudgetUsd": 1.5,
                    "systemPrompt": "Be thorough."
                },
                "cwd": "/project"
            },
            {
                "id": "n2",
                "name": "Follow-up",
                "type": "task",
                "agent": "claude",
                "prompt": "Continue analysis",
                "continueSessionFrom": "n1"
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
    assert_eq!(w["nodes"][0]["agentConfig"]["maxBudgetUsd"], 1.5);
    assert_eq!(w["nodes"][0]["agentConfig"]["systemPrompt"], "Be thorough.");
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
    assert_eq!(capabilities["workflowVersion"], 3);
    assert_eq!(capabilities["supportedNodeTypes"], json!(["task", "approval", "split", "collector"]));
    assert_eq!(capabilities["features"]["split"], true);
    assert_eq!(capabilities["features"]["collector"], true);

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

    // Verify Gemini is listed
    assert!(capabilities["agents"]["gemini"].is_object());
    let gemini_caps = &capabilities["agents"]["gemini"]["capabilities"];
    assert_eq!(gemini_caps["nativeJsonSchema"], false);
    assert_eq!(gemini_caps["reasoningConfig"], true);
}

#[tokio::test]
async fn lists_templates_without_failing_on_invalid_files() {
    let (temp, router) = test_router().await;
    std::fs::write(
        temp.path().join("templates").join("valid.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 3,
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
                    "type": "task",
                    "agent": "claude",
                    "prompt": "Say hi"
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
    assert_eq!(list[0]["workflow"]["version"], 3);
}

#[tokio::test]
async fn creates_and_approves_runs() {
    let (_temp, router) = test_router().await;
    let approval_workflow = json!({
        "version": 3,
        "name": "Approval Only",
        "goal": "Approve",
        "cwd": "",
        "useOrchestrator": false,
        "entryNodeId": "a1",
        "variables": [],
        "limits": { "maxTotalSteps": 5, "maxVisitsPerNode": 5 },
        "nodes": [
            { "id": "a1", "name": "Approval", "type": "approval", "prompt": "Approve?" }
        ],
        "edges": []
    });

    let (status, create) = json_response(
        &router,
        Request::builder()
            .method("POST")
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
            .any(|run| run["runId"] == run_id)
    );

    let (status, approved) = json_response(
        &router,
        Request::builder()
            .method("POST")
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
