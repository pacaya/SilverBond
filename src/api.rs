use std::collections::BTreeMap;

use anyhow::Context;
use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures::Stream;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    app::AppState,
    model::{WorkflowNode, WorkflowNodeType, normalize_workflow_value, validate_workflow},
    runtime::{NodeTestContext, available_agents, check_cli, run_node_preview},
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/capabilities", get(capabilities))
        .route("/api/workflows", get(list_workflows).post(save_workflow))
        .route(
            "/api/workflows/{name}",
            get(get_workflow).delete(delete_workflow),
        )
        .route("/api/validate-workflow", post(validate_workflow_route))
        .route("/api/templates", get(list_templates))
        .route("/api/test-node", post(test_node))
        .route("/api/runs", post(create_run))
        .route("/api/runs/{run_id}/stream", get(stream_run))
        .route("/api/runs/{run_id}/events", get(run_events))
        .route("/api/runs/{run_id}/approve", post(approve_run))
        .route("/api/runs/{run_id}/abort", post(abort_run))
        .route("/api/runs/{run_id}/resume", post(resume_run))
        .route(
            "/api/runs/{run_id}/restart-from/{node_id}",
            post(restart_run),
        )
        .route("/api/runs/{run_id}/dismiss", post(dismiss_run))
        .route("/api/interrupted-runs", get(interrupted_runs))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/{id}/history", get(session_history))
        .route("/api/logs", get(list_logs))
        .route("/api/logs/{id}", get(get_log).delete(delete_log))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn capabilities() -> Result<Json<Value>, ApiError> {
    let mut agents = serde_json::Map::new();
    for spec in available_agents() {
        let (available, path) = check_cli(spec.name).await?;
        agents.insert(
            spec.name.to_string(),
            json!({
                "available": available,
                "path": path,
                "capabilities": spec.capabilities,
            }),
        );
    }
    Ok(Json(json!({
        "workflowVersion": 3,
        "supportedNodeTypes": ["task", "approval", "split", "collector"],
        "supportedEdgeOutcomes": ["success", "reject", "branch", "loop_continue", "loop_exit"],
        "agents": agents,
        "features": {
            "split": true,
            "collector": true,
        }
    })))
}

async fn list_workflows(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(state.workflows.list().await?)?))
}

