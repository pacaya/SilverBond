use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use anyhow::Context;
use async_stream::stream;
use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{SinkExt, Stream, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tmux_tools_core::stream::{CaptureAnsiOpts, capture_ansi, stream_pane};
use tokio::{io::AsyncReadExt, time::Instant};

use crate::{
    app::AppState,
    model::{
        WorkflowNode, WorkflowNodeType, WorkflowV3, normalize_workflow_value, validate_workflow,
    },
    runtime::{
        NodeTestContext, RuntimeCheckpoint, RuntimeStatus, available_agents, check_cli,
        run_node_preview,
    },
};

const PANE_STREAM_HEARTBEAT: Duration = Duration::from_secs(5);
const PANE_STREAM_SEND_TIMEOUT: Duration = Duration::from_secs(2);

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
        .route(
            "/api/runs/{run_id}/panes/{pane}/stream",
            get(pane_stream_ws),
        )
        .route("/api/runs/{run_id}/events", get(run_events))
        .route("/api/runs/{run_id}/approve", post(approve_run))
        .route(
            "/api/runs/{run_id}/respond-interaction",
            post(respond_interaction),
        )
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
        let (available, path) = check_cli(&spec.binary).await?;
        agents.insert(
            spec.name.clone(),
            json!({
                "available": available,
                "path": path,
                "binary": spec.binary,
                "capabilities": spec.capabilities,
            }),
        );
    }
    Ok(Json(json!({
        "workflowVersion": 3,
        "supportedNodeTypes": ["task", "approval", "split", "collector", "decide", "parallel_batch", "subflow", "call", "spawn", "send", "wait", "capture", "kill", "run_agent"],
        "supportedEdgeOutcomes": ["success", "reject", "branch", "loop_continue", "loop_exit"],
        "agents": agents,
        "features": {
            "split": true,
            "collector": true,
            "subflow": true,
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
    State(state): State<AppState>,
    Json(request): Json<WorkflowPayloadRequest>,
) -> Result<Json<Value>, ApiError> {
    let mut normalized = normalize_workflow_value(request.workflow)
        .map_err(|error| ApiError::status(StatusCode::BAD_REQUEST, error.to_string()))?;
    hydrate_saved_subflows(&state, &mut normalized.workflow).await?;
    let mut result = validate_workflow(normalized.workflow);
    result.notices = normalized.notices;
    Ok(Json(serde_json::to_value(result)?))
}

async fn hydrate_saved_subflows(
    state: &AppState,
    workflow: &mut WorkflowV3,
) -> Result<(), ApiError> {
    let mut loaded = workflow.subflows.keys().cloned().collect::<BTreeSet<_>>();
    loop {
        let needed = collect_referenced_subflows(workflow)
            .into_iter()
            .filter(|name| !loaded.contains(name))
            .collect::<Vec<_>>();
        if needed.is_empty() {
            return Ok(());
        }

        for name in needed {
            loaded.insert(name.clone());
            if let Some(stored) = state.workflows.get(&name).await? {
                workflow.subflows.insert(name, Box::new(stored.workflow));
            }
        }
    }
}

fn collect_referenced_subflows(workflow: &WorkflowV3) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_referenced_subflows_in_workflow(workflow, &mut names);
    for subflow in workflow.subflows.values() {
        collect_referenced_subflows_in_workflow(subflow, &mut names);
    }
    names
}

fn collect_referenced_subflows_in_workflow(workflow: &WorkflowV3, names: &mut BTreeSet<String>) {
    for node in &workflow.nodes {
        if !matches!(
            node.node_type,
            WorkflowNodeType::Subflow | WorkflowNodeType::Call
        ) {
            continue;
        }
        if let Some(name) = node
            .subflow_config
            .as_ref()
            .map(|config| config.workflow_name.trim())
            .filter(|name| !name.is_empty())
        {
            names.insert(name.to_string());
        }
    }
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
    let mut normalized = normalize_workflow_value(request.workflow)
        .map_err(|error| ApiError::status(StatusCode::BAD_REQUEST, error.to_string()))?;
    hydrate_saved_subflows(&state, &mut normalized.workflow).await?;
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

pub async fn pane_stream_ws(
    State(state): State<AppState>,
    Path((run_id, pane)): Path<(String, String)>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        let log_run_id = run_id.clone();
        let log_pane = pane.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = pane_stream_socket(socket, state, run_id, pane).await {
                tracing::warn!(
                    run_id = %log_run_id,
                    pane = %log_pane,
                    error = %error,
                    "pane websocket stream ended with error"
                );
            }
        });
        let _ = task.await;
    })
}

