use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use anyhow::Context;
use async_stream::stream;
use axum::{
    Json, Router,
    extract::{
        Path, Query, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{
    SinkExt, Stream, StreamExt,
    future::{join_all, try_join_all},
    stream::SplitSink,
};
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
    app::{AppState, PaneStreamEntry, SecurityConfig},
    model::{
        NodeKind, RunAsConfig, WorkflowNode, WorkflowV3, normalize_workflow_value,
        validate_workflow,
    },
    runtime::{
        InterruptedRunSummary, NodeTestContext, PersistedRun, RunControlError, RuntimeCheckpoint,
        RuntimeStatus, available_agents, check_cli, run_node_preview,
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

struct PaneStreamGuard {
    state: Option<AppState>,
    pane_target: String,
}

impl PaneStreamGuard {
    fn new(state: AppState, pane_target: String) -> Self {
        Self {
            state: Some(state),
            pane_target,
        }
    }

    fn unsubscribe(mut self) {
        self.spawn_unsubscribe();
    }

    fn spawn_unsubscribe(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        let pane_target = self.pane_target.clone();
        tokio::spawn(async move {
            state.pane_streams.unsubscribe(&pane_target).await;
        });
    }
}

impl Drop for PaneStreamGuard {
    fn drop(&mut self) {
        self.spawn_unsubscribe();
    }
}

pub fn router(state: AppState) -> Router {
    let stream_routes = Router::new()
        .route("/api/runs/{run_id}/stream", get(stream_run))
        .route(
            "/api/runs/{run_id}/panes/{pane}/stream",
            get(pane_stream_ws),
        )
        .route_layer(middleware::from_fn(require_allowed_stream_origin));

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
        .merge(stream_routes)
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
        .layer(middleware::from_fn(require_same_origin_mutation))
}

async fn require_allowed_stream_origin(request: Request, next: Next) -> Response {
    let allowed = if is_websocket_upgrade(&request) {
        has_allowed_origin(request.headers())
    } else {
        is_allowed_same_origin_or_origin(request.headers())
    };

    if allowed {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

async fn require_same_origin_mutation(request: Request, next: Next) -> Response {
    if request.method() == Method::GET || is_allowed_same_origin_or_origin(request.headers()) {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
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

fn run_tmux_invocation(workflow: &WorkflowV3, run_id: &str) -> TmuxInvocation {
    workflow
        .run_as
        .as_ref()
        .map(|run_as| build_tmux_invocation(run_as, run_id))
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

async fn session_name_from_pane_target(
    invocation: &TmuxInvocation,
    pane_target: &str,
    stored_session_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = stored_session_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return Some(name.to_owned());
    }

    let invocation = invocation.clone();
    let pane_target = pane_target.to_owned();
    tokio::task::spawn_blocking(move || {
        with_invocation(invocation, || {
            tmux::run_checked(&[
                "display-message",
                "-p",
                "-t",
                pane_target.as_str(),
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
    session_name_from_pane_target(invocation, &pane, None).await
}

async fn resolve_run_session_name(
    state: &AppState,
    run_id: &str,
    persisted: &PersistedRun,
) -> Option<String> {
    let invocation = run_tmux_invocation(&persisted.workflow, run_id);
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
    let invocation = run_tmux_invocation(&persisted.workflow, run_id);
    let mut pane_entries = state.runtime.registry.active_pane_entries(run_id).await;
    pane_entries.sort_by_key(|entry| entry.sequence);

    let panes = join_all(pane_entries.iter().map(|entry| async {
        let session_name = session_name_from_pane_target(
            &invocation,
            &entry.target,
            entry.session_name.as_deref(),
        )
        .await?;
        Some(json!({
            "pane": entry.key,
            "sessionName": session_name,
            "attachCommand": build_attach_command(&invocation, &session_name),
        }))
    }))
    .await
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    let (session_name, attach_command) = if let Some(last) = panes.last() {
        (
            last.get("sessionName")
                .and_then(Value::as_str)
                .map(str::to_owned),
            last.get("attachCommand")
                .and_then(Value::as_str)
                .map(str::to_owned),
        )
    } else {
        let session_name = resolve_run_session_name(state, run_id, &persisted).await;
        (
            session_name.clone(),
            session_name
                .as_ref()
                .map(|name| build_attach_command(&invocation, name)),
        )
    };

    let (Some(session_name), Some(attach_command)) = (session_name, attach_command) else {
        return fields;
    };

    fields.insert("sessionName".to_owned(), json!(session_name));
    fields.insert("attachCommand".to_owned(), json!(attach_command));
    if !panes.is_empty() {
        fields.insert("panes".to_owned(), json!(panes));
    }
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

async fn run_action_response(state: &AppState, run_id: &str) -> Result<Value, ApiError> {
    let persisted = state
        .runtime
        .db
        .get_run(run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    let mut response = json!({
        "success": true,
        "runId": run_id,
        "streamToken": persisted.stream_token,
    });
    merge_run_observability(&mut response, run_observability_object(state, run_id).await);
    Ok(response)
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
        None => Err(ApiError::not_found("Not found")),
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
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
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
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
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
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let node =
        node_from_value(node_value).map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !matches!(&node.kind, NodeKind::Task { .. }) {
        return Err(ApiError::bad_request("Only task nodes can be tested"));
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
    #[serde(default)]
    unlock_secret: Option<String>,
}

async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<Value>, ApiError> {
    let mut normalized = normalize_workflow_value(request.workflow)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    hydrate_saved_subflows(&state, &mut normalized.workflow).await?;
    let validation = validate_workflow(normalized.workflow.clone());
    let errors = validation
        .issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .cloned()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(ApiError::validation_body(
            StatusCode::BAD_REQUEST,
            json!({
                "error": "Validation failed",
                "details": errors,
            }),
        ));
    }
    authorize_and_prepare_run_security(
        &mut normalized.workflow,
        &state.security,
        request.unlock_secret.as_deref(),
    )?;
    let run_id = state
        .runtime
        .start_run(
            normalized.workflow,
            request.variable_overrides,
            request.start_node_id,
        )
        .await?;
    Ok(Json(run_action_response(&state, &run_id).await?))
}

fn authorize_and_prepare_run_security(
    workflow: &mut WorkflowV3,
    security: &SecurityConfig,
    unlock_secret: Option<&str>,
) -> Result<(), ApiError> {
    let launches_processes = workflow_launches_processes(workflow);
    let uses_run_as_prefix = workflow.run_as.is_some();
    if (launches_processes || uses_run_as_prefix)
        && resolved_run_as_is_privileged(workflow.run_as.as_ref(), security.agent_user.as_deref())
        && !security.verify_unlock_secret(unlock_secret)
    {
        return Err(privileged_unlock_required());
    }

    if launches_processes && workflow.run_as.is_none() {
        if let Some(agent_user) = security.agent_user.as_deref() {
            workflow.run_as = Some(RunAsConfig {
                user: Some(agent_user.to_owned()),
                command: None,
                socket: None,
            });
        }
    }

    Ok(())
}

fn privileged_unlock_required() -> ApiError {
    ApiError::validation_body(
        StatusCode::FORBIDDEN,
        json!({
            "error": "Privileged run requires unlock password.",
            "code": "privileged_unlock_required",
        }),
    )
}

fn resolved_run_as_is_privileged(run_as: Option<&RunAsConfig>, agent_user: Option<&str>) -> bool {
    match (run_as, agent_user) {
        (None, Some(_)) => false,
        (None, None) => true,
        (Some(run_as), Some(agent_user)) => {
            if run_as.command.is_some() {
                return true;
            }
            match run_as.user.as_deref() {
                Some(user) => user != agent_user,
                None => true,
            }
        }
        (Some(_), None) => true,
    }
}

fn workflow_launches_processes(workflow: &WorkflowV3) -> bool {
    workflow.nodes.iter().any(node_launches_process)
        || workflow
            .subflows
            .values()
            .any(|subflow| workflow_launches_processes(subflow))
}

fn node_launches_process(node: &WorkflowNode) -> bool {
    matches!(
        &node.kind,
        NodeKind::Task { .. }
            | NodeKind::Decide { .. }
            | NodeKind::Spawn { .. }
            | NodeKind::RunAgent { .. }
    )
}

async fn stream_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<StreamTokenQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let token_candidates = query.token.into_iter().collect::<Vec<_>>();
    require_run_stream_token(&state, &run_id, &token_candidates).await?;
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
    let token_candidates = websocket_stream_token_candidates(&headers);
    let stream_token = match require_run_stream_token(&state, &run_id, &token_candidates).await {
        Ok((_, token)) => token,
        Err(error) => return error.into_response(),
    };
    ws.protocols([stream_token.clone()])
        .on_upgrade(move |socket| async move {
            let log_run_id = run_id.clone();
            let log_pane = pane.clone();
            let task = tokio::spawn(async move {
                if let Err(error) =
                    pane_stream_socket(socket, state, run_id, pane, stream_token).await
                {
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

fn has_allowed_origin(headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(is_allowed_origin)
}

fn sec_fetch_site(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("sec-fetch-site")
        .and_then(|site| site.to_str().ok())
        .map(str::trim)
}

fn is_same_origin_fetch_site(site: &str) -> bool {
    site.eq_ignore_ascii_case("same-origin") || site.eq_ignore_ascii_case("none")
}

fn is_allowed_same_origin_or_origin(headers: &HeaderMap) -> bool {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
    {
        if !is_allowed_origin(origin) {
            return false;
        }
    }

    if let Some(site) = sec_fetch_site(headers) {
        if site.eq_ignore_ascii_case("cross-site") {
            return false;
        }
        if is_same_origin_fetch_site(site) {
            return true;
        }
    }

    has_allowed_origin(headers)
}

fn is_websocket_upgrade(request: &Request) -> bool {
    request
        .headers()
        .get(header::UPGRADE)
        .and_then(|upgrade| upgrade.to_str().ok())
        .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"))
}

#[derive(Debug, Deserialize)]
struct StreamTokenQuery {
    token: Option<String>,
}

fn stream_token_forbidden() -> ApiError {
    ApiError::validation_body(
        StatusCode::FORBIDDEN,
        json!({ "error": "stream token required" }),
    )
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for idx in 0..max_len {
        let left_byte = left.get(idx).copied().unwrap_or(0);
        let right_byte = right.get(idx).copied().unwrap_or(0);
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}

fn websocket_stream_token_candidates(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("sec-websocket-protocol")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn require_run_stream_token(
    state: &AppState,
    run_id: &str,
    candidates: &[String],
) -> Result<(PersistedRun, String), ApiError> {
    if candidates.is_empty() {
        return Err(stream_token_forbidden());
    }

    let persisted = state
        .runtime
        .db
        .get_run(run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    let Some(stream_token) = candidates
        .iter()
        .find(|candidate| constant_time_eq(candidate, &persisted.stream_token))
        .cloned()
    else {
        return Err(stream_token_forbidden());
    };

    Ok((persisted, stream_token))
}

async fn pane_stream_socket(
    mut socket: WebSocket,
    state: AppState,
    run_id: String,
    pane: String,
    stream_token: String,
) -> anyhow::Result<()> {
    let (pane_target, invocation) =
        match resolve_run_pane_context(&state, &run_id, &pane, &stream_token).await {
            Ok(context) => context,
            Err(error) => {
                if matches!(error, PaneContextError::Internal(_)) {
                    tracing::warn!(
                        run_id = %run_id,
                        pane = %pane,
                        error = ?error,
                        "failed to resolve pane websocket context"
                    );
                }
                send_socket_error_and_close(&mut socket, 0, error.client_message()).await;
                return Ok(());
            }
        };

    let mut pane_receiver =
        match subscribe_pane_stream(&state, &run_id, &pane, &pane_target, &invocation).await {
            Ok(receiver) => receiver,
            Err(error) => {
                send_socket_error_and_close(&mut socket, 0, &error.to_string()).await;
                return Ok(());
            }
        };
    let pane_stream_guard = PaneStreamGuard::new(state.clone(), pane_target.clone());

    let (mut sender, mut receiver) = socket.split();
    let mut next_seq = 0_u64;
    send_snapshot(&mut sender, &pane_target, &invocation, &mut next_seq).await?;

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
    pane_stream_guard.unsubscribe();

    Ok(())
}

async fn resolve_run_pane_context(
    state: &AppState,
    run_id: &str,
    pane: &str,
    stream_token: &str,
) -> Result<(String, TmuxInvocation), PaneContextError> {
    let persisted = state
        .runtime
        .db
        .get_run(run_id)
        .await
        .map_err(PaneContextError::Internal)?
        .ok_or(PaneContextError::RunNotFound)?;
    if !constant_time_eq(stream_token, &persisted.stream_token) {
        return Err(PaneContextError::Unauthorized);
    }
    let invocation = run_tmux_invocation(&persisted.workflow, run_id);

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
        return Err(PaneContextError::PaneUnavailable);
    }
    Err(PaneContextError::PaneUnavailable)
}

#[derive(Debug, thiserror::Error)]
enum PaneContextError {
    #[error("run not found")]
    RunNotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("pane unavailable")]
    PaneUnavailable,
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl PaneContextError {
    fn client_message(&self) -> &'static str {
        match self {
            Self::RunNotFound => "run not found",
            Self::Unauthorized => "unauthorized",
            Self::PaneUnavailable | Self::Internal(_) => "pane unavailable",
        }
    }
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
) -> Result<broadcast::Receiver<Vec<u8>>, PaneStreamSubscribeError> {
    let mut streams = state.pane_streams.inner.lock().await;
    let subscriber_limit = state.pane_streams.max_subscribers_per_pane;
    if let Some(entry) = streams.get_mut(pane_target) {
        if entry.refcount >= subscriber_limit {
            return Err(PaneStreamSubscribeError::TooManySubscribers {
                limit: subscriber_limit,
            });
        }
        entry.refcount = entry.refcount.saturating_add(1);
        return Ok(entry.sender.subscribe());
    }

    if subscriber_limit == 0 {
        return Err(PaneStreamSubscribeError::TooManySubscribers {
            limit: subscriber_limit,
        });
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

    Ok(receiver)
}

#[derive(Debug, thiserror::Error)]
enum PaneStreamSubscribeError {
    #[error("too many pane stream subscribers (limit: {limit})")]
    TooManySubscribers { limit: usize },
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

        cleanup_pane_stream_task_exit(&state.pane_streams, &pane_target, &sender, exit).await;
    });
}

async fn cleanup_pane_stream_task_exit(
    pane_streams: &crate::app::PaneStreamRegistry,
    pane_target: &str,
    sender: &broadcast::Sender<Vec<u8>>,
    exit: PaneStreamTaskExit,
) {
    match exit {
        PaneStreamTaskExit::NoSubscribers | PaneStreamTaskExit::Terminal => {
            pane_streams
                .remove_terminal_sender(pane_target, sender)
                .await;
        }
    }
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
    Ok(Json(run_action_response(&state, &run_id).await?))
}

async fn restart_run(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let new_run_id = state.runtime.restart_from(&run_id, &node_id).await?;
    Ok(Json(run_action_response(&state, &new_run_id).await?))
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
    let enriched = try_join_all(
        runs.into_iter()
            .map(|run| enrich_interrupted_run(&state, run)),
    )
    .await?;
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
        None => Err(ApiError::not_found("Not found")),
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
    match serde_json::from_value::<WorkflowNode>(value.clone()) {
        Ok(node) => {
            if matches!(&node.kind, NodeKind::Task { .. } | NodeKind::Approval) {
                return Ok(node);
            }
            anyhow::bail!(
                "node payload must use canonical v3 task or approval kind types; got {}",
                node.kind.as_str()
            )
        }
        Err(err) => {
            // Give a targeted hint when the caller used the obsolete flat shape
            // (top-level `type` with no `kind` wrapper).
            if value.get("type").is_some() && value.get("kind").is_none() {
                anyhow::bail!(
                    "node uses the legacy flat shape; migrate to the canonical v3 `kind: {{ type, ... }}` form"
                );
            }
            Err(anyhow::anyhow!(err).context("node payload must be a canonical v3 WorkflowNode"))
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ApiError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{message}")]
    Validation { status: StatusCode, message: String },
    #[error("{message}")]
    ValidationBody {
        status: StatusCode,
        message: String,
        body: Value,
    },
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
}

impl ApiError {
    fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::Validation {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unprocessable(message: impl Into<String>) -> Self {
        Self::Validation {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn validation_body(status: StatusCode, body: Value) -> Self {
        let message = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("validation error")
            .to_string();
        Self::ValidationBody {
            status,
            message,
            body,
        }
    }

    fn internal(error: impl Into<anyhow::Error>) -> Self {
        Self::Internal(error.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(error)
    }
}

impl From<RunControlError> for ApiError {
    fn from(error: RunControlError) -> Self {
        match error {
            RunControlError::RunNotFound | RunControlError::NoActiveInteraction => {
                Self::not_found(error.to_string())
            }
            RunControlError::TerminalState | RunControlError::StaleInteractionSession => {
                Self::conflict(error.to_string())
            }
            RunControlError::NodeNotFound => Self::unprocessable(error.to_string()),
            RunControlError::Internal(error) => Self::internal(error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, body) = match self {
            Self::NotFound(message) => (StatusCode::NOT_FOUND, json!({ "error": message })),
            Self::Conflict(message) => (StatusCode::CONFLICT, json!({ "error": message })),
            Self::Validation { status, message } => (status, json!({ "error": message })),
            Self::ValidationBody { status, body, .. } => (status, body),
            Self::Internal(error) => {
                tracing::warn!(error = ?error, "internal API error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": "internal error" }),
                )
            }
        };
        (status, Json(body)).into_response()
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
        app::{
            AppPaths, AppState, PaneStreamRegistry, SecurityConfig, sha256_unlock_password_hash,
        },
        model::{RunAsConfig, SplitFailurePolicy, WorkflowLimits, WorkflowNodeType},
        runtime::{
            AgentExecutionMetadata, ExecutionLog, NodeExecutionLog, NodeResult, RuntimeStatus,
        },
        storage::{Database, TemplateStore, WorkflowStore},
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

    fn test_app_state(pane_streams: PaneStreamRegistry) -> AppState {
        let paths = AppPaths::from_root(std::env::temp_dir().join("silverbond-api-tests"));
        AppState {
            paths: paths.clone(),
            workflows: WorkflowStore::new(paths.workflows_dir.clone()),
            templates: TemplateStore::new(paths.templates_dir.clone()),
            runtime: crate::runtime::RuntimeContext::new(Database::new(
                paths.database_path.clone(),
            )),
            pane_streams,
            security: SecurityConfig::default(),
        }
    }

    fn approval_workflow_with_run_as(command: Vec<String>) -> WorkflowV3 {
        WorkflowV3 {
            version: 3,
            name: Some("observability-test".to_string()),
            goal: "wait for approval".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: Some(RunAsConfig {
                user: None,
                command: Some(command),
                socket: Some("observability-test".to_string()),
            }),
            entry_node_id: "approve".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 5,
                max_visits_per_node: 5,
            },
            nodes: vec![WorkflowNode {
                id: "approve".to_string(),
                name: "Approve".to_string(),
                kind: WorkflowNodeType::Approval.into(),
                agent: None,
                prompt: "continue?".to_string(),
                context_sources: Vec::new(),
                response_format: None,
                output_schema: None,
                retry_count: None,
                retry_delay: None,
                timeout: None,
                skip_condition: None,
                loop_max_iterations: None,
                loop_condition: None,
                split_failure_policy: SplitFailurePolicy::BestEffortContinue,
                cwd: None,
                continue_session_from: None,
            }],
            edges: Vec::new(),
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        }
    }

    fn approval_only_workflow() -> WorkflowV3 {
        WorkflowV3 {
            version: 3,
            name: Some("approval-only".to_string()),
            goal: "wait for approval".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: "approve".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 5,
                max_visits_per_node: 5,
            },
            nodes: vec![WorkflowNode {
                id: "approve".to_string(),
                name: "Approve".to_string(),
                kind: WorkflowNodeType::Approval.into(),
                agent: None,
                prompt: "continue?".to_string(),
                context_sources: Vec::new(),
                response_format: None,
                output_schema: None,
                retry_count: None,
                retry_delay: None,
                timeout: None,
                skip_condition: None,
                loop_max_iterations: None,
                loop_condition: None,
                split_failure_policy: SplitFailurePolicy::BestEffortContinue,
                cwd: None,
                continue_session_from: None,
            }],
            edges: Vec::new(),
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        }
    }

    fn approval_workflow_with_unreachable_task() -> WorkflowV3 {
        let mut workflow = approval_only_workflow();
        workflow.name = Some("privileged-process-test".to_string());
        workflow.nodes.push(WorkflowNode {
            id: "task".to_string(),
            name: "Unreachable Task".to_string(),
            kind: WorkflowNodeType::Task.into(),
            agent: Some("codex".to_string()),
            prompt: "do work".to_string(),
            context_sources: Vec::new(),
            response_format: None,
            output_schema: None,
            retry_count: None,
            retry_delay: None,
            timeout: None,
            skip_condition: None,
            loop_max_iterations: None,
            loop_condition: None,
            split_failure_policy: SplitFailurePolicy::BestEffortContinue,
            cwd: None,
            continue_session_from: None,
        });
        workflow
    }

    async fn create_test_state(security: SecurityConfig) -> (tempfile::TempDir, AppState) {
        let temp = tempfile::TempDir::new().unwrap();
        let paths = AppPaths::from_root(temp.path());
        let db = Database::new(paths.database_path.clone());
        db.init().await.unwrap();
        let state = AppState {
            paths: paths.clone(),
            workflows: WorkflowStore::new(paths.workflows_dir.clone()),
            templates: TemplateStore::new(paths.templates_dir.clone()),
            runtime: crate::runtime::RuntimeContext::new(db),
            pane_streams: PaneStreamRegistry::default(),
            security,
        };
        (temp, state)
    }

    fn create_run_request(workflow: WorkflowV3, unlock_secret: Option<&str>) -> CreateRunRequest {
        CreateRunRequest {
            workflow: serde_json::to_value(workflow).unwrap(),
            variable_overrides: BTreeMap::new(),
            start_node_id: None,
            unlock_secret: unlock_secret.map(str::to_owned),
        }
    }

    fn assert_privileged_unlock_required(error: ApiError) {
        let ApiError::ValidationBody { status, body, .. } = error else {
            panic!("expected privileged unlock error, got {error:?}");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "privileged_unlock_required");
    }

    #[tokio::test]
    async fn create_run_rejects_privileged_node_without_unlock_secret() {
        let security = SecurityConfig {
            agent_user: None,
            unlock_password_hash: Some(sha256_unlock_password_hash("correct")),
        };
        let (_temp, state) = create_test_state(security).await;

        let error = create_run(
            State(state),
            Json(create_run_request(
                approval_workflow_with_unreachable_task(),
                None,
            )),
        )
        .await
        .unwrap_err();

        assert_privileged_unlock_required(error);
    }

    #[tokio::test]
    async fn create_run_accepts_privileged_node_with_correct_unlock_secret() {
        let security = SecurityConfig {
            agent_user: None,
            unlock_password_hash: Some(sha256_unlock_password_hash("correct")),
        };
        let (_temp, state) = create_test_state(security).await;

        let Json(response) = create_run(
            State(state),
            Json(create_run_request(
                approval_workflow_with_unreachable_task(),
                Some("correct"),
            )),
        )
        .await
        .unwrap();

        assert_eq!(response["success"], true);
        assert!(
            response["runId"]
                .as_str()
                .is_some_and(|run_id| !run_id.is_empty())
        );
        assert!(
            response["streamToken"]
                .as_str()
                .is_some_and(|token| token.len() >= 32)
        );
    }

    #[tokio::test]
    async fn create_run_accepts_unprivileged_workflow_without_unlock_secret() {
        let security = SecurityConfig {
            agent_user: Some("agent".to_string()),
            unlock_password_hash: Some(sha256_unlock_password_hash("correct")),
        };
        let (_temp, state) = create_test_state(security).await;

        let Json(response) = create_run(
            State(state),
            Json(create_run_request(approval_only_workflow(), None)),
        )
        .await
        .unwrap();

        assert_eq!(response["success"], true);
        assert!(
            response["runId"]
                .as_str()
                .is_some_and(|run_id| !run_id.is_empty())
        );
        assert!(
            response["streamToken"]
                .as_str()
                .is_some_and(|token| token.len() >= 32)
        );
    }

    async fn wait_for_pending_approval(db: &Database, run_id: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(persisted) = db.get_run(run_id).await.unwrap() {
                if persisted.checkpoint.pending_approval.is_some() {
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for run {run_id} to require approval"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
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

    #[cfg(unix)]
    #[tokio::test]
    async fn interrupted_runs_resolves_tmux_panes_concurrently() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let paths = AppPaths::from_root(temp.path());
        let db = Database::new(paths.database_path.clone());
        db.init().await.unwrap();
        let state = AppState {
            paths: paths.clone(),
            workflows: WorkflowStore::new(paths.workflows_dir.clone()),
            templates: TemplateStore::new(paths.templates_dir.clone()),
            runtime: crate::runtime::RuntimeContext::new(db.clone()),
            pane_streams: PaneStreamRegistry::default(),
            security: SecurityConfig::default(),
        };

        let script = temp.path().join("fake-tmux-prefix.sh");
        let log = temp.path().join("display.log");
        std::fs::write(
            &script,
            r#"#!/bin/sh
log=$1
delay=$2
shift 2
if [ "$1" = "zsh" ]; then
  printf '%s\n' 'zshrc banner'
  printf '%s\n' 'SBTMUX:fake-tmux'
  exit 0
fi
printf '%s\n' "$*" >> "$log"
sleep "$delay"
target=""
previous=""
for arg in "$@"; do
  if [ "$previous" = "-t" ]; then
    target=$arg
  fi
  previous=$arg
done
printf 'session-%s\n' "$target"
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let command = vec![
            script.to_string_lossy().into_owned(),
            log.to_string_lossy().into_owned(),
            "0.8".to_string(),
        ];
        let first_run = state
            .runtime
            .start_run(
                approval_workflow_with_run_as(command.clone()),
                BTreeMap::new(),
                None,
            )
            .await
            .unwrap();
        let second_run = state
            .runtime
            .start_run(
                approval_workflow_with_run_as(command),
                BTreeMap::new(),
                None,
            )
            .await
            .unwrap();
        wait_for_pending_approval(&db, &first_run).await;
        wait_for_pending_approval(&db, &second_run).await;
        state
            .runtime
            .registry
            .set_active_pane_with_session(&first_run, "pane-a", "%pane-a", None)
            .await;
        state
            .runtime
            .registry
            .set_active_pane_with_session(&second_run, "pane-b", "%pane-b", None)
            .await;

        let started = Instant::now();
        let Json(value) = interrupted_runs(State(state.clone())).await.unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_millis(1400),
            "interrupted_runs should resolve pane sessions concurrently; elapsed={elapsed:?}"
        );
        assert_eq!(value.as_array().map(Vec::len), Some(2));
        let recorded = std::fs::read_to_string(log).unwrap();
        assert_eq!(
            recorded
                .lines()
                .filter(|line| line.contains("display-message"))
                .count(),
            2
        );
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
    async fn pane_context_errors_use_sanitized_client_messages() {
        let temp = tempfile::TempDir::new().unwrap();
        let paths = AppPaths::from_root(temp.path());
        let db = Database::new(paths.database_path.clone());
        db.init().await.unwrap();
        let state = AppState {
            paths: paths.clone(),
            workflows: WorkflowStore::new(paths.workflows_dir.clone()),
            templates: TemplateStore::new(paths.templates_dir.clone()),
            runtime: crate::runtime::RuntimeContext::new(db.clone()),
            pane_streams: PaneStreamRegistry::default(),
            security: SecurityConfig::default(),
        };

        let missing_run_id = "run_secret_missing";
        let missing = resolve_run_pane_context(&state, missing_run_id, "%secret-pane", "any-token")
            .await
            .unwrap_err();
        assert!(matches!(missing, PaneContextError::RunNotFound));
        assert_eq!(missing.client_message(), "run not found");
        assert!(!missing.client_message().contains(missing_run_id));

        let run_id = state
            .runtime
            .start_run(
                approval_workflow_with_run_as(vec![
                    "/tmp/secret-tmux-prefix".to_string(),
                    "%secret-target".to_string(),
                ]),
                BTreeMap::new(),
                None,
            )
            .await
            .unwrap();
        wait_for_pending_approval(&db, &run_id).await;
        let stream_token = db.get_run(&run_id).await.unwrap().unwrap().stream_token;

        let pane_name = "%secret-pane";
        let unauthorized = resolve_run_pane_context(&state, &run_id, pane_name, "wrong-token")
            .await
            .unwrap_err();
        assert!(matches!(unauthorized, PaneContextError::Unauthorized));
        assert_eq!(unauthorized.client_message(), "unauthorized");

        let unavailable = resolve_run_pane_context(&state, &run_id, pane_name, &stream_token)
            .await
            .unwrap_err();
        assert!(matches!(unavailable, PaneContextError::PaneUnavailable));
        assert_eq!(unavailable.client_message(), "pane unavailable");
        assert!(!unavailable.client_message().contains(&run_id));
        assert!(!unavailable.client_message().contains(pane_name));
        assert!(!unavailable.client_message().contains("%secret-target"));
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
    async fn subscribe_pane_stream_rejects_when_subscriber_limit_reached() {
        let mut registry = PaneStreamRegistry::default();
        registry.max_subscribers_per_pane = 1;
        let (sender, _receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(
                "%1".to_string(),
                PaneStreamEntry {
                    sender: sender.clone(),
                    refcount: 1,
                },
            );
        }
        let state = test_app_state(registry.clone());

        let result = subscribe_pane_stream(
            &state,
            "run-test",
            "pane-test",
            "%1",
            &TmuxInvocation::default(),
        )
        .await;

        assert!(matches!(
            result,
            Err(PaneStreamSubscribeError::TooManySubscribers { limit: 1 })
        ));
        assert_eq!(
            registry
                .inner
                .lock()
                .await
                .get("%1")
                .map(|entry| entry.refcount),
            Some(1)
        );
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

    #[tokio::test]
    async fn pane_stream_task_no_subscribers_uses_terminal_sender_cleanup() {
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

        cleanup_pane_stream_task_exit(&registry, "%1", &sender, PaneStreamTaskExit::NoSubscribers)
            .await;
        assert!(!registry.inner.lock().await.contains_key("%1"));

        let (new_sender, _new_receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(
                "%1".to_string(),
                PaneStreamEntry {
                    sender: new_sender.clone(),
                    refcount: 1,
                },
            );
        }

        cleanup_pane_stream_task_exit(&registry, "%1", &sender, PaneStreamTaskExit::NoSubscribers)
            .await;
        let streams = registry.inner.lock().await;
        let entry = streams.get("%1").expect("new pane stream entry remains");
        assert!(entry.sender.same_channel(&new_sender));
        assert_eq!(entry.refcount, 1);
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
            tmux_sessions: BTreeSet::new(),
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