async fn get_workflow(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let workflow = state.workflows.get(&name).await?;
    match workflow {
        Some(workflow) => Ok(Json(serde_json::to_value(workflow)?)),
        None => Err(ApiError::status(StatusCode::NOT_FOUND, "Not found")),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveWorkflowRequest {
    name: String,
    workflow: Value,
}

async fn save_workflow(
    State(state): State<AppState>,
    Json(request): Json<SaveWorkflowRequest>,
) -> Result<Json<Value>, ApiError> {
    let normalized = normalize_workflow_value(request.workflow)
        .map_err(|error| ApiError::status(StatusCode::BAD_REQUEST, error.to_string()))?;
    let saved_name = state.workflows.save(&request.name, normalized).await?;
    Ok(Json(json!({ "success": true, "name": saved_name })))
}

async fn delete_workflow(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state.workflows.delete(&name).await?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowPayloadRequest {
    workflow: Value,
}

async fn validate_workflow_route(
    Json(request): Json<WorkflowPayloadRequest>,
) -> Result<Json<Value>, ApiError> {
    let normalized = normalize_workflow_value(request.workflow)
        .map_err(|error| ApiError::status(StatusCode::BAD_REQUEST, error.to_string()))?;
    let mut result = validate_workflow(normalized.workflow);
    result.notices = normalized.notices;
    Ok(Json(serde_json::to_value(result)?))
}

async fn list_templates(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(state.templates.list().await?)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestStepRequest {
    node: Option<Value>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    mock_context: Option<NodeTestContext>,
}

async fn test_node(Json(request): Json<TestStepRequest>) -> Result<Json<Value>, ApiError> {
    let node_value = request
        .node
        .context("node is required")
        .map_err(|error| ApiError::status(StatusCode::BAD_REQUEST, error.to_string()))?;
    let node = node_from_value(node_value)
        .map_err(|error| ApiError::status(StatusCode::BAD_REQUEST, error.to_string()))?;
    if node.node_type != WorkflowNodeType::Task {
        return Err(ApiError::status(
            StatusCode::BAD_REQUEST,
            "Only task nodes can be tested",
        ));
    }
    let preview = run_node_preview(
        &node,
        request.cwd.as_deref().unwrap_or_default(),
        request.mock_context.unwrap_or_default(),
    )
    .await?;
    Ok(Json(serde_json::to_value(preview)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRunRequest {
    workflow: Value,
    #[serde(default)]
    variable_overrides: BTreeMap<String, String>,
    #[serde(default)]
    start_node_id: Option<String>,
}

async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<Value>, ApiError> {
    let normalized = normalize_workflow_value(request.workflow)
        .map_err(|error| ApiError::status(StatusCode::BAD_REQUEST, error.to_string()))?;
    let validation = validate_workflow(normalized.workflow.clone());
    let errors = validation
        .issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .cloned()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(ApiError::json(
            StatusCode::BAD_REQUEST,
            json!({
                "error": "Validation failed",
                "details": errors,
            }),
        ));
    }
    let run_id = state
        .runtime
        .start_run(
            normalized.workflow,
            request.variable_overrides,
            request.start_node_id,
        )
        .await?;
    Ok(Json(json!({ "success": true, "runId": run_id })))
}

async fn stream_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let replay = state.runtime.db.list_events(&run_id).await?;
    let receiver = state.runtime.registry.subscribe(&run_id).await;
    let stream = stream! {
        let mut done_seen = false;
        for event in replay {
            if event.kind == "done" {
                done_seen = true;
            }
            yield Ok(Event::default().data(serde_json::to_string(&event).unwrap_or_default()));
        }

        if done_seen {
            return;
        }

        if let Some(mut receiver) = receiver {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let is_done = event.kind == "done";
                        yield Ok(Event::default().data(serde_json::to_string(&event).unwrap_or_default()));
                        if is_done {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(
        state.runtime.db.list_events(&run_id).await?,
    )?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalRequest {
    approved: bool,
    #[serde(default)]
    user_input: String,
}

async fn approve_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<ApprovalRequest>,
) -> Result<Json<Value>, ApiError> {
    state
        .runtime
        .approve_run(&run_id, request.approved, request.user_input)
        .await?;
    Ok(Json(json!({ "success": true })))
}

async fn abort_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state.runtime.abort_run(&run_id).await?;
    Ok(Json(json!({ "success": true })))
}

async fn resume_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state.runtime.resume_run(&run_id).await?;
    Ok(Json(json!({ "success": true, "runId": run_id })))
}

async fn restart_run(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let new_run_id = state.runtime.restart_from(&run_id, &node_id).await?;
    Ok(Json(json!({ "success": true, "runId": new_run_id })))
}

async fn dismiss_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .runtime
        .db
        .mark_run_status(
            &run_id,
            crate::runtime::RuntimeStatus::Aborted,
            Some("aborted".to_string()),
        )
        .await?;
    Ok(Json(json!({ "success": true })))
}

async fn interrupted_runs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(
        state.runtime.db.list_interrupted_runs().await?,
    )?))
}

async fn list_logs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(
        state.runtime.db.list_logs().await?,
    )?))
}

async fn get_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let log = state.runtime.db.get_log(&id).await?;
    match log {
        Some(log) => Ok(Json(log)),
        None => Err(ApiError::status(StatusCode::NOT_FOUND, "Not found")),
    }
}

async fn delete_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state.runtime.db.delete_log(&id).await?;
    Ok(Json(json!({ "success": true })))
}

async fn list_sessions(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sessions = state.runtime.session_manager.list_sessions().await;
    Ok(Json(serde_json::to_value(sessions)?))
}

async fn session_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let history = state
        .runtime
        .session_manager
        .get_history(&id)
        .await
        .map_err(|_| ApiError::status(StatusCode::NOT_FOUND, "Session not found"))?;
    Ok(Json(serde_json::to_value(history)?))
}

fn node_from_value(value: Value) -> anyhow::Result<WorkflowNode> {
    if value.get("version").is_some() || value.get("entryNodeId").is_some() {
        anyhow::bail!("workflow payload is not valid for node testing");
    }
    if value
        .get("type")
        .and_then(Value::as_str)
        .map(|ty| matches!(ty, "task" | "approval"))
        .unwrap_or(false)
    {
        return Ok(serde_json::from_value(value)?);
    }
    anyhow::bail!("node payload must use canonical v3 task or approval types")
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    body: Value,
}

impl ApiError {
    fn status(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": message.into() }),
        }
    }

    fn json(status: StatusCode, body: Value) -> Self {
        Self { status, body }
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        let error = value.into();
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: json!({ "error": error.to_string() }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.body)).into_response()
    }
}