async fn pane_stream_socket(
    mut socket: WebSocket,
    state: AppState,
    run_id: String,
    pane: String,
) -> anyhow::Result<()> {
    let pane_target = match resolve_run_pane_target(&state, &run_id, &pane).await {
        Ok(target) => target,
        Err(error) => {
            send_socket_error_and_close(&mut socket, 0, &error.to_string()).await;
            return Ok(());
        }
    };

    let (mut sender, mut receiver) = socket.split();
    let mut next_seq = 0_u64;
    send_snapshot(&mut sender, &pane_target, &mut next_seq).await?;

    let mut pane_reader = match stream_pane(&pane_target).await {
        Ok(reader) => reader,
        Err(error) => {
            let message = format!("pane stream unavailable: {error}");
            let _ = send_error_frame(&mut sender, &mut next_seq, &message).await;
            tracing::warn!(
                run_id = %run_id,
                pane = %pane,
                pane_target = %pane_target,
                error = %error,
                "failed to start pane stream"
            );
            return Ok(());
        }
    };

    let mut heartbeat = Box::pin(tokio::time::sleep(PANE_STREAM_HEARTBEAT));
    let mut buffer = [0_u8; 8192];
    loop {
        tokio::select! {
            client_message = receiver.next() => {
                match client_message {
                    Some(Ok(message)) if is_resync_request(&message) => {
                        send_snapshot(&mut sender, &pane_target, &mut next_seq).await?;
                        heartbeat.as_mut().reset(Instant::now() + PANE_STREAM_HEARTBEAT);
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::debug!(
                            run_id = %run_id,
                            pane = %pane,
                            pane_target = %pane_target,
                            error = %error,
                            "pane websocket receive failed"
                        );
                        break;
                    }
                }
            }
            read_result = pane_reader.read(&mut buffer) => {
                match read_result {
                    Ok(0) => {
                        tracing::debug!(
                            run_id = %run_id,
                            pane = %pane,
                            pane_target = %pane_target,
                            "pane stream reached EOF"
                        );
                        break;
                    }
                    Ok(bytes_read) => {
                        match send_data_frame(&mut sender, &buffer[..bytes_read], &mut next_seq).await {
                            Ok(()) => {
                                heartbeat.as_mut().reset(Instant::now() + PANE_STREAM_HEARTBEAT);
                            }
                            Err(PaneWsSendError::Backpressure) => {
                                tracing::warn!(
                                    run_id = %run_id,
                                    pane = %pane,
                                    pane_target = %pane_target,
                                    "pane websocket send stalled; sending snapshot resync"
                                );
                                send_snapshot(&mut sender, &pane_target, &mut next_seq).await?;
                                heartbeat.as_mut().reset(Instant::now() + PANE_STREAM_HEARTBEAT);
                            }
                            Err(error) => {
                                tracing::debug!(
                                    run_id = %run_id,
                                    pane = %pane,
                                    pane_target = %pane_target,
                                    error = %error,
                                    "pane websocket send failed"
                                );
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            run_id = %run_id,
                            pane = %pane,
                            pane_target = %pane_target,
                            error = %error,
                            "pane stream read failed"
                        );
                        break;
                    }
                }
            }
            _ = &mut heartbeat => {
                match send_heartbeat(&mut sender, current_seq(next_seq)).await {
                    Ok(()) => heartbeat.as_mut().reset(Instant::now() + PANE_STREAM_HEARTBEAT),
                    Err(error) => {
                        tracing::debug!(
                            run_id = %run_id,
                            pane = %pane,
                            pane_target = %pane_target,
                            error = %error,
                            "pane websocket heartbeat failed"
                        );
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn resolve_run_pane_target(
    state: &AppState,
    run_id: &str,
    pane: &str,
) -> anyhow::Result<String> {
    if let Some(target) = state
        .runtime
        .registry
        .resolve_active_pane(run_id, pane)
        .await
    {
        return Ok(target);
    }

    let persisted = state
        .runtime
        .db
        .get_run(run_id)
        .await?
        .context("run not found")?;
    let candidates = pane_candidates(&persisted.checkpoint);
    if let Some(target) = match_pane_candidate(
        pane,
        &candidates,
        persisted.checkpoint.current_node_id.as_deref(),
    ) {
        return Ok(target);
    }

    if matches!(
        persisted.checkpoint.status,
        RuntimeStatus::Running | RuntimeStatus::Paused
    ) {
        anyhow::bail!("run has no active pane matching {pane}");
    }
    anyhow::bail!("run has no pane matching {pane}");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneCandidate {
    key: String,
    target: String,
}

fn pane_candidates(checkpoint: &RuntimeCheckpoint) -> Vec<PaneCandidate> {
    let mut candidates = Vec::new();
    for (node_id, result) in &checkpoint.all_results {
        push_pane_candidate(
            &mut candidates,
            node_id,
            result.metadata.agent_session_id.as_deref(),
        );
        if let Some(value) = &result.parsed_output {
            push_pane_candidate(&mut candidates, node_id, pane_target_from_value(value));
        }
        if let Ok(value) = serde_json::from_str::<Value>(&result.output) {
            push_pane_candidate(&mut candidates, node_id, pane_target_from_value(&value));
        }
    }

    for execution in &checkpoint.execution_log.node_executions {
        push_pane_candidate(
            &mut candidates,
            &execution.node_id,
            execution.metadata.agent_session_id.as_deref(),
        );
    }

    candidates
}

fn push_pane_candidate(candidates: &mut Vec<PaneCandidate>, key: &str, target: Option<&str>) {
    let Some(target) = target.map(str::trim).filter(|target| !target.is_empty()) else {
        return;
    };
    if candidates
        .iter()
        .any(|candidate| candidate.key == key && candidate.target == target)
    {
        return;
    }
    candidates.push(PaneCandidate {
        key: key.to_string(),
        target: target.to_string(),
    });
}

fn pane_target_from_value(value: &Value) -> Option<&str> {
    ["paneId", "pane_id", "target"]
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
}

fn match_pane_candidate(
    pane: &str,
    candidates: &[PaneCandidate],
    current_node_id: Option<&str>,
) -> Option<String> {
    if matches!(pane, "active" | "current") {
        if let Some(current_node_id) = current_node_id {
            if let Some(candidate) = candidates
                .iter()
                .rev()
                .find(|candidate| candidate.key == current_node_id)
            {
                return Some(candidate.target.clone());
            }
        }
        return match candidates {
            [candidate] => Some(candidate.target.clone()),
            candidates => candidates.last().map(|candidate| candidate.target.clone()),
        };
    }

    candidates
        .iter()
        .rev()
        .find(|candidate| candidate.key == pane || candidate.target == pane)
        .map(|candidate| candidate.target.clone())
}

#[derive(Debug, Serialize)]
struct PaneWsFrame<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

#[derive(Debug, thiserror::Error)]
enum PaneWsSendError {
    #[error("websocket send timed out")]
    Backpressure,
    #[error("websocket send failed: {0}")]
    Send(#[from] axum::Error),
    #[error("websocket frame serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

async fn send_snapshot(
    sender: &mut SplitSink<WebSocket, Message>,
    pane_target: &str,
    next_seq: &mut u64,
) -> anyhow::Result<()> {
    let target = pane_target.to_string();
    let snapshot = tokio::task::spawn_blocking(move || {
        capture_ansi(&target, CaptureAnsiOpts::default()).map(|snapshot| snapshot.into_bytes())
    })
    .await
    .context("capture_ansi task panicked")??;
    let seq = *next_seq;
    let frame = PaneWsFrame {
        kind: "snapshot",
        seq,
        data: Some(BASE64_STANDARD.encode(snapshot)),
        error: None,
    };
    send_frame(sender, &frame)
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    *next_seq = (*next_seq).saturating_add(1);
    Ok(())
}

async fn send_data_frame(
    sender: &mut SplitSink<WebSocket, Message>,
    bytes: &[u8],
    next_seq: &mut u64,
) -> Result<(), PaneWsSendError> {
    let seq = *next_seq;
    let frame = PaneWsFrame {
        kind: "data",
        seq,
        data: Some(BASE64_STANDARD.encode(bytes)),
        error: None,
    };
    send_frame(sender, &frame).await?;
    *next_seq = (*next_seq).saturating_add(1);
    Ok(())
}

async fn send_heartbeat(
    sender: &mut SplitSink<WebSocket, Message>,
    seq: u64,
) -> Result<(), PaneWsSendError> {
    let frame = PaneWsFrame {
        kind: "heartbeat",
        seq,
        data: None,
        error: None,
    };
    send_frame(sender, &frame).await
}

async fn send_error_frame(
    sender: &mut SplitSink<WebSocket, Message>,
    next_seq: &mut u64,
    error: &str,
) -> Result<(), PaneWsSendError> {
    let seq = *next_seq;
    let frame = PaneWsFrame {
        kind: "error",
        seq,
        data: None,
        error: Some(error),
    };
    send_frame(sender, &frame).await?;
    *next_seq = (*next_seq).saturating_add(1);
    Ok(())
}

async fn send_frame(
    sender: &mut SplitSink<WebSocket, Message>,
    frame: &PaneWsFrame<'_>,
) -> Result<(), PaneWsSendError> {
    let payload = serde_json::to_string(frame)?;
    match tokio::time::timeout(
        PANE_STREAM_SEND_TIMEOUT,
        sender.send(Message::Text(payload.into())),
    )
    .await
    {
        Ok(result) => result.map_err(PaneWsSendError::Send),
        Err(_) => Err(PaneWsSendError::Backpressure),
    }
}

async fn send_socket_error_and_close(socket: &mut WebSocket, seq: u64, error: &str) {
    let frame = PaneWsFrame {
        kind: "error",
        seq,
        data: None,
        error: Some(error),
    };
    if let Ok(payload) = serde_json::to_string(&frame) {
        let _ = socket.send(Message::Text(payload.into())).await;
    }
    let _ = socket.send(Message::Close(None)).await;
}

fn is_resync_request(message: &Message) -> bool {
    match message {
        Message::Text(text) => {
            let text = text.as_str().trim();
            text.eq_ignore_ascii_case("resync")
                || serde_json::from_str::<Value>(text)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("type")
                            .and_then(Value::as_str)
                            .map(|kind| matches!(kind, "resync" | "snapshot"))
                    })
                    .unwrap_or(false)
        }
        Message::Binary(bytes) => serde_json::from_slice::<Value>(bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|kind| matches!(kind, "resync" | "snapshot"))
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn current_seq(next_seq: u64) -> u64 {
    next_seq.saturating_sub(1)
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InteractionResponseRequest {
    response: String,
}

async fn respond_interaction(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<InteractionResponseRequest>,
) -> Result<Json<Value>, ApiError> {
    state
        .runtime
        .respond_interaction(&run_id, request.response)
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
