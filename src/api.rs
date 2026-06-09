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
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{SinkExt, Stream, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tmux_tools_core::{
    TmuxInvocation,
    stream::{CaptureAnsiOpts, capture_ansi, stream_pane},
    tmux, with_invocation,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::broadcast,
    time::Instant,
};

use crate::{
    app::{AppState, PaneStreamEntry},
    model::{NodeKind, WorkflowNode, WorkflowV3, normalize_workflow_value, validate_workflow},
    runtime::{
        InterruptedRunSummary, NodeTestContext, PersistedRun, RuntimeCheckpoint, RuntimeStatus,
        available_agents, check_cli, run_node_preview,
    },
    tmux_exec::build_tmux_invocation,
};

const PANE_STREAM_HEARTBEAT: Duration = Duration::from_secs(5);
const PANE_STREAM_SEND_TIMEOUT: Duration = Duration::from_secs(2);
const PANE_STREAM_READ_MAX_RETRIES: usize = 5;
const PANE_STREAM_READ_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneStreamTaskExit {
    NoSubscribers,
    Terminal,
}

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
            "runAs": true,
        }
    })))
}

fn run_tmux_invocation(workflow: &WorkflowV3) -> TmuxInvocation {
    workflow
        .run_as
        .as_ref()
        .map(build_tmux_invocation)
        .unwrap_or_default()
}

fn shell_quote(token: &str) -> String {
    shlex::try_quote(token)
        .map(|quoted| quoted.into_owned())
        .unwrap_or_else(|_| format!("'{}'", token.replace('\'', "'\\''")))
}

fn build_attach_command(invocation: &TmuxInvocation, session_name: &str) -> String {
    let mut parts = Vec::new();

    for token in &invocation.prefix {
        parts.push(shell_quote(token));
    }

    parts.push(shell_quote(&invocation.tmux_bin));

    if let Some(socket) = invocation
        .socket
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push("-L".to_owned());
        parts.push(shell_quote(socket));
    }

    parts.push("attach".to_owned());
    parts.push("-t".to_owned());
    parts.push(shell_quote(session_name));

    parts.join(" ")
}

fn session_name_from_value(value: &Value) -> Option<String> {
    ["sessionName", "session_name"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn session_name_from_checkpoint(checkpoint: &RuntimeCheckpoint) -> Option<String> {
    if let Some(node_id) = checkpoint.current_node_id.as_deref() {
        if let Some(result) = checkpoint.all_results.get(node_id) {
            if let Some(name) = result
                .parsed_output
                .as_ref()
                .and_then(session_name_from_value)
            {
                return Some(name);
            }
            if let Ok(value) = serde_json::from_str::<Value>(&result.output) {
                if let Some(name) = session_name_from_value(&value) {
                    return Some(name);
                }
            }
        }
    }

    for result in checkpoint.all_results.values().rev() {
        if let Some(name) = result
            .parsed_output
            .as_ref()
            .and_then(session_name_from_value)
        {
            return Some(name);
        }
        if let Ok(value) = serde_json::from_str::<Value>(&result.output) {
            if let Some(name) = session_name_from_value(&value) {
                return Some(name);
            }
        }
    }

    None
}

async fn session_name_from_active_pane(
    state: &AppState,
    run_id: &str,
    invocation: &TmuxInvocation,
    current_node_id: Option<&str>,
) -> Option<String> {
    let pane = if let Some(node_id) = current_node_id {
        state
            .runtime
            .registry
            .resolve_active_pane(run_id, node_id)
            .await
    } else {
        state
            .runtime
            .registry
            .resolve_active_pane(run_id, "active")
            .await
    }?;
    let invocation = invocation.clone();
    tokio::task::spawn_blocking(move || {
        with_invocation(invocation, || {
            tmux::run_checked(&[
                "display-message",
                "-p",
                "-t",
                pane.as_str(),
                "-F",
                "#{session_name}",
            ])
            .ok()
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
        })
    })
    .await
    .ok()
    .flatten()
}

async fn resolve_run_session_name(
    state: &AppState,
    run_id: &str,
    persisted: &PersistedRun,
) -> Option<String> {
    let invocation = run_tmux_invocation(&persisted.workflow);
    session_name_from_active_pane(
        state,
        run_id,
        &invocation,
        persisted.checkpoint.current_node_id.as_deref(),
    )
    .await
    .or_else(|| session_name_from_checkpoint(&persisted.checkpoint))
}

async fn run_observability_object(
    state: &AppState,
    run_id: &str,
) -> serde_json::Map<String, Value> {
    let mut fields = serde_json::Map::new();
    let Ok(Some(persisted)) = state.runtime.db.get_run(run_id).await else {
        return fields;
    };
    let Some(session_name) = resolve_run_session_name(state, run_id, &persisted).await else {
        return fields;
    };
    let invocation = run_tmux_invocation(&persisted.workflow);
    fields.insert("sessionName".to_owned(), json!(session_name));
    fields.insert(
        "attachCommand".to_owned(),
        json!(build_attach_command(&invocation, &session_name)),
    );
    fields
}

fn merge_run_observability(value: &mut Value, observability: serde_json::Map<String, Value>) {
    if observability.is_empty() {
        return;
    }
    let Some(object) = value.as_object_mut() else {
        return;
    };
    object.extend(observability);
}

async fn enrich_interrupted_run(
    state: &AppState,
    run: InterruptedRunSummary,
) -> Result<Value, ApiError> {
    let run_id = run.run_id.clone();
    let mut value = serde_json::to_value(run)?;
    merge_run_observability(&mut value, run_observability_object(state, &run_id).await);
    Ok(value)
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
        let config = match &node.kind {
            NodeKind::Subflow { subflow_config } | NodeKind::Call { subflow_config } => {
                subflow_config
            }
            NodeKind::Task { .. }
            | NodeKind::Approval
            | NodeKind::Split
            | NodeKind::Collector
            | NodeKind::Decide { .. }
            | NodeKind::ParallelBatch { .. }
            | NodeKind::Spawn { .. }
            | NodeKind::Send { .. }
            | NodeKind::Wait { .. }
            | NodeKind::Capture { .. }
            | NodeKind::Kill { .. }
            | NodeKind::RunAgent { .. } => continue,
        };
        let name = config.workflow_name.trim();
        if !name.is_empty() {
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
    if !matches!(&node.kind, NodeKind::Task { .. }) {
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
    let mut response = json!({ "success": true, "runId": run_id });
    merge_run_observability(
        &mut response,
        run_observability_object(&state, &run_id).await,
    );
    Ok(Json(response))
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
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if let Some(origin) = headers.get(header::ORIGIN) {
        match origin.to_str() {
            Ok(origin) if is_allowed_origin(origin) => {}
            _ => return StatusCode::FORBIDDEN.into_response(),
        }
    }

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
    .into_response()
}

fn is_allowed_origin(origin: &str) -> bool {
    let origin = origin.trim();
    origin.starts_with("tauri://")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://localhost:")
}

async fn pane_stream_socket(
    mut socket: WebSocket,
    state: AppState,
    run_id: String,
    pane: String,
) -> anyhow::Result<()> {
    let (pane_target, invocation) = match resolve_run_pane_context(&state, &run_id, &pane).await {
        Ok(context) => context,
        Err(error) => {
            send_socket_error_and_close(&mut socket, 0, &error.to_string()).await;
            return Ok(());
        }
    };

    let (mut sender, mut receiver) = socket.split();
    let mut next_seq = 0_u64;
    send_snapshot(&mut sender, &pane_target, &invocation, &mut next_seq).await?;

    let mut pane_receiver =
        subscribe_pane_stream(&state, &run_id, &pane, &pane_target, &invocation).await;

    let mut heartbeat = Box::pin(tokio::time::sleep(PANE_STREAM_HEARTBEAT));
    let mut stream_chunks_seen = false;
    loop {
        tokio::select! {
            client_message = receiver.next() => {
                match client_message {
                    Some(Ok(message)) if is_resync_request(&message) => {
                        send_snapshot(&mut sender, &pane_target, &invocation, &mut next_seq).await?;
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
            stream_result = pane_receiver.recv() => {
                match stream_result {
                    Ok(bytes) => {
                        stream_chunks_seen = true;
                        match send_data_frame(&mut sender, &bytes, &mut next_seq).await {
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
                                send_snapshot(&mut sender, &pane_target, &invocation, &mut next_seq).await?;
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
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            run_id = %run_id,
                            pane = %pane,
                            pane_target = %pane_target,
                            skipped,
                            "pane websocket stream lagged; sending snapshot resync"
                        );
                        send_snapshot(&mut sender, &pane_target, &invocation, &mut next_seq).await?;
                        heartbeat.as_mut().reset(Instant::now() + PANE_STREAM_HEARTBEAT);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!(
                            run_id = %run_id,
                            pane = %pane,
                            pane_target = %pane_target,
                            "pane stream reached EOF"
                        );
                        if !stream_chunks_seen {
                            let _ = send_error_frame(
                                &mut sender,
                                &mut next_seq,
                                "pane stream unavailable",
                            )
                            .await;
                        }
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

    drop(pane_receiver);
    state.pane_streams.unsubscribe(&pane_target).await;

    Ok(())
}

async fn resolve_run_pane_context(
    state: &AppState,
    run_id: &str,
    pane: &str,
) -> anyhow::Result<(String, TmuxInvocation)> {
    let persisted = state
        .runtime
        .db
        .get_run(run_id)
        .await?
        .context("run not found")?;
    let invocation = run_tmux_invocation(&persisted.workflow);

    if let Some(target) = state
        .runtime
        .registry
        .resolve_active_pane(run_id, pane)
        .await
    {
        return Ok((target, invocation));
    }

    let candidates = pane_candidates(&persisted.checkpoint);
    if let Some(target) = match_pane_candidate(
        pane,
        &candidates,
        persisted.checkpoint.current_node_id.as_deref(),
    ) {
        return Ok((target, invocation));
    }

    if matches!(
        persisted.checkpoint.status,
        RuntimeStatus::Running | RuntimeStatus::Paused
    ) {
        anyhow::bail!("run has no active pane matching {pane}");
    }
    anyhow::bail!("run has no pane matching {pane}");
}

async fn start_pane_stream(
    pane_target: &str,
    invocation: &TmuxInvocation,
) -> anyhow::Result<impl AsyncReadExt + Send + Unpin + 'static> {
    let pane_target = pane_target.to_string();
    let invocation = invocation.clone();
    tokio::task::spawn_blocking(move || {
        with_invocation(invocation, || {
            futures::executor::block_on(stream_pane(&pane_target))
        })
    })
    .await
    .context("stream_pane task panicked")?
}

async fn subscribe_pane_stream(
    state: &AppState,
    run_id: &str,
    pane: &str,
    pane_target: &str,
    invocation: &TmuxInvocation,
) -> broadcast::Receiver<Vec<u8>> {
    let mut streams = state.pane_streams.inner.lock().await;
    if let Some(entry) = streams.get_mut(pane_target) {
        entry.refcount = entry.refcount.saturating_add(1);
        return entry.sender.subscribe();
    }

    let (sender, receiver) = broadcast::channel::<Vec<u8>>(256);
    streams.insert(
        pane_target.to_string(),
        PaneStreamEntry {
            sender: sender.clone(),
            refcount: 1,
        },
    );
    drop(streams);

    spawn_pane_stream_task(
        state.clone(),
        run_id.to_string(),
        pane.to_string(),
        pane_target.to_string(),
        invocation.clone(),
        sender,
    );

    receiver
}

async fn pump_pane_stream<R>(
    pane_reader: &mut R,
    sender: &broadcast::Sender<Vec<u8>>,
    run_id: &str,
    pane: &str,
    pane_target: &str,
    retry_delay: Duration,
) -> PaneStreamTaskExit
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 8192];
    let mut consecutive_read_errors = 0_usize;
    loop {
        tokio::select! {
            _ = sender.closed() => {
                tracing::debug!(
                    run_id = %run_id,
                    pane = %pane,
                    pane_target = %pane_target,
                    "pane stream has no subscribers"
                );
                break PaneStreamTaskExit::NoSubscribers;
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
                        break PaneStreamTaskExit::Terminal;
                    }
                    Ok(bytes_read) => {
                        consecutive_read_errors = 0;
                        if sender.send(buffer[..bytes_read].to_vec()).is_err() {
                            tracing::debug!(
                                run_id = %run_id,
                                pane = %pane,
                                pane_target = %pane_target,
                                "pane stream has no subscribers"
                            );
                            break PaneStreamTaskExit::NoSubscribers;
                        }
                    }
                    Err(error) => {
                        consecutive_read_errors += 1;
                        if consecutive_read_errors > PANE_STREAM_READ_MAX_RETRIES {
                            tracing::warn!(
                                run_id = %run_id,
                                pane = %pane,
                                pane_target = %pane_target,
                                error = %error,
                                retries = PANE_STREAM_READ_MAX_RETRIES,
                                "pane stream read failed permanently"
                            );
                            break PaneStreamTaskExit::Terminal;
                        }

                        tracing::warn!(
                            run_id = %run_id,
                            pane = %pane,
                            pane_target = %pane_target,
                            error = %error,
                            retry = consecutive_read_errors,
                            max_retries = PANE_STREAM_READ_MAX_RETRIES,
                            "pane stream read failed; retrying"
                        );
                        tokio::select! {
                            _ = sender.closed() => {
                                tracing::debug!(
                                    run_id = %run_id,
                                    pane = %pane,
                                    pane_target = %pane_target,
                                    "pane stream has no subscribers"
                                );
                                break PaneStreamTaskExit::NoSubscribers;
                            }
                            _ = tokio::time::sleep(retry_delay) => {}
                        }
                    }
                }
            }
        }
    }
}

fn spawn_pane_stream_task(
    state: AppState,
    run_id: String,
    pane: String,
    pane_target: String,
    invocation: TmuxInvocation,
    sender: broadcast::Sender<Vec<u8>>,
) {
    tokio::spawn(async move {
        let exit = match start_pane_stream(&pane_target, &invocation).await {
            Ok(mut pane_reader) => {
                pump_pane_stream(
                    &mut pane_reader,
                    &sender,
                    &run_id,
                    &pane,
                    &pane_target,
                    PANE_STREAM_READ_RETRY_DELAY,
                )
                .await
            }
            Err(error) => {
                tracing::warn!(
                    run_id = %run_id,
                    pane = %pane,
                    pane_target = %pane_target,
                    error = %error,
                    "failed to start pane stream"
                );
                PaneStreamTaskExit::Terminal
            }
        };

        match exit {
            PaneStreamTaskExit::NoSubscribers => {
                state
                    .pane_streams
                    .remove_if_sender(&pane_target, &sender)
                    .await;
            }
            PaneStreamTaskExit::Terminal => {
                state
                    .pane_streams
                    .remove_terminal_sender(&pane_target, &sender)
                    .await;
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneCandidate {
    key: String,
    target: String,
}

fn pane_candidates(checkpoint: &RuntimeCheckpoint) -> Vec<PaneCandidate> {
    let mut candidates = Vec::new();
    let trusted_targets = trusted_pane_targets(checkpoint);
    for (node_id, result) in &checkpoint.all_results {
        push_pane_candidate(
            &mut candidates,
            node_id,
            result.metadata.agent_session_id.as_deref(),
        );
        if let Some(value) = &result.parsed_output {
            push_output_pane_candidate(
                &mut candidates,
                &trusted_targets,
                &checkpoint.run_id,
                node_id,
                pane_target_from_value(value),
            );
        }
        if let Ok(value) = serde_json::from_str::<Value>(&result.output) {
            push_output_pane_candidate(
                &mut candidates,
                &trusted_targets,
                &checkpoint.run_id,
                node_id,
                pane_target_from_value(&value),
            );
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

fn trusted_pane_targets(checkpoint: &RuntimeCheckpoint) -> BTreeSet<String> {
    let mut trusted_targets = BTreeSet::new();
    for result in checkpoint.all_results.values() {
        insert_trusted_pane_target(
            &mut trusted_targets,
            result.metadata.agent_session_id.as_deref(),
        );
    }
    for execution in &checkpoint.execution_log.node_executions {
        insert_trusted_pane_target(
            &mut trusted_targets,
            execution.metadata.agent_session_id.as_deref(),
        );
    }
    trusted_targets
}

fn insert_trusted_pane_target(trusted_targets: &mut BTreeSet<String>, target: Option<&str>) {
    let Some(target) = target.map(str::trim).filter(|target| !target.is_empty()) else {
        return;
    };
    trusted_targets.insert(target.to_string());
}

fn push_output_pane_candidate(
    candidates: &mut Vec<PaneCandidate>,
    trusted_targets: &BTreeSet<String>,
    run_id: &str,
    node_id: &str,
    target: Option<&str>,
) {
    let Some(target) = target.map(str::trim).filter(|target| !target.is_empty()) else {
        return;
    };
    if trusted_targets.contains(target) {
        push_pane_candidate(candidates, node_id, Some(target));
    } else {
        tracing::warn!(
            run_id = %run_id,
            node_id = %node_id,
            pane_target = %target,
            "ignored untrusted pane target from node output"
        );
    }
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
    invocation: &TmuxInvocation,
    next_seq: &mut u64,
) -> anyhow::Result<()> {
    let target = pane_target.to_string();
    let invocation = invocation.clone();
    let snapshot = tokio::task::spawn_blocking(move || {
        with_invocation(invocation, || {
            capture_ansi(&target, CaptureAnsiOpts::default()).map(|snapshot| snapshot.into_bytes())
        })
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
    session_id: String,
    response: String,
}

async fn respond_interaction(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<InteractionResponseRequest>,
) -> Result<Json<Value>, ApiError> {
    state
        .runtime
        .respond_interaction(&run_id, &request.session_id, request.response)
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
    let mut response = json!({ "success": true, "runId": run_id });
    merge_run_observability(
        &mut response,
        run_observability_object(&state, &run_id).await,
    );
    Ok(Json(response))
}

async fn restart_run(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let new_run_id = state.runtime.restart_from(&run_id, &node_id).await?;
    let mut response = json!({ "success": true, "runId": new_run_id });
    merge_run_observability(
        &mut response,
        run_observability_object(&state, &new_run_id).await,
    );
    Ok(Json(response))
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
    let runs = state.runtime.db.list_interrupted_runs().await?;
    let mut enriched = Vec::with_capacity(runs.len());
    for run in runs {
        enriched.push(enrich_interrupted_run(&state, run).await?);
    }
    Ok(Json(json!(enriched)))
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

fn node_from_value(value: Value) -> anyhow::Result<WorkflowNode> {
    if value.get("version").is_some()
        || value.get("nodes").is_some()
        || value.get("entryNodeId").is_some()
    {
        anyhow::bail!("workflow payload is not valid for node testing; provide a single node");
    }
    let node: WorkflowNode = serde_json::from_value(value)
        .context("node payload must be a canonical v3 WorkflowNode")?;
    if matches!(&node.kind, NodeKind::Task { .. } | NodeKind::Approval) {
        return Ok(node);
    }
    anyhow::bail!(
        "node payload must use canonical v3 task or approval kind types; got {}",
        node.kind.as_str()
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        io,
        pin::Pin,
        task::{Context as TaskContext, Poll},
    };
    use tokio::io::ReadBuf;

    use crate::{
        app::PaneStreamRegistry,
        runtime::{
            AgentExecutionMetadata, ExecutionLog, NodeExecutionLog, NodeResult, RuntimeStatus,
        },
    };

    struct ScriptedReader {
        reads: VecDeque<io::Result<Vec<u8>>>,
    }

    impl ScriptedReader {
        fn new(reads: Vec<io::Result<Vec<u8>>>) -> Self {
            Self {
                reads: reads.into(),
            }
        }
    }

    impl AsyncRead for ScriptedReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut TaskContext<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            match self.reads.pop_front() {
                Some(Ok(bytes)) => {
                    buf.put_slice(&bytes);
                    Poll::Ready(Ok(()))
                }
                Some(Err(error)) => Poll::Ready(Err(error)),
                None => Poll::Ready(Ok(())),
            }
        }
    }

    #[test]
    fn build_attach_command_omits_socket_flag_for_default_invocation() {
        let invocation = TmuxInvocation::default();
        let command = build_attach_command(&invocation, "my-run");
        assert!(!command.contains("-L"));
        assert_eq!(command, "tmux attach -t my-run");
    }

    #[test]
    fn build_attach_command_includes_socket_flag_for_run_as_invocation() {
        let invocation = TmuxInvocation {
            prefix: vec![
                "sudo".to_string(),
                "-u".to_string(),
                "agent".to_string(),
                "-H".to_string(),
                "--".to_string(),
            ],
            socket: Some("silverbond".to_string()),
            tmux_bin: "tmux".to_string(),
        };
        let command = build_attach_command(&invocation, "my-run");
        assert!(command.contains("-L silverbond"));
        assert_eq!(
            command,
            "sudo -u agent -H -- tmux -L silverbond attach -t my-run"
        );
    }

    #[test]
    fn build_attach_command_quotes_tokens_with_spaces() {
        let invocation = TmuxInvocation::default();
        let command = build_attach_command(&invocation, "my session");
        assert!(command.contains("'my session'") || command.contains("\"my session\""));
        assert!(!command.ends_with("my session"));
    }

    #[test]
    fn origin_allowlist_accepts_tauri_and_local_development() {
        assert!(is_allowed_origin("tauri://localhost"));
        assert!(is_allowed_origin("http://127.0.0.1:3333"));
        assert!(is_allowed_origin("http://localhost:5173"));
        assert!(!is_allowed_origin("https://evil.com"));
        assert!(!is_allowed_origin("http://evil.com:3333"));
    }

    #[test]
    fn pane_candidates_ignore_untrusted_json_targets() {
        let mut checkpoint = test_checkpoint();
        checkpoint.all_results.insert(
            "node1".to_string(),
            NodeResult {
                parsed_output: Some(json!({ "target": "%999" })),
                output: r#"{"target":"%999"}"#.to_string(),
                metadata: AgentExecutionMetadata {
                    agent_session_id: Some("%123".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        checkpoint.all_results.insert(
            "node2".to_string(),
            NodeResult {
                parsed_output: Some(json!({ "target": "%222" })),
                ..Default::default()
            },
        );
        checkpoint
            .execution_log
            .node_executions
            .push(NodeExecutionLog {
                node_id: "node2".to_string(),
                metadata: AgentExecutionMetadata {
                    agent_session_id: Some("%222".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            });

        let candidates = pane_candidates(&checkpoint);

        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.target == "%999")
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.key == "node1" && candidate.target == "%123")
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.key == "node2" && candidate.target == "%222")
        );
    }

    #[tokio::test]
    async fn pane_stream_registry_fans_out_and_cleans_up() {
        let registry = PaneStreamRegistry::default();
        let (sender, mut first_receiver) = broadcast::channel::<Vec<u8>>(256);
        let mut second_receiver = {
            let mut streams = registry.inner.lock().await;
            streams.insert(
                "%1".to_string(),
                PaneStreamEntry {
                    sender: sender.clone(),
                    refcount: 1,
                },
            );
            let entry = streams.get_mut("%1").expect("pane stream entry exists");
            entry.refcount += 1;
            entry.sender.subscribe()
        };

        sender.send(b"chunk".to_vec()).unwrap();
        assert_eq!(first_receiver.recv().await.unwrap(), b"chunk".to_vec());
        assert_eq!(second_receiver.recv().await.unwrap(), b"chunk".to_vec());

        registry.unsubscribe("%1").await;
        assert_eq!(
            registry
                .inner
                .lock()
                .await
                .get("%1")
                .map(|entry| entry.refcount),
            Some(1)
        );

        registry.unsubscribe("%1").await;
        assert!(!registry.inner.lock().await.contains_key("%1"));
    }

    #[tokio::test]
    async fn pane_stream_retry_keeps_receivers_after_transient_read_error() {
        let (sender, mut receiver) = broadcast::channel::<Vec<u8>>(16);
        let mut reader = ScriptedReader::new(vec![
            Err(io::Error::new(io::ErrorKind::Interrupted, "temporary")),
            Ok(b"chunk".to_vec()),
            Ok(Vec::new()),
        ]);

        let exit = pump_pane_stream(
            &mut reader,
            &sender,
            "run-test",
            "pane-test",
            "%1",
            Duration::ZERO,
        )
        .await;

        assert_eq!(exit, PaneStreamTaskExit::Terminal);
        assert_eq!(receiver.recv().await.unwrap(), b"chunk".to_vec());
    }

    #[tokio::test]
    async fn pane_stream_read_errors_stop_after_retry_limit() {
        let (sender, mut receiver) = broadcast::channel::<Vec<u8>>(16);
        let mut reader = ScriptedReader::new(
            (0..=PANE_STREAM_READ_MAX_RETRIES)
                .map(|attempt| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("failure {attempt}"),
                    ))
                })
                .collect(),
        );

        let exit = pump_pane_stream(
            &mut reader,
            &sender,
            "run-test",
            "pane-test",
            "%1",
            Duration::ZERO,
        )
        .await;

        assert_eq!(exit, PaneStreamTaskExit::Terminal);
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn pane_stream_registry_remove_if_sender_keeps_live_subscribers() {
        let registry = PaneStreamRegistry::default();
        let (sender, _receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(
                "%1".to_string(),
                PaneStreamEntry {
                    sender: sender.clone(),
                    refcount: 2,
                },
            );
        }

        registry.remove_if_sender("%1", &sender).await;
        assert_eq!(
            registry
                .inner
                .lock()
                .await
                .get("%1")
                .map(|entry| entry.refcount),
            Some(2)
        );

        registry.remove_terminal_sender("%1", &sender).await;
        assert!(!registry.inner.lock().await.contains_key("%1"));
    }

    fn test_checkpoint() -> RuntimeCheckpoint {
        RuntimeCheckpoint {
            run_id: "run-test".to_string(),
            status: RuntimeStatus::Running,
            workflow_name: "workflow".to_string(),
            current_node_id: None,
            current_node_name: None,
            all_results: BTreeMap::new(),
            batch_item_results: BTreeMap::new(),
            last_output: String::new(),
            execution_epoch: 0,
            active_cursors: Vec::new(),
            split_families: BTreeMap::new(),
            collector_barriers: BTreeMap::new(),
            queued_approvals: Vec::new(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            total_executed: 0,
            output_hashes: BTreeMap::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            var_map: BTreeMap::new(),
            goal: String::new(),
            cwd: String::new(),
            use_orchestrator: false,
            max_total_steps: 0,
            max_visits_per_node: 0,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            pending_approval: None,
            execution_log: ExecutionLog {
                run_id: "run-test".to_string(),
                workflow_name: "workflow".to_string(),
                goal: String::new(),
                cwd: String::new(),
                start_time: "2026-01-01T00:00:00Z".to_string(),
                end_time: None,
                use_orchestrator: false,
                aborted: false,
                total_duration: String::new(),
                node_executions: Vec::new(),
                decisions: Vec::new(),
                transitions: Vec::new(),
                terminal_reason: None,
            },
        }
    }
}
