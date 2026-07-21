use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path as FsPath, PathBuf},
    pin::Pin,
    process::{Command, Stdio},
    task::{Context as TaskContext, Poll, ready},
    time::Duration,
};

use anyhow::Context;
use async_stream::stream;
use axum::{
    Json, Router,
    extract::{
        Path, Request, State,
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
use futures::{SinkExt, Stream, StreamExt, future::try_join_all, stream::SplitSink};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tmux_tools_core::{
    TmuxInvocation,
    stream::{CaptureAnsiOpts, capture_ansi},
    tmux, with_invocation,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, ReadBuf, unix::AsyncFd},
    sync::{broadcast, oneshot, watch},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::{
    app::{AppState, PaneStreamEntry, PaneStreamKey, SecurityConfig, UnlockThrottle},
    model::{
        NodeKind, RunAsConfig, WORKFLOW_SCHEMA_VERSION, WorkflowLimits, WorkflowNode, WorkflowV3,
        normalize_workflow_value, validate_workflow, validate_workflow_input_bounds,
    },
    runtime::{
        InterruptedRunSummary, NodeTestContext, PaneCandidate, PersistedRun, RunControlError,
        RuntimeCheckpoint, RuntimeEvent, RuntimeStatus, available_agents, check_cli,
        load_or_resolve_run_tmux_invocation, run_node_preview,
    },
    util::constant_time_eq,
};

const PANE_STREAM_HEARTBEAT: Duration = Duration::from_secs(5);
const PANE_STREAM_SEND_TIMEOUT: Duration = Duration::from_secs(2);
const PANE_STREAM_PENDING_TIMEOUT: Duration = Duration::from_secs(120);
const PANE_STREAM_SETUP_TIMEOUT: Duration = Duration::from_secs(6);
const PANE_STREAM_READ_MAX_RETRIES: usize = 5;
const PANE_STREAM_READ_RETRY_DELAY: Duration = Duration::from_millis(100);
const PANE_SESSION_LOOKUP_CONCURRENCY: usize = 4;
const STREAM_TOKEN_HEADER: &str = "x-stream-token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneStreamTaskExit {
    NoSubscribers,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneStreamOwnerAction {
    Continue,
    Teardown,
    Stale,
}

struct PaneStreamGuard {
    state: Option<AppState>,
    key: PaneStreamKey,
    sender: broadcast::WeakSender<Vec<u8>>,
}

impl PaneStreamGuard {
    fn new(state: AppState, key: PaneStreamKey, sender: broadcast::Sender<Vec<u8>>) -> Self {
        Self {
            state: Some(state),
            key,
            sender: sender.downgrade(),
        }
    }

    fn unsubscribe(mut self) {
        self.spawn_unsubscribe();
    }

    fn spawn_unsubscribe(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        let key = self.key.clone();
        let sender = self.sender.clone();
        tokio::spawn(async move {
            if let Some(sender) = sender.upgrade() {
                state.pane_streams.unsubscribe(&key, &sender).await;
            }
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
        .route("/api/runs/{run_id}/events", get(run_events))
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
                "accessProfiles": crate::driver::agent_access_profile_names(&spec.name),
            }),
        );
    }
    Ok(Json(json!({
        "workflowVersion": WORKFLOW_SCHEMA_VERSION,
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

async fn run_tmux_invocation(
    state: &AppState,
    persisted: &PersistedRun,
) -> anyhow::Result<TmuxInvocation> {
    load_or_resolve_run_tmux_invocation(&state.runtime.db, persisted, None).await
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
    invocation: &TmuxInvocation,
) -> Option<String> {
    session_name_from_active_pane(
        state,
        run_id,
        invocation,
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
    let Ok(invocation) = run_tmux_invocation(state, &persisted).await else {
        return fields;
    };
    let mut pane_entries = state.runtime.registry.active_pane_entries(run_id).await;
    pane_entries.sort_by_key(|entry| entry.sequence);
    let registered_session_name = if pane_entries
        .iter()
        .any(|entry| entry.session_name.is_none())
    {
        state
            .runtime
            .db
            .list_run_tmux_session_names(run_id)
            .await
            .ok()
            .and_then(|session_names| match session_names.as_slice() {
                [session_name] => Some(session_name.clone()),
                _ => None,
            })
    } else {
        None
    };

    let pane_lookups = pane_entries.into_iter().map(|entry| {
        let invocation = invocation.clone();
        let registered_session_name = registered_session_name.clone();
        async move {
            let session_name = session_name_from_pane_target(
                &invocation,
                &entry.target,
                entry
                    .session_name
                    .as_deref()
                    .or(registered_session_name.as_deref()),
            )
            .await?;
            Some((
                entry.sequence,
                json!({
                    "pane": entry.key,
                    "sessionName": session_name,
                    "attachCommand": build_attach_command(&invocation, &session_name),
                }),
            ))
        }
    });
    let mut panes = futures::stream::iter(pane_lookups)
        .buffer_unordered(PANE_SESSION_LOOKUP_CONCURRENCY)
        .filter_map(|pane| async move { pane })
        .collect::<Vec<_>>()
        .await;
    panes.sort_by_key(|(sequence, _)| *sequence);
    let panes = panes.into_iter().map(|(_, pane)| pane).collect::<Vec<_>>();

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
        let session_name = resolve_run_session_name(state, run_id, &persisted, &invocation).await;
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
    enforce_workflow_input_bounds(&normalized.workflow)?;
    let mut result = validate_workflow(normalized.workflow);
    result.notices = normalized.notices;
    Ok(Json(serde_json::to_value(result)?))
}

fn enforce_workflow_input_bounds(workflow: &WorkflowV3) -> Result<(), ApiError> {
    validate_workflow_input_bounds(workflow).map_err(|issue| {
        ApiError::validation_body(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "error": "Validation failed",
                "details": [issue],
            }),
        )
    })
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
    #[serde(default)]
    unlock_secret: Option<String>,
}

async fn test_node(
    State(state): State<AppState>,
    Json(request): Json<TestStepRequest>,
) -> Result<Json<Value>, ApiError> {
    let node_value = request
        .node
        .context("node is required")
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let node =
        node_from_value(node_value).map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !matches!(&node.kind, NodeKind::Task { .. }) {
        return Err(ApiError::bad_request("Only task nodes can be tested"));
    }
    let cwd = request.cwd.unwrap_or_default();
    let run_as = authorize_and_prepare_node_preview_security(
        &node,
        &cwd,
        &state.security,
        &state.unlock_throttle,
        request.unlock_secret.as_deref(),
    )
    .await?;
    let preview_id = format!("preview_{}", uuid::Uuid::now_v7());
    let invocation = tokio::task::spawn_blocking(move || {
        crate::tmux_exec::build_tmux_invocation(&run_as, &preview_id)
    })
    .await
    .context("join error while preparing node preview security")?;
    let preview = run_node_preview(
        &node,
        &cwd,
        request.mock_context.unwrap_or_default(),
        invocation,
    )
    .await?;
    Ok(Json(serde_json::to_value(preview)?))
}

async fn authorize_and_prepare_node_preview_security(
    node: &WorkflowNode,
    cwd: &str,
    security: &SecurityConfig,
    unlock_throttle: &UnlockThrottle,
    unlock_secret: Option<&str>,
) -> Result<RunAsConfig, ApiError> {
    let mut workflow = WorkflowV3 {
        version: WORKFLOW_SCHEMA_VERSION,
        name: Some("Node preview".to_string()),
        goal: "Preview one workflow node".to_string(),
        cwd: cwd.to_string(),
        use_orchestrator: false,
        run_as: None,
        entry_node_id: node.id.clone(),
        variables: Vec::new(),
        limits: WorkflowLimits::default(),
        nodes: vec![node.clone()],
        edges: Vec::new(),
        agent_defaults: BTreeMap::new(),
        subflows: BTreeMap::new(),
        ui: None,
    };
    authorize_and_prepare_run_security(&mut workflow, security, unlock_throttle, unlock_secret)
        .await?;
    Ok(workflow.run_as.unwrap_or_default())
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
    authorize_and_prepare_run_security(
        &mut normalized.workflow,
        &state.security,
        &state.unlock_throttle,
        request.unlock_secret.as_deref(),
    )
    .await?;
    enforce_workflow_input_bounds(&normalized.workflow)?;
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

async fn authorize_and_prepare_run_security(
    workflow: &mut WorkflowV3,
    security: &SecurityConfig,
    unlock_throttle: &UnlockThrottle,
    unlock_secret: Option<&str>,
) -> Result<(), ApiError> {
    let launches_processes = workflow_launches_processes(workflow);
    let uses_run_as_prefix = workflow.run_as.is_some();
    if (launches_processes || uses_run_as_prefix)
        && resolved_run_as_is_privileged(workflow.run_as.as_ref(), security.agent_user.as_deref())
    {
        if security.unlock_password_hash.is_none() {
            return Err(unlock_not_configured());
        }
        let Some(unlock_secret) = unlock_secret else {
            return Err(privileged_unlock_required());
        };
        match unlock_throttle
            .verify_unlock_secret(security, unlock_secret)
            .await
        {
            Ok(true) => {}
            Ok(false) => return Err(privileged_unlock_required()),
            Err(retry_delay) => return Err(unlock_throttled(retry_delay)),
        }
    }

    if launches_processes && workflow.run_as.is_none() {
        if let Some(agent_user) = security.agent_user.as_deref() {
            workflow.run_as = Some(RunAsConfig {
                user: Some(agent_user.to_owned()),
                command: None,
            });
        }
    }

    Ok(())
}

fn unlock_not_configured() -> ApiError {
    ApiError::validation_body(
        StatusCode::FORBIDDEN,
        json!({
            "error": "Privileged run unlock is not configured. Set SILVERBOND_UNLOCK_PASSWORD_HASH to require an unlock password, or set SILVERBOND_AGENT_USER to run agents as a low-privilege user.",
            "code": "unlock_not_configured",
        }),
    )
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

fn unlock_throttled(retry_delay: Duration) -> ApiError {
    let retry_after_seconds = retry_delay.as_secs() + u64::from(retry_delay.subsec_nanos() > 0);
    ApiError::validation_body(
        StatusCode::TOO_MANY_REQUESTS,
        json!({
            "error": "Privileged run unlock is temporarily throttled after failed attempts.",
            "code": "unlock_throttled",
            "retryAfterSeconds": retry_after_seconds,
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
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let token_candidates = header_stream_token_candidates(&headers, STREAM_TOKEN_HEADER);
    require_run_stream_token(&state, &run_id, &token_candidates).await?;
    let receiver = state.runtime.registry.subscribe(&run_id).await;
    let replay = state.runtime.db.list_events(&run_id).await?;
    let db = state.runtime.db.clone();
    let run_id_for_stream = run_id.clone();
    let stream = stream! {
        let mut done_seen = false;
        let mut max_seq = 0u64;

        for event in replay {
            let seq = runtime_event_seq(&event);
            if seq > max_seq {
                max_seq = seq;
            }
            if event.kind == "done" {
                done_seen = true;
            }
            yield Ok(run_stream_sse_event(&event));
        }

        if done_seen {
            return;
        }

        if let Some(mut receiver) = receiver {
            for item in drain_buffered_run_events(&mut receiver, &mut max_seq) {
                match item {
                    RunStreamBufferedItem::Resync => {
                        yield Ok(run_stream_resync_sse_event());
                        match resync_run_stream_from_journal(&db, &run_id_for_stream, &mut max_seq).await {
                            Ok(events) => {
                                for item in events {
                                    if item.is_done {
                                        yield Ok(item.sse);
                                        return;
                                    }
                                    yield Ok(item.sse);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    RunStreamBufferedItem::Event { sse, is_done } => {
                        if is_done {
                            yield Ok(sse);
                            return;
                        }
                        yield Ok(sse);
                    }
                }
            }

            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let seq = runtime_event_seq(&event);
                        if seq <= max_seq {
                            continue;
                        }
                        max_seq = seq;
                        let is_done = event.kind == "done";
                        yield Ok(run_stream_sse_event(&event));
                        if is_done {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        yield Ok(run_stream_resync_sse_event());
                        match resync_run_stream_from_journal(&db, &run_id_for_stream, &mut max_seq).await {
                            Ok(events) => {
                                for item in events {
                                    if item.is_done {
                                        yield Ok(item.sse);
                                        return;
                                    }
                                    yield Ok(item.sse);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

const RUN_STREAM_RESYNC_EVENT: &str = "stream_resync";

fn runtime_event_seq(event: &RuntimeEvent) -> u64 {
    event.seq.unwrap_or(0)
}

fn run_stream_sse_event(event: &RuntimeEvent) -> Event {
    let payload = serde_json::to_string(event).unwrap_or_default();
    let mut sse = Event::default().data(payload);
    let seq = runtime_event_seq(event);
    if seq > 0 {
        sse = sse.id(seq.to_string());
    }
    sse
}

fn run_stream_resync_sse_event() -> Event {
    Event::default().data(
        serde_json::json!({ "type": RUN_STREAM_RESYNC_EVENT })
            .to_string(),
    )
}

struct RunStreamJournalItem {
    sse: Event,
    is_done: bool,
}

enum RunStreamBufferedItem {
    Event { sse: Event, is_done: bool },
    Resync,
}

fn drain_buffered_run_events(
    receiver: &mut tokio::sync::broadcast::Receiver<RuntimeEvent>,
    max_seq: &mut u64,
) -> Vec<RunStreamBufferedItem> {
    use tokio::sync::broadcast::error::TryRecvError;
    let mut out = Vec::new();
    loop {
        match receiver.try_recv() {
            Ok(event) => {
                let seq = runtime_event_seq(&event);
                if seq <= *max_seq {
                    continue;
                }
                *max_seq = seq;
                let is_done = event.kind == "done";
                out.push(RunStreamBufferedItem::Event {
                    sse: run_stream_sse_event(&event),
                    is_done,
                });
            }
            Err(TryRecvError::Lagged(_)) => {
                out.push(RunStreamBufferedItem::Resync);
                break;
            }
            Err(TryRecvError::Closed) | Err(TryRecvError::Empty) => break,
        }
    }
    out
}

async fn resync_run_stream_from_journal(
    db: &crate::storage::Database,
    run_id: &str,
    max_seq: &mut u64,
) -> anyhow::Result<Vec<RunStreamJournalItem>> {
    let replay = db.list_events(run_id).await?;
    let mut out = Vec::new();
    for event in replay {
        let seq = runtime_event_seq(&event);
        if seq <= *max_seq {
            continue;
        }
        *max_seq = seq;
        let is_done = event.kind == "done";
        out.push(RunStreamJournalItem {
            sse: run_stream_sse_event(&event),
            is_done,
        });
    }
    Ok(out)
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

fn stream_token_forbidden() -> ApiError {
    ApiError::validation_body(
        StatusCode::FORBIDDEN,
        json!({ "error": "stream token required" }),
    )
}

fn websocket_stream_token_candidates(headers: &HeaderMap) -> Vec<String> {
    header_stream_token_candidates(headers, "sec-websocket-protocol")
}

fn header_stream_token_candidates(headers: &HeaderMap, header_name: &'static str) -> Vec<String> {
    headers
        .get_all(header_name)
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
        .ok_or_else(stream_token_forbidden)?;
    let Some(stream_token) = candidates
        .iter()
        .find(|candidate| constant_time_eq(candidate.as_bytes(), persisted.stream_token.as_bytes()))
        .cloned()
    else {
        return Err(stream_token_forbidden());
    };

    Ok((persisted, stream_token))
}

async fn pane_stream_socket(
    socket: WebSocket,
    state: AppState,
    run_id: String,
    pane: String,
    stream_token: String,
) -> anyhow::Result<()> {
    let (mut sender, mut receiver) = socket.split();
    let mut next_seq = 0_u64;

    let (pane_target, invocation) = match resolve_or_wait_for_pane_context(
        &state,
        &run_id,
        &pane,
        &stream_token,
        &mut sender,
        &mut receiver,
        &mut next_seq,
    )
    .await
    {
        Ok(context) => context,
        Err(PaneContextError::ClientDisconnected) => return Ok(()),
        Err(error) => {
            if matches!(error, PaneContextError::Internal(_)) {
                tracing::warn!(
                    run_id = %run_id,
                    pane = %pane,
                    error = ?error,
                    "failed to resolve pane websocket context"
                );
            }
            let _ = send_error_frame(&mut sender, &mut next_seq, error.client_message()).await;
            let _ = sender.close().await;
            return Ok(());
        }
    };

    let (mut pane_receiver, pane_sender, pane_stream_key) =
        match subscribe_pane_stream(&state, &run_id, &pane, &pane_target, &invocation).await {
            Ok(subscription) => subscription,
            Err(error) => {
                let _ = send_error_frame(&mut sender, &mut next_seq, &error.to_string()).await;
                let _ = sender.close().await;
                return Ok(());
            }
        };
    let pane_stream_guard = PaneStreamGuard::new(state.clone(), pane_stream_key, pane_sender);

    // Snapshot capture and live pipe-pane bytes arrive through independent channels, so there is
    // no atomic boundary: bytes rendered here may be replayed from the live buffer. "Clear and
    // repaint" is the user remedy. See H3's control-mode rejection before revisiting the transport.
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
    if !constant_time_eq(stream_token.as_bytes(), persisted.stream_token.as_bytes()) {
        return Err(PaneContextError::Unauthorized);
    }
    let invocation = run_tmux_invocation(state, &persisted)
        .await
        .map_err(PaneContextError::Internal)?;

    if let Some(target) = state
        .runtime
        .registry
        .resolve_active_pane(run_id, pane)
        .await
    {
        return Ok((target, invocation));
    }

    let candidates = persisted.checkpoint.pane_candidates();
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

async fn run_still_awaiting_pane(
    state: &AppState,
    run_id: &str,
) -> Result<bool, PaneContextError> {
    let persisted = state
        .runtime
        .db
        .get_run(run_id)
        .await
        .map_err(PaneContextError::Internal)?
        .ok_or(PaneContextError::RunNotFound)?;
    Ok(matches!(
        persisted.checkpoint.status,
        RuntimeStatus::Running | RuntimeStatus::Paused
    ))
}

async fn resolve_or_wait_for_pane_context<RS>(
    state: &AppState,
    run_id: &str,
    pane: &str,
    stream_token: &str,
    sender: &mut SplitSink<WebSocket, Message>,
    receiver: &mut RS,
    next_seq: &mut u64,
) -> Result<(String, TmuxInvocation), PaneContextError>
where
    RS: Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    let deadline = Instant::now() + PANE_STREAM_PENDING_TIMEOUT;
    let mut heartbeat = Box::pin(tokio::time::sleep(PANE_STREAM_HEARTBEAT));

    loop {
        match resolve_run_pane_context(state, run_id, pane, stream_token).await {
            Ok(context) => return Ok(context),
            Err(PaneContextError::PaneUnavailable) => {
                if !run_still_awaiting_pane(state, run_id).await? {
                    return Err(PaneContextError::PaneUnavailable);
                }
                if Instant::now() >= deadline {
                    return Err(PaneContextError::PaneUnavailable);
                }
            }
            Err(error) => return Err(error),
        }

        let pane_notify = state.runtime.registry.active_pane_notify(run_id).await;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait_for_pane = async {
            if let Some(notify) = pane_notify {
                let notified = notify.notified();
                tokio::pin!(notified);
                let _ = tokio::time::timeout(remaining, notified).await;
            } else {
                let poll = std::cmp::min(remaining, Duration::from_millis(250));
                tokio::time::sleep(poll).await;
            }
        };

        tokio::select! {
            client_message = receiver.next() => {
                match client_message {
                    Some(Ok(message)) if is_resync_request(&message) => {
                        heartbeat.as_mut().reset(Instant::now() + PANE_STREAM_HEARTBEAT);
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        return Err(PaneContextError::ClientDisconnected);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::debug!(
                            run_id = %run_id,
                            pane = %pane,
                            error = %error,
                            "pane websocket receive failed while waiting for pane"
                        );
                        return Err(PaneContextError::ClientDisconnected);
                    }
                }
            }
            _ = &mut heartbeat => {
                match send_heartbeat(sender, current_seq(*next_seq)).await {
                    Ok(()) => heartbeat.as_mut().reset(Instant::now() + PANE_STREAM_HEARTBEAT),
                    Err(error) => {
                        tracing::debug!(
                            run_id = %run_id,
                            pane = %pane,
                            error = %error,
                            "pane websocket heartbeat failed while waiting for pane"
                        );
                        return Err(PaneContextError::ClientDisconnected);
                    }
                }
            }
            _ = wait_for_pane => {}
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum PaneContextError {
    #[error("run not found")]
    RunNotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("pane unavailable")]
    PaneUnavailable,
    #[error("client disconnected")]
    ClientDisconnected,
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl PaneContextError {
    fn client_message(&self) -> &'static str {
        match self {
            Self::RunNotFound => "run not found",
            Self::Unauthorized => "unauthorized",
            Self::PaneUnavailable | Self::Internal(_) | Self::ClientDisconnected => "pane unavailable",
        }
    }
}

async fn start_pane_stream(
    root: &FsPath,
    pane_target: &str,
    invocation: &TmuxInvocation,
) -> anyhow::Result<PaneStream> {
    let setup_deadline = Instant::now() + PANE_STREAM_SETUP_TIMEOUT;
    let root = root.to_path_buf();
    let setup_pane_target = pane_target.to_string();
    let setup_invocation = invocation.clone();
    let cross_user = !invocation.prefix.is_empty();
    let data_path = pane_stream_fifo_path(&root, "fifo");
    let termination_path = pane_stream_fifo_path(&root, "done.fifo");
    let setup_data_path = data_path.clone();
    let setup_termination_path = termination_path.clone();
    let prepare_invocation = setup_invocation.clone();
    let setup_result = async move {
        // `timeout_at` only drops the `JoinHandle`, which detaches the blocking
        // task rather than aborting it. The task therefore owns the FIFO guard
        // itself and only hands it over on success, so FIFOs it creates after we
        // stopped waiting are unlinked when its abandoned result is dropped.
        let (prepared, fifo_guard) = tokio::time::timeout_at(
            setup_deadline,
            tokio::task::spawn_blocking(move || {
                prepare_pane_stream_task(
                    &root,
                    &setup_data_path,
                    &setup_termination_path,
                    cross_user,
                    &prepare_invocation,
                )
            }),
        )
        .await
        .context("timed out while starting pane stream")?
        .context("pane stream setup task panicked")??;

        let mut stream = PaneStream::new(prepared, fifo_guard)?;

        let data_path = shell_quote(&data_path.to_string_lossy());
        let lifecycle_path = shell_quote(&termination_path.to_string_lossy());
        let shell_command = format!(
            "exec 3> {data_path} && printf r > {lifecycle_path}; cat >&3; printf . > {lifecycle_path}"
        );
        run_tmux_status_until(
            &setup_invocation,
            &["pipe-pane", "-t", &setup_pane_target, &shell_command],
            setup_deadline,
            "starting pane stream",
        )
        .await?;

        if Instant::now() >= setup_deadline {
            anyhow::bail!("timed out while starting pane stream");
        }
        tokio::time::timeout_at(setup_deadline, stream.wait_until_ready())
            .await
            .context("timed out while starting pane stream")?
            .context("pane stream writer did not become ready")?;
        Ok::<_, anyhow::Error>(stream)
    }
    .await;

    match setup_result {
        Ok(stream) => Ok(stream),
        Err(error) => {
            if let Err(cleanup_error) = stop_pane_stream(pane_target, invocation).await {
                tracing::debug!(
                    pane_target,
                    error = %cleanup_error,
                    "failed to stop pane stream after setup error"
                );
            }
            Err(error)
        }
    }
}

async fn run_tmux_status_until(
    invocation: &TmuxInvocation,
    args: &[&str],
    deadline: Instant,
    operation: &'static str,
) -> anyhow::Result<()> {
    let invocation = invocation.clone();
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let deadline = deadline.into_std();
    let (result_sender, result_receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name("pane-stream-tmux".to_string())
        .spawn(move || {
            let timeout = deadline.saturating_duration_since(std::time::Instant::now());
            let result = if timeout.is_zero() {
                Err(anyhow::anyhow!("timed out while {operation}"))
            } else {
                let args = args.iter().map(String::as_str).collect::<Vec<_>>();
                run_tmux_status_with_timeout(&invocation, &args, timeout, operation)
            };
            let _ = result_sender.send(result);
        })
        .with_context(|| format!("failed to spawn tmux thread while {operation}"))?;

    result_receiver
        .await
        .with_context(|| format!("tmux thread exited without a result while {operation}"))?
}

fn tmux_command(invocation: &TmuxInvocation, args: &[&str]) -> Command {
    let mut command = if invocation.prefix.is_empty() {
        Command::new(&invocation.tmux_bin)
    } else {
        let mut command = Command::new(&invocation.prefix[0]);
        command.args(&invocation.prefix[1..]);
        command.arg(&invocation.tmux_bin);
        command
    };
    if let Some(socket) = invocation.socket.as_deref() {
        command.args(["-L", socket]);
    }
    command.args(args);
    command
}

fn run_tmux_status_with_timeout(
    invocation: &TmuxInvocation,
    args: &[&str],
    timeout: Duration,
    operation: &str,
) -> anyhow::Result<()> {
    let mut command = tmux_command(invocation, args);
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn tmux while {operation}"))?;
    let process_group = i32::try_from(child.id()).context("tmux pid exceeded i32")?;
    let deadline = std::time::Instant::now() + timeout;

    loop {
        match child
            .try_wait()
            .with_context(|| format!("failed to wait for tmux while {operation}"))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                anyhow::bail!(
                    "tmux command failed with exit code {} while {operation}",
                    status.code().unwrap_or(-1)
                );
            }
            None if std::time::Instant::now() >= deadline => {
                // The child is its own process-group leader, so a negative pid
                // cannot signal SilverBond's process group.
                unsafe {
                    libc::kill(-process_group, libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("timed out while {operation} after {timeout:?}");
            }
            None => {
                std::thread::sleep(
                    deadline
                        .saturating_duration_since(std::time::Instant::now())
                        .min(Duration::from_millis(10)),
                );
            }
        }
    }
}

/// The blocking half of pane-stream setup. It owns the FIFO guard for the paths
/// it creates and only releases it to the caller on success, so an abandoned
/// (detached) task still unlinks its FIFOs when its result is dropped.
fn prepare_pane_stream_task(
    root: &FsPath,
    data_path: &FsPath,
    termination_path: &FsPath,
    cross_user: bool,
    invocation: &TmuxInvocation,
) -> anyhow::Result<(PreparedPaneStream, PaneStreamFifoGuard)> {
    ensure_run_as_can_traverse_root(root, invocation)?;
    let fifo_guard = PaneStreamFifoGuard {
        paths: vec![data_path.to_path_buf(), termination_path.to_path_buf()],
    };
    let prepared = prepare_pane_stream(root, data_path, termination_path, cross_user)?;
    Ok((prepared, fifo_guard))
}

fn pane_stream_fifo_path(root: &FsPath, suffix: &str) -> PathBuf {
    root.join("run-states")
        .join(format!("pane-{}.{suffix}", uuid::Uuid::new_v4()))
}

fn ensure_run_as_can_traverse_root(
    root: &FsPath,
    invocation: &TmuxInvocation,
) -> anyhow::Result<()> {
    let Some((program, prefix_args)) = invocation.prefix.split_first() else {
        return Ok(());
    };
    let mut command = Command::new(program);
    command
        .args(prefix_args)
        .args(["/bin/test", "-x"])
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let output = tmux::command_output_with_timeout(command, tmux::DEFAULT_TMUX_COMMAND_TIMEOUT)
        .with_context(|| {
            format!(
                "failed to check whether the configured run-as identity can traverse SILVERBOND_ROOT {}",
                root.display()
            )
        })?;
    if !output.status.success() {
        let chmod_command = format!("chmod o+x {}", shell_quote(&root.to_string_lossy()));
        anyhow::bail!(
            "configured run-as identity cannot traverse SILVERBOND_ROOT {}; run `{}` and repeat it for each inaccessible ancestor",
            root.display(),
            chmod_command
        );
    }
    Ok(())
}

async fn stop_pane_stream(pane_target: &str, invocation: &TmuxInvocation) -> anyhow::Result<()> {
    run_tmux_status_until(
        invocation,
        &["pipe-pane", "-t", pane_target],
        Instant::now() + PANE_STREAM_SETUP_TIMEOUT,
        "stopping pane stream",
    )
    .await
}

struct PaneStreamFifoGuard {
    paths: Vec<PathBuf>,
}

impl Drop for PaneStreamFifoGuard {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

struct PreparedPaneStream {
    data: File,
    termination: File,
}

fn prepare_pane_stream(
    root: &FsPath,
    data_path: &FsPath,
    termination_path: &FsPath,
    cross_user: bool,
) -> anyhow::Result<PreparedPaneStream> {
    let stream_dir = root.join("run-states");
    fs::create_dir_all(&stream_dir).with_context(|| {
        format!(
            "failed to create pane stream directory {}",
            stream_dir.display()
        )
    })?;
    fs::set_permissions(&stream_dir, fs::Permissions::from_mode(0o711)).with_context(|| {
        format!(
            "failed to make pane stream directory traversable {}",
            stream_dir.display()
        )
    })?;

    make_cross_user_fifo(data_path, cross_user)?;
    make_cross_user_fifo(termination_path, cross_user)?;
    let data = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&data_path)
        .with_context(|| format!("failed to open pane stream FIFO {}", data_path.display()))?;
    let termination = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&termination_path)
        .with_context(|| {
            format!(
                "failed to open pane stream termination FIFO {}",
                termination_path.display()
            )
        })?;

    Ok(PreparedPaneStream { data, termination })
}

fn make_cross_user_fifo(path: &FsPath, cross_user: bool) -> anyhow::Result<()> {
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .with_context(|| format!("failed to run mkfifo for {}", path.display()))?;
    if !status.success() {
        anyhow::bail!(
            "mkfifo {} failed with exit code {}",
            path.display(),
            status.code().unwrap_or(-1)
        );
    }

    // mkfifo's requested/default mode is reduced by umask, so set the final
    // mode explicitly. Only cross-user pipe-pane writers need other-write.
    let mode = if cross_user { 0o622 } else { 0o600 };
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
        let _ = fs::remove_file(path);
        return Err(error)
            .with_context(|| format!("failed to chmod pane stream FIFO {}", path.display()));
    }
    Ok(())
}

struct PaneStream {
    data: AsyncFd<File>,
    termination: AsyncFd<File>,
    ready: bool,
    terminated: bool,
    _fifo_guard: PaneStreamFifoGuard,
}

impl PaneStream {
    fn new(prepared: PreparedPaneStream, fifo_guard: PaneStreamFifoGuard) -> anyhow::Result<Self> {
        Ok(Self {
            data: AsyncFd::new(prepared.data).context("failed to register pane stream FIFO")?,
            termination: AsyncFd::new(prepared.termination)
                .context("failed to register pane stream termination FIFO")?,
            ready: false,
            terminated: false,
            _fifo_guard: fifo_guard,
        })
    }

    async fn wait_until_ready(&mut self) -> io::Result<()> {
        while !self.ready {
            if self.terminated {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "pane stream writer terminated before becoming ready",
                ));
            }

            let marker_result = {
                let mut readiness = self.termination.readable().await?;
                let mut marker = [0_u8; 1];
                match readiness.try_io(|inner| read_nonblocking(inner.get_ref(), &mut marker)) {
                    Ok(result) => Some(result.map(|bytes_read| (bytes_read, marker[0]))),
                    Err(_would_block) => None,
                }
            };

            match marker_result {
                Some(Ok((0, _))) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "pane stream lifecycle FIFO closed before writer became ready",
                    ));
                }
                Some(Ok((_, marker))) => self.observe_lifecycle_marker(marker)?,
                Some(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Some(Err(error)) => return Err(error),
                None => continue,
            }
        }
        Ok(())
    }

    fn observe_lifecycle_marker(&mut self, marker: u8) -> io::Result<()> {
        match marker {
            b'r' => self.ready = true,
            b'.' => self.terminated = true,
            marker => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected pane stream lifecycle marker {marker:#04x}"),
                ));
            }
        }
        Ok(())
    }
}

fn read_nonblocking(file: &File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut file = file;
    file.read(buffer)
}

impl AsyncRead for PaneStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        if !this.terminated {
            loop {
                let mut readiness = match this.termination.poll_read_ready(cx) {
                    Poll::Ready(Ok(readiness)) => readiness,
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Pending => break,
                };
                let mut marker = [0_u8; 1];
                match readiness.try_io(|inner| read_nonblocking(inner.get_ref(), &mut marker)) {
                    Ok(Ok(0)) => break,
                    Ok(Ok(_)) => match this.observe_lifecycle_marker(marker[0]) {
                        Ok(()) if this.terminated => break,
                        Ok(()) => continue,
                        Err(error) => return Poll::Ready(Err(error)),
                    },
                    Ok(Err(error)) => return Poll::Ready(Err(error)),
                    Err(_would_block) => continue,
                }
            }
        }

        if this.terminated {
            let unfilled = buffer.initialize_unfilled();
            match read_nonblocking(this.data.get_ref(), unfilled) {
                Ok(0) => return Poll::Ready(Ok(())),
                Ok(bytes_read) => {
                    buffer.advance(bytes_read);
                    return Poll::Ready(Ok(()));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Poll::Ready(Ok(()));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Err(error) => return Poll::Ready(Err(error)),
            }
        }

        loop {
            let mut readiness = ready!(this.data.poll_read_ready(cx))?;
            let unfilled = buffer.initialize_unfilled();
            match readiness.try_io(|inner| read_nonblocking(inner.get_ref(), unfilled)) {
                Ok(Ok(bytes_read)) => {
                    buffer.advance(bytes_read);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_would_block) => continue,
            }
        }
    }
}

async fn subscribe_pane_stream(
    state: &AppState,
    run_id: &str,
    pane: &str,
    pane_target: &str,
    invocation: &TmuxInvocation,
) -> Result<
    (
        broadcast::Receiver<Vec<u8>>,
        broadcast::Sender<Vec<u8>>,
        PaneStreamKey,
    ),
    PaneStreamSubscribeError,
> {
    let key = PaneStreamKey::new(invocation, pane_target);
    let subscriber_limit = state.pane_streams.max_subscribers_per_pane;
    loop {
        let mut streams = state.pane_streams.inner.lock().await;
        if let Some(entry) = streams.get_mut(&key) {
            if entry.is_terminating() {
                let owner_done = entry.owner_done();
                drop(streams);
                owner_done.cancelled().await;
                continue;
            }
            if entry.refcount >= subscriber_limit {
                return Err(PaneStreamSubscribeError::TooManySubscribers {
                    limit: subscriber_limit,
                });
            }
            entry.refcount = entry.refcount.saturating_add(1);
            entry.revive();
            let sender = entry.sender.clone();
            return Ok((sender.subscribe(), sender, key));
        }

        if subscriber_limit == 0 {
            return Err(PaneStreamSubscribeError::TooManySubscribers {
                limit: subscriber_limit,
            });
        }

        let (sender, receiver) = broadcast::channel::<Vec<u8>>(256);
        let entry = PaneStreamEntry::new(sender.clone(), 1);
        let drain_signal = entry.drain_receiver();
        let owner_done = entry.owner_done();
        streams.insert(key.clone(), entry);
        drop(streams);

        spawn_pane_stream_task(
            state.clone(),
            run_id.to_string(),
            pane.to_string(),
            pane_target.to_string(),
            key.clone(),
            invocation.clone(),
            sender.clone(),
            drain_signal,
            owner_done,
        );

        return Ok((receiver, sender, key));
    }
}

#[derive(Debug, thiserror::Error)]
enum PaneStreamSubscribeError {
    #[error("too many pane stream subscribers (limit: {limit})")]
    TooManySubscribers { limit: usize },
}

async fn pump_pane_stream<R>(
    pane_reader: &mut R,
    sender: &broadcast::Sender<Vec<u8>>,
    drain_signal: &mut watch::Receiver<bool>,
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
            drain_result = drain_signal.changed() => {
                if drain_result.is_err() || *drain_signal.borrow() {
                    tracing::debug!(
                        run_id = %run_id,
                        pane = %pane,
                        pane_target = %pane_target,
                        "pane stream is draining"
                    );
                    break PaneStreamTaskExit::NoSubscribers;
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
                            drain_result = drain_signal.changed() => {
                                if drain_result.is_err() || *drain_signal.borrow() {
                                    tracing::debug!(
                                        run_id = %run_id,
                                        pane = %pane,
                                        pane_target = %pane_target,
                                        "pane stream is draining"
                                    );
                                    break PaneStreamTaskExit::NoSubscribers;
                                }
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
    key: PaneStreamKey,
    invocation: TmuxInvocation,
    sender: broadcast::Sender<Vec<u8>>,
    mut drain_signal: watch::Receiver<bool>,
    owner_done: CancellationToken,
) {
    tokio::spawn(async move {
        let owns_entry = match start_pane_stream(&state.paths.root, &pane_target, &invocation).await
        {
            Ok(mut pane_reader) => loop {
                let exit = pump_pane_stream(
                    &mut pane_reader,
                    &sender,
                    &mut drain_signal,
                    &run_id,
                    &pane,
                    &pane_target,
                    PANE_STREAM_READ_RETRY_DELAY,
                )
                .await;
                match claim_pane_stream_task_exit(&state.pane_streams, &key, &sender, exit).await {
                    PaneStreamOwnerAction::Continue => continue,
                    PaneStreamOwnerAction::Stale => break false,
                    PaneStreamOwnerAction::Teardown => {
                        if let Err(error) = stop_pane_stream(&pane_target, &invocation).await {
                            tracing::debug!(
                                run_id = %run_id,
                                pane = %pane,
                                pane_target = %pane_target,
                                error = %error,
                                "failed to stop pane stream pipe"
                            );
                        }
                        break true;
                    }
                }
            },
            Err(error) => {
                tracing::warn!(
                    run_id = %run_id,
                    pane = %pane,
                    pane_target = %pane_target,
                    error = %error,
                    "failed to start pane stream"
                );
                matches!(
                    claim_pane_stream_task_exit(
                        &state.pane_streams,
                        &key,
                        &sender,
                        PaneStreamTaskExit::Terminal,
                    )
                    .await,
                    PaneStreamOwnerAction::Teardown
                )
            }
        };

        if owns_entry {
            state
                .pane_streams
                .remove_terminal_sender(&key, &sender)
                .await;
        }
        owner_done.cancel();
    });
}

async fn claim_pane_stream_task_exit(
    pane_streams: &crate::app::PaneStreamRegistry,
    key: &PaneStreamKey,
    sender: &broadcast::Sender<Vec<u8>>,
    exit: PaneStreamTaskExit,
) -> PaneStreamOwnerAction {
    let mut streams = pane_streams.inner.lock().await;
    let Some(entry) = streams.get_mut(key) else {
        return PaneStreamOwnerAction::Stale;
    };
    if !entry.sender.same_channel(sender) {
        return PaneStreamOwnerAction::Stale;
    }

    match exit {
        PaneStreamTaskExit::NoSubscribers => {
            if entry.refcount != 0 || !entry.is_draining() {
                return PaneStreamOwnerAction::Continue;
            }
        }
        PaneStreamTaskExit::Terminal => {}
    }
    entry.begin_termination();
    PaneStreamOwnerAction::Teardown
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
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let token_candidates = header_stream_token_candidates(&headers, STREAM_TOKEN_HEADER);
    require_run_stream_token(&state, &run_id, &token_candidates).await?;
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
    state.runtime.dismiss_run(&run_id).await?;
    Ok(Json(json!({ "success": true })))
}

async fn interrupted_runs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let runs = state.runtime.db.list_interrupted_runs().await?;
    let active_run_ids = state.runtime.registry.active_run_ids().await;
    let runs = runs
        .into_iter()
        .filter(|run| !active_run_ids.contains(&run.run_id));
    let enriched = try_join_all(runs.map(|run| enrich_interrupted_run(&state, run))).await?;
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
                "node payload must use canonical v4 task or approval kind types; got {}",
                node.kind.as_str()
            )
        }
        Err(err) => {
            // Give a targeted hint when the caller used the obsolete flat shape
            // (top-level `type` with no `kind` wrapper).
            if value.get("type").is_some() && value.get("kind").is_none() {
                anyhow::bail!(
                    "node uses the legacy flat shape; migrate to the canonical v4 `kind: {{ type, ... }}` form"
                );
            }
            Err(anyhow::anyhow!(err).context("node payload must be a canonical v4 WorkflowNode"))
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
            RunControlError::TerminalState
            | RunControlError::AlreadyActive
            | RunControlError::StaleInteractionSession => Self::conflict(error.to_string()),
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
    use axum::body::Body;
    use std::{
        collections::VecDeque,
        io,
        pin::Pin,
        task::{Context as TaskContext, Poll},
    };
    use tokio::io::ReadBuf;
    use tower::ServiceExt;

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

    const PANE_STREAM_TEST_TIMEOUT: Duration =
        crate::test_support::test_budget(PANE_STREAM_SETUP_TIMEOUT);

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
            unlock_throttle: UnlockThrottle::default(),
        }
    }

    fn test_pane_stream_key(pane_target: &str) -> PaneStreamKey {
        PaneStreamKey::new(&TmuxInvocation::default(), pane_target)
    }

    fn approval_workflow_with_run_as(command: Vec<String>) -> WorkflowV3 {
        WorkflowV3 {
            version: WORKFLOW_SCHEMA_VERSION,
            name: Some("observability-test".to_string()),
            goal: "wait for approval".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: Some(RunAsConfig {
                user: None,
                command: Some(command),
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
            version: WORKFLOW_SCHEMA_VERSION,
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

    fn workflow_exceeding_subflow_limit() -> WorkflowV3 {
        let mut workflow = approval_only_workflow();
        for index in 0..=crate::model::MAX_WORKFLOW_SUBFLOWS {
            workflow.subflows.insert(
                format!("subflow-{index}"),
                Box::new(approval_only_workflow()),
            );
        }
        workflow
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
            unlock_throttle: UnlockThrottle::default(),
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

    fn assert_unlock_throttled(error: ApiError) {
        let ApiError::ValidationBody { status, body, .. } = error else {
            panic!("expected unlock throttle error, got {error:?}");
        };
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["code"], "unlock_throttled");
        assert!(
            body["retryAfterSeconds"]
                .as_u64()
                .is_some_and(|delay| (1..=5).contains(&delay))
        );
    }

    #[tokio::test]
    async fn validate_workflow_route_rejects_oversized_subflow_catalog_with_422() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;

        let error = validate_workflow_route(
            State(state),
            Json(WorkflowPayloadRequest {
                workflow: serde_json::to_value(workflow_exceeding_subflow_limit()).unwrap(),
            }),
        )
        .await
        .unwrap_err();

        let ApiError::ValidationBody { status, body, .. } = error else {
            panic!("expected validation body, got {error:?}");
        };
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            body["details"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("at most 1024 are allowed")),
            "got {body}"
        );
    }

    #[tokio::test]
    async fn create_run_rejects_oversized_subflow_catalog_with_422() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;

        let error = create_run(
            State(state),
            Json(create_run_request(workflow_exceeding_subflow_limit(), None)),
        )
        .await
        .unwrap_err();

        let ApiError::ValidationBody { status, body, .. } = error else {
            panic!("expected validation body, got {error:?}");
        };
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            body["details"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("at most 1024 are allowed")),
            "got {body}"
        );
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
    async fn create_run_rejects_privileged_node_with_wrong_unlock_secret() {
        let security = SecurityConfig {
            agent_user: None,
            unlock_password_hash: Some(sha256_unlock_password_hash("correct")),
        };
        let (_temp, state) = create_test_state(security).await;

        let error = create_run(
            State(state),
            Json(create_run_request(
                approval_workflow_with_unreachable_task(),
                Some("wrong"),
            )),
        )
        .await
        .unwrap_err();

        assert_privileged_unlock_required(error);
    }

    #[tokio::test]
    async fn create_run_throttles_correct_secret_after_failed_unlock_attempt() {
        let security = SecurityConfig {
            agent_user: None,
            unlock_password_hash: Some(sha256_unlock_password_hash("correct")),
        };
        let (_temp, state) = create_test_state(security).await;

        let first_error = create_run(
            State(state.clone()),
            Json(create_run_request(
                approval_workflow_with_unreachable_task(),
                Some("wrong"),
            )),
        )
        .await
        .unwrap_err();
        assert_privileged_unlock_required(first_error);

        let throttled = create_run(
            State(state),
            Json(create_run_request(
                approval_workflow_with_unreachable_task(),
                Some("correct"),
            )),
        )
        .await
        .unwrap_err();

        assert_unlock_throttled(throttled);
    }

    #[tokio::test]
    async fn create_run_counts_failed_unlock_before_workflow_validation() {
        let security = SecurityConfig {
            agent_user: None,
            unlock_password_hash: Some(sha256_unlock_password_hash("correct")),
        };
        let (_temp, state) = create_test_state(security).await;
        let mut invalid_workflow = approval_workflow_with_unreachable_task();
        invalid_workflow.entry_node_id = "missing-entry".to_string();

        let first_error = create_run(
            State(state.clone()),
            Json(create_run_request(invalid_workflow, Some("wrong"))),
        )
        .await
        .unwrap_err();
        assert_privileged_unlock_required(first_error);

        let throttled = create_run(
            State(state),
            Json(create_run_request(
                approval_workflow_with_unreachable_task(),
                Some("correct"),
            )),
        )
        .await
        .unwrap_err();

        assert_unlock_throttled(throttled);
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

    #[tokio::test]
    async fn authorized_node_preview_invocation_uses_agent_user_downgrade() {
        let node = approval_workflow_with_unreachable_task()
            .nodes
            .pop()
            .unwrap();
        let security = SecurityConfig {
            agent_user: Some("preview-agent".to_string()),
            unlock_password_hash: Some(sha256_unlock_password_hash("correct")),
        };

        let run_as = authorize_and_prepare_node_preview_security(
            &node,
            "/preview",
            &security,
            &UnlockThrottle::default(),
            Some("correct"),
        )
        .await
        .unwrap();
        let invocation = crate::tmux_exec::build_tmux_invocation(&run_as, "preview_security");

        assert_eq!(
            invocation.prefix,
            ["sudo", "-u", "preview-agent", "-H", "--"].map(str::to_string)
        );
        assert_eq!(
            invocation.socket.as_deref(),
            Some("silverbond-preview_security")
        );
    }

    #[tokio::test]
    async fn resume_run_rejects_an_already_active_run_with_a_distinct_conflict() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;

        let response = resume_run(State(state), Path(run_id))
            .await
            .expect_err("an active run must not spawn a second executor")
            .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"], "run is already actively executing");
        assert_ne!(body["error"], "run is in terminal state");
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

    #[tokio::test]
    async fn run_events_requires_the_run_stream_token() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;

        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/runs/{run_id}/events"))
                    .header(header::ORIGIN, "http://localhost:5173")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn run_events_accepts_the_run_stream_token_header() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        let stream_token = state
            .runtime
            .db
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .stream_token;

        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/runs/{run_id}/events"))
                    .header(header::ORIGIN, "http://localhost:5173")
                    .header("x-stream-token", stream_token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn run_events_rejects_cross_origin_requests() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        let stream_token = state
            .runtime
            .db
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .stream_token;

        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/runs/{run_id}/events"))
                    .header(header::ORIGIN, "https://example.com")
                    .header("x-stream-token", stream_token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn run_stream_accepts_the_run_stream_token_header() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        let stream_token = state
            .runtime
            .db
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .stream_token;

        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/runs/{run_id}/stream"))
                    .header(header::ORIGIN, "http://localhost:5173")
                    .header("x-stream-token", stream_token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn run_stream_rejects_a_stream_token_in_the_url() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        let stream_token = state
            .runtime
            .db
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .stream_token;

        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/runs/{run_id}/stream?token={stream_token}"))
                    .header(header::ORIGIN, "http://localhost:5173")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn stream_token_rejection_does_not_reveal_run_existence() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        let app = router(state);

        let existing_run_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/runs/{run_id}/events"))
                    .header(header::ORIGIN, "http://localhost:5173")
                    .header("x-stream-token", "wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let missing_run_response = app
            .oneshot(
                Request::builder()
                    .uri("/api/runs/missing-run/events")
                    .header(header::ORIGIN, "http://localhost:5173")
                    .header("x-stream-token", "wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(existing_run_response.status(), StatusCode::FORBIDDEN);
        assert_eq!(missing_run_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn run_stream_dedupes_buffered_live_event_after_replay() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let run_id = "run-stream-dedupe";
        state.runtime.registry.register_test_run(run_id).await;
        let mut receiver = state
            .runtime
            .registry
            .subscribe(run_id)
            .await
            .expect("registered run");

        let base = RuntimeEvent::new("node_start").with("runId", run_id);
        let seq = state
            .runtime
            .db
            .append_event(run_id, &base)
            .await
            .unwrap();
        state
            .runtime
            .registry
            .send_test_event(run_id, base.with_seq(seq))
            .await;

        let replay = state.runtime.db.list_events(run_id).await.unwrap();
        let mut max_seq = runtime_event_seq(&replay[0]);
        let drained = drain_buffered_run_events(&mut receiver, &mut max_seq);
        assert!(
            drained.is_empty(),
            "live broadcast duplicate must be dropped after replay"
        );
    }

    #[test]
    fn run_stream_buffered_drain_signals_resync_on_lagged() {
        let (tx, mut rx) = broadcast::channel(1);
        let _ = tx.send(RuntimeEvent::new("first").with_seq(1));
        let _ = tx.send(RuntimeEvent::new("second").with_seq(2));

        let mut max_seq = 0u64;
        let items = drain_buffered_run_events(&mut rx, &mut max_seq);
        assert!(
            items
                .iter()
                .any(|item| matches!(item, RunStreamBufferedItem::Resync)),
            "expected resync after broadcast lag"
        );
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
    async fn run_observability_prefers_single_registered_session_over_tmux_lookup() {
        use std::os::unix::fs::PermissionsExt;

        let (temp, mut state) = create_test_state(SecurityConfig::default()).await;
        let tmux = temp.path().join("fake-tmux.sh");
        let tmux_marker = temp.path().join("tmux-invoked");
        std::fs::write(
            &tmux,
            "#!/bin/sh\ntouch \"$(dirname \"$0\")/tmux-invoked\"\nprintf 'fallback-session\\n'\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&tmux, permissions).unwrap();
        state.runtime.run_invocation = Some(TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: tmux.to_string_lossy().into_owned(),
        });
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        state
            .runtime
            .registry
            .set_active_pane(&run_id, "approve", "%1")
            .await;
        state
            .runtime
            .db
            .register_tmux_session(&run_id, "registered-session")
            .await
            .unwrap();

        let observability = run_observability_object(&state, &run_id).await;

        assert_eq!(
            observability.get("sessionName"),
            Some(&json!("registered-session"))
        );
        assert_eq!(
            observability["panes"][0]["sessionName"],
            "registered-session"
        );
        assert!(!tmux_marker.exists());
        state.runtime.abort_run(&run_id).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_observability_bounds_tmux_session_lookup_concurrency() {
        use std::os::unix::fs::PermissionsExt;

        let (temp, mut state) = create_test_state(SecurityConfig::default()).await;
        let tmux = temp.path().join("counting-tmux.sh");
        std::fs::write(temp.path().join("current"), "0\n").unwrap();
        std::fs::write(temp.path().join("maximum"), "0\n").unwrap();
        std::fs::write(
            &tmux,
            r#"#!/bin/sh
dir=$(dirname "$0")
while ! mkdir "$dir/lock" 2>/dev/null; do sleep 0.01; done
current=$(cat "$dir/current")
current=$((current + 1))
printf '%s\n' "$current" > "$dir/current"
maximum=$(cat "$dir/maximum")
if [ "$current" -gt "$maximum" ]; then
  printf '%s\n' "$current" > "$dir/maximum"
fi
rmdir "$dir/lock"
sleep 0.05
while ! mkdir "$dir/lock" 2>/dev/null; do sleep 0.01; done
current=$(cat "$dir/current")
printf '%s\n' "$((current - 1))" > "$dir/current"
rmdir "$dir/lock"
printf 'fallback-session\n'
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&tmux, permissions).unwrap();
        state.runtime.run_invocation = Some(TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: tmux.to_string_lossy().into_owned(),
        });
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        for index in 0..8 {
            state
                .runtime
                .registry
                .set_active_pane(&run_id, &format!("pane-{index}"), &format!("%{index}"))
                .await;
        }

        let observability = run_observability_object(&state, &run_id).await;

        assert_eq!(observability["panes"].as_array().unwrap().len(), 8);
        let maximum = std::fs::read_to_string(temp.path().join("maximum"))
            .unwrap()
            .trim()
            .parse::<usize>()
            .unwrap();
        assert!(maximum <= 4, "observed {maximum} concurrent tmux lookups");
        state.runtime.abort_run(&run_id).await.unwrap();
    }

    #[tokio::test]
    async fn interrupted_runs_excludes_registered_executors() {
        let (_temp, state) = create_test_state(SecurityConfig::default()).await;
        let active_run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &active_run_id).await;

        let mut inactive_run = state
            .runtime
            .db
            .get_run(&active_run_id)
            .await
            .unwrap()
            .unwrap();
        inactive_run.checkpoint.run_id = "run_interrupted".to_string();
        inactive_run.checkpoint.execution_log.run_id = "run_interrupted".to_string();
        state.runtime.db.upsert_run(&inactive_run).await.unwrap();

        let Json(value) = interrupted_runs(State(state.clone())).await.unwrap();
        let runs = value.as_array().unwrap();

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["runId"], "run_interrupted");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dismiss_run_reaps_only_the_interrupted_runs_tmux_sessions() {
        use std::os::unix::fs::PermissionsExt;

        let (temp, state) = create_test_state(SecurityConfig::default()).await;
        let dismissed_run_id = "run_interrupted_to_dismiss";
        let running_run_id = "run_still_running";
        let dismissed_session = "silverbond-Interrupted-Dismissed";
        let running_session = "silverbond-Still-Running";
        let socket = format!("silverbond-{dismissed_run_id}");
        let script = temp.path().join("dismiss-tmux-prefix.sh");
        let log = temp.path().join("dismiss-tmux-args.log");
        let resolver_marker = temp.path().join("dismiss-tmux-resolver-ran");
        std::fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\ndismissed=\"$2\"\nrunning=\"$3\"\nresolver_marker=\"$4\"\nshift 4\nif [ \"$1\" = \"zsh\" ]; then touch \"$resolver_marker\"; printf 'SBTMUX:tmux\\n'; exit 0; fi\nprintf '%s\\n' \"$@\" >> \"$log\"\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"-L\" ]; then shift 2; fi\nif [ \"$1\" = \"list-sessions\" ]; then printf '%s\\n%s\\n' \"$dismissed\" \"$running\"; fi\nif [ \"$1\" = \"has-session\" ]; then exit 1; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let workflow = approval_workflow_with_run_as(vec![
            script.to_string_lossy().into_owned(),
            log.to_string_lossy().into_owned(),
            dismissed_session.to_string(),
            running_session.to_string(),
            resolver_marker.to_string_lossy().into_owned(),
        ]);
        for (run_id, session) in [
            (dismissed_run_id, dismissed_session),
            (running_run_id, running_session),
        ] {
            let mut checkpoint = test_checkpoint();
            checkpoint.run_id = run_id.to_string();
            checkpoint.execution_log.run_id = run_id.to_string();
            state
                .runtime
                .db
                .upsert_run(&PersistedRun {
                    stream_token: format!("stream-{run_id}"),
                    tmux_invocation: None,
                    checkpoint,
                    workflow: workflow.clone(),
                })
                .await
                .unwrap();
            state
                .runtime
                .db
                .register_tmux_session(run_id, session)
                .await
                .unwrap();
        }

        let Json(response) = dismiss_run(State(state.clone()), Path(dismissed_run_id.to_string()))
            .await
            .unwrap();

        assert_eq!(response, json!({ "success": true }));
        assert!(
            !state
                .runtime
                .db
                .list_interrupted_runs()
                .await
                .unwrap()
                .iter()
                .any(|run| run.run_id == dismissed_run_id),
            "dismissed run should leave the interrupted-run list"
        );
        assert!(
            state
                .runtime
                .db
                .list_run_tmux_session_names(dismissed_run_id)
                .await
                .unwrap()
                .is_empty(),
            "dismissed run should have its tmux registration reaped"
        );
        assert_eq!(
            state
                .runtime
                .db
                .list_run_tmux_session_names(running_run_id)
                .await
                .unwrap(),
            vec![running_session.to_string()],
            "another running run must retain its tmux registration"
        );
        assert!(
            !resolver_marker.exists(),
            "dismissal reaping must not resolve tmux"
        );
        let recorded = std::fs::read_to_string(log)
            .expect("dismissal should issue invocation-scoped tmux commands");
        let args = recorded.lines().collect::<Vec<_>>();
        assert!(
            args.windows(6).any(|window| window
                == [
                    "tmux",
                    "-L",
                    socket.as_str(),
                    "list-sessions",
                    "-F",
                    "#{session_name}"
                ]),
            "dismissal should list sessions under the run invocation; args={args:?}"
        );
        assert!(
            args.windows(6).any(|window| window
                == [
                    "tmux",
                    "-L",
                    socket.as_str(),
                    "kill-session",
                    "-t",
                    dismissed_session
                ]),
            "dismissal should kill the interrupted run session; args={args:?}"
        );
        assert!(
            !args.windows(6).any(|window| window
                == [
                    "tmux",
                    "-L",
                    socket.as_str(),
                    "kill-session",
                    "-t",
                    running_session
                ]),
            "dismissal must not kill another running run session; args={args:?}"
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

        let candidates = checkpoint.pane_candidates();

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
            unlock_throttle: UnlockThrottle::default(),
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

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_context_uses_persisted_invocation_without_running_resolver() {
        use std::os::unix::fs::PermissionsExt;

        let (temp, mut state) = create_test_state(SecurityConfig::default()).await;

        let resolver_marker = temp.path().join("request-path-resolver-ran");
        let resolver = temp.path().join("poison-tmux-resolver.sh");
        std::fs::write(&resolver, "#!/bin/sh\ntouch \"$1\"\nexit 1\n").unwrap();
        let mut permissions = std::fs::metadata(&resolver).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&resolver, permissions).unwrap();

        let expected_invocation = TmuxInvocation {
            prefix: vec!["stored-prefix".to_string()],
            socket: Some("stored-socket".to_string()),
            tmux_bin: "/stored/bin/tmux".to_string(),
        };
        state.runtime.run_invocation = Some(expected_invocation.clone());
        let run_id = state
            .runtime
            .start_run(
                approval_workflow_with_run_as(vec![
                    resolver.to_string_lossy().into_owned(),
                    resolver_marker.to_string_lossy().into_owned(),
                ]),
                BTreeMap::new(),
                None,
            )
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        let stream_token = state
            .runtime
            .db
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .stream_token;
        state
            .runtime
            .registry
            .set_active_pane(&run_id, "active", "%stored-pane")
            .await;

        let (pane_target, invocation) =
            resolve_run_pane_context(&state, &run_id, "active", &stream_token)
                .await
                .unwrap();

        assert_eq!(pane_target, "%stored-pane");
        assert_eq!(invocation, expected_invocation);
        assert!(!resolver_marker.exists());
        state.runtime.abort_run(&run_id).await.unwrap();
    }

    #[tokio::test]
    async fn pane_stream_registry_fans_out_and_cleans_up() {
        let registry = PaneStreamRegistry::default();
        let key = test_pane_stream_key("%1");
        let (sender, mut first_receiver) = broadcast::channel::<Vec<u8>>(256);
        let mut second_receiver = {
            let mut streams = registry.inner.lock().await;
            streams.insert(key.clone(), PaneStreamEntry::new(sender.clone(), 1));
            let entry = streams.get_mut(&key).expect("pane stream entry exists");
            entry.refcount += 1;
            entry.sender.subscribe()
        };

        sender.send(b"chunk".to_vec()).unwrap();
        assert_eq!(first_receiver.recv().await.unwrap(), b"chunk".to_vec());
        assert_eq!(second_receiver.recv().await.unwrap(), b"chunk".to_vec());

        registry.unsubscribe(&key, &sender).await;
        assert_eq!(
            registry
                .inner
                .lock()
                .await
                .get(&key)
                .map(|entry| entry.refcount),
            Some(1)
        );

        registry.unsubscribe(&key, &sender).await;
        {
            let streams = registry.inner.lock().await;
            let entry = streams
                .get(&key)
                .expect("draining owner remains registered");
            assert_eq!(entry.refcount, 0);
            assert!(entry.is_draining());
        }

        registry.remove_terminal_sender(&key, &sender).await;
        assert!(!registry.inner.lock().await.contains_key(&key));
    }

    #[tokio::test]
    async fn pane_stream_registry_isolates_same_pane_id_on_different_tmux_servers() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let release = temp.path().join("release");
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            format!(
                "#!/bin/sh\nwhile [ ! -e '{}' ]; do sleep 0.01; done\nexit 1\n",
                release.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_tmux, permissions).unwrap();

        let registry = PaneStreamRegistry::default();
        let state = test_app_state(registry);
        let first_invocation = TmuxInvocation {
            prefix: Vec::new(),
            socket: Some("server-a".to_string()),
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };
        let second_invocation = TmuxInvocation {
            socket: Some("server-b".to_string()),
            ..first_invocation.clone()
        };

        let (mut first_receiver, first_sender, _) =
            subscribe_pane_stream(&state, "run-a", "pane", "%0", &first_invocation)
                .await
                .unwrap();
        let (mut second_receiver, second_sender, _) =
            subscribe_pane_stream(&state, "run-b", "pane", "%0", &second_invocation)
                .await
                .unwrap();

        assert!(!first_sender.same_channel(&second_sender));
        first_sender.send(b"first server".to_vec()).unwrap();
        assert_eq!(
            first_receiver.recv().await.unwrap(),
            b"first server".to_vec()
        );
        assert!(matches!(
            second_receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        second_sender.send(b"second server".to_vec()).unwrap();
        assert_eq!(
            second_receiver.recv().await.unwrap(),
            b"second server".to_vec()
        );

        std::fs::write(release, b"done").unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_fast_reconnect_reuses_setup_in_progress() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let setup_started = temp.path().join("setup-started");
        let release_setup = temp.path().join("release-setup");
        let tmux_log = temp.path().join("tmux.log");
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            format!(
                "#!/bin/sh\nif [ \"$#\" -eq 3 ]; then printf 'stop\\n' >> '{}'; exit 0; fi\nprintf 'enable\\n' >> '{}'\n: > '{}'\nwhile [ ! -e '{}' ]; do sleep 0.01; done\nexit 1\n",
                tmux_log.display(),
                tmux_log.display(),
                setup_started.display(),
                release_setup.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();

        let registry = PaneStreamRegistry::default();
        let state = test_app_state(registry.clone());
        let invocation = TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };
        let (first_receiver, first_sender, key) =
            subscribe_pane_stream(&state, "run-test", "pane-test", "%1", &invocation)
                .await
                .unwrap();

        tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, async {
            while !setup_started.exists() {
                tokio::time::sleep(PANE_STREAM_READ_RETRY_DELAY).await;
            }
        })
        .await
        .expect("the first pane-stream setup must start");

        drop(first_receiver);
        registry.unsubscribe(&key, &first_sender).await;
        let (second_receiver, second_sender, second_key) =
            subscribe_pane_stream(&state, "run-test", "pane-test", "%1", &invocation)
                .await
                .unwrap();
        tokio::time::sleep(PANE_STREAM_READ_RETRY_DELAY).await;

        let reused_owner = first_sender.same_channel(&second_sender);
        let enable_count = std::fs::read_to_string(&tmux_log)
            .unwrap()
            .lines()
            .filter(|line| *line == "enable")
            .count();

        drop(second_receiver);
        registry.unsubscribe(&second_key, &second_sender).await;
        std::fs::write(release_setup, b"release").unwrap();

        assert!(
            reused_owner,
            "a reconnect during setup must reuse the current pane-stream owner"
        );
        assert_eq!(
            enable_count, 1,
            "a reconnect during setup must not start a second pipe-pane"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_start_preserves_silverbond_root_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mode_before = std::fs::metadata(temp.path()).unwrap().permissions().mode();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\nfor arg do command=$arg; done\n/bin/sh -c \"$command\" </dev/null >/dev/null 2>&1\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec!["/usr/bin/env".to_string()],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let stream = start_pane_stream(temp.path(), "%1", &invocation)
            .await
            .unwrap();
        let mode_after = std::fs::metadata(temp.path()).unwrap().permissions().mode();

        assert_eq!(mode_after, mode_before);
        drop(stream);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_start_rejects_root_the_run_as_identity_cannot_traverse() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let run_as = temp.path().join("run-as");
        std::fs::write(
            &run_as,
            "#!/bin/sh\nif [ \"$1\" = /bin/test ]; then exit 1; fi\nexec \"$@\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&run_as, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![run_as.to_string_lossy().into_owned()],
            socket: None,
            tmux_bin: "/usr/bin/true".to_string(),
        };

        let error = start_pane_stream(temp.path(), "%1", &invocation)
            .await
            .err()
            .expect("a non-traversable root must reject pane-stream setup");
        let required_command = format!("chmod o+x {}", shell_quote(&temp.path().to_string_lossy()));

        assert!(
            error.to_string().contains(&required_command),
            "error must tell the operator how to make the root traversable: {error:#}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_probe_timeout_cannot_late_enable_pipe_or_leave_fifos() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let probe_started = temp.path().join("probe-started");
        let release_probe = temp.path().join("release-probe");
        let tmux_log = temp.path().join("tmux.log");
        let run_as = temp.path().join("run-as");
        std::fs::write(
            &run_as,
            "#!/bin/sh\nstarted=$1\nrelease=$2\nshift 2\nif [ \"$1\" = /bin/test ]; then\n  : > \"$started\"\n  while [ ! -e \"$release\" ]; do sleep 1; done\n  exit 0\nfi\nexec \"$@\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&run_as, std::fs::Permissions::from_mode(0o755)).unwrap();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
                tmux_log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![
                run_as.to_string_lossy().into_owned(),
                probe_started.to_string_lossy().into_owned(),
                release_probe.to_string_lossy().into_owned(),
            ],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let root = temp.path().to_path_buf();
        let setup = tokio::spawn(async move { start_pane_stream(&root, "%1", &invocation).await });
        tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, async {
            while !probe_started.exists() {
                tokio::time::sleep(PANE_STREAM_READ_RETRY_DELAY).await;
            }
        })
        .await
        .expect("traversability probe must start");
        let setup_error = tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, setup)
            .await
            .expect("hung probe must not outlive the pane-stream test budget")
            .expect("pane-stream setup task must not panic")
            .err()
            .expect("hung probe must fail pane-stream setup");
        assert!(
            format!("{setup_error:#}").contains("timed out"),
            "hung probe must surface a timeout: {setup_error:#}"
        );

        std::fs::write(&release_probe, b"release").unwrap();
        tokio::time::sleep(PANE_STREAM_SETUP_TIMEOUT).await;

        assert_eq!(
            std::fs::read_to_string(&tmux_log).unwrap(),
            "pipe-pane -t %1\n",
            "teardown must be the only pipe-pane command after a probe timeout"
        );
        let stream_dir = temp.path().join("run-states");
        if stream_dir.exists() {
            assert_eq!(
                std::fs::read_dir(stream_dir).unwrap().count(),
                0,
                "pane-stream FIFOs must not survive timeout cleanup"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_enable_descendant_pipe_cannot_outlive_setup_deadline() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\nif [ \"$#\" -eq 3 ]; then exit 0; fi\n(sleep 9) &\nsleep 9\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let result = tokio::time::timeout(
            PANE_STREAM_SETUP_TIMEOUT + Duration::from_secs(2),
            start_pane_stream(temp.path(), "%1", &invocation),
        )
        .await
        .expect("pane-stream enable must not outlive the shared setup deadline");
        let Err(result) = result else {
            panic!("the hung fake tmux command must fail pane-stream setup");
        };

        assert!(
            format!("{result:#}").contains("timed out while starting pane stream"),
            "hung enable must surface the pane-stream setup timeout: {result:#}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_pane_stream_enable_cannot_replace_a_newer_pipe() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let tmux_log = temp.path().join("tmux.log");
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            format!(
                "#!/bin/sh\nif [ \"$#\" -eq 3 ]; then printf 'stop\\n' >> '{}'; else printf 'enable\\n' >> '{}'; fi\n",
                tmux_log.display(),
                tmux_log.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let run_as = temp.path().join("run-as");
        std::fs::write(
            &run_as,
            "#!/bin/sh\nif [ \"$#\" -eq 5 ]; then\n  (sleep 7; exec \"$@\") &\n  sleep 8 >/dev/null 2>&1\n  exit 0\nfi\nexec \"$@\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&run_as, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![run_as.to_string_lossy().into_owned()],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let result = start_pane_stream(temp.path(), "%1", &invocation).await;
        assert!(result.is_err(), "the delayed enable must fail setup");
        tokio::time::sleep(Duration::from_secs(2)).await;

        assert_eq!(
            std::fs::read_to_string(tmux_log).unwrap(),
            "stop\n",
            "an expired enable must never run after setup cleanup"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_pane_stream_stop_cannot_run_after_owner_exits() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let stop_log = temp.path().join("stop.log");
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            format!("#!/bin/sh\nprintf 'stop\\n' >> '{}'\n", stop_log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let run_as = temp.path().join("run-as");
        std::fs::write(
            &run_as,
            "#!/bin/sh\n(sleep 7; exec \"$@\") &\nsleep 8 >/dev/null 2>&1\n",
        )
        .unwrap();
        std::fs::set_permissions(&run_as, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![run_as.to_string_lossy().into_owned()],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let error = stop_pane_stream("%1", &invocation)
            .await
            .expect_err("the delayed teardown wrapper must time out");
        assert!(
            format!("{error:#}").contains("timed out while stopping pane stream"),
            "delayed teardown must surface a timeout: {error:#}"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;

        assert!(
            !stop_log.exists(),
            "a teardown that lost ownership must not run pipe-pane later"
        );
    }

    /// Dropping a `JoinHandle` detaches the blocking task instead of aborting
    /// it, so a setup task abandoned by the setup deadline still runs to
    /// completion and creates its FIFOs. It must unlink them itself.
    #[cfg(unix)]
    #[tokio::test]
    async fn abandoned_pane_stream_setup_task_unlinks_the_fifos_it_creates() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let probe_started = temp.path().join("probe-started");
        let release_probe = temp.path().join("release-probe");
        let run_as = temp.path().join("run-as");
        std::fs::write(
            &run_as,
            "#!/bin/sh\nstarted=$1\nrelease=$2\nshift 2\nif [ \"$1\" = /bin/test ]; then\n  : > \"$started\"\n  while [ ! -e \"$release\" ]; do sleep 1; done\n  exit 0\nfi\nexec \"$@\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&run_as, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![
                run_as.to_string_lossy().into_owned(),
                probe_started.to_string_lossy().into_owned(),
                release_probe.to_string_lossy().into_owned(),
            ],
            socket: None,
            tmux_bin: "/usr/bin/true".to_string(),
        };

        let root = temp.path().to_path_buf();
        let data_path = pane_stream_fifo_path(&root, "fifo");
        let termination_path = pane_stream_fifo_path(&root, "done.fifo");
        let task_root = root.clone();
        let handle = tokio::task::spawn_blocking(move || {
            prepare_pane_stream_task(
                &task_root,
                &data_path,
                &termination_path,
                true,
                &invocation,
            )
        });

        tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, async {
            while !probe_started.exists() {
                tokio::time::sleep(PANE_STREAM_READ_RETRY_DELAY).await;
            }
        })
        .await
        .expect("traversability probe must start");

        // This is exactly what an expired `timeout_at` does to the setup task.
        drop(handle);
        std::fs::write(&release_probe, b"release").unwrap();

        let stream_dir = root.join("run-states");
        tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, async {
            // The detached task still reaches FIFO creation...
            while !stream_dir.exists() {
                tokio::time::sleep(PANE_STREAM_READ_RETRY_DELAY).await;
            }
            // ...and must clean up after itself once its result is dropped.
            while std::fs::read_dir(&stream_dir).unwrap().count() != 0 {
                tokio::time::sleep(PANE_STREAM_READ_RETRY_DELAY).await;
            }
        })
        .await
        .expect("an abandoned setup task must unlink the FIFOs it created");
    }

    /// The pane-stream enable must be bounded by whatever is left of the shared
    /// setup budget instead of stacking `DEFAULT_TMUX_COMMAND_TIMEOUT` on top of
    /// it.
    #[cfg(unix)]
    #[test]
    fn tmux_commands_can_be_bounded_below_the_default_timeout() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(&fake_tmux, "#!/bin/sh\nsleep 60 </dev/null >/dev/null 2>&1\n").unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let started = std::time::Instant::now();
        let error = run_tmux_status_with_timeout(
            &invocation,
            &["pipe-pane", "-t", "%1", "cat > /dev/null"],
            Duration::from_millis(300),
            "starting pane stream",
        )
        .expect_err("a hanging tmux command must fail");
        let elapsed = started.elapsed();

        assert!(
            format!("{error:#}").contains("timed out"),
            "a bounded tmux command must surface a timeout: {error:#}"
        );
        assert!(
            elapsed < tmux::DEFAULT_TMUX_COMMAND_TIMEOUT,
            "the caller's timeout must win over the default one, took {elapsed:?}"
        );
    }

    /// A `pipe-pane` enable that never returns must fail setup inside the setup
    /// budget, must be waited out (not abandoned) before teardown, and must not
    /// leave FIFOs behind.
    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_slow_enable_fails_setup_without_leaking() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_tmux = temp.path().join("tmux");
        let ticks_path = temp.path().join("enable-ticks");
        // The enable carries a shell command argument; teardown does not. While
        // the enable hangs it appends a tick, so the tick count tells us whether
        // it was attempted and whether it is still alive after setup failed.
        std::fs::write(
            &fake_tmux,
            format!(
                "#!/bin/sh\nif [ \"$#\" -le 3 ]; then\n  exit 0\nfi\nexec </dev/null >/dev/null 2>&1\nwhile :; do\n  printf . >> '{}'\n  sleep 0.1\ndone\n",
                ticks_path.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let setup_error = tokio::time::timeout(
            PANE_STREAM_TEST_TIMEOUT,
            start_pane_stream(temp.path(), "%1", &invocation),
        )
        .await
        .expect("a slow enable must not outlive the pane-stream test budget")
        .err()
        .expect("a slow enable must fail pane-stream setup");

        assert!(
            format!("{setup_error:#}").contains("timed out"),
            "a slow enable must surface a timeout: {setup_error:#}"
        );
        let ticks = || std::fs::read_to_string(&ticks_path).map(|t| t.len()).unwrap_or(0);
        let ticks_at_failure = ticks();
        assert!(ticks_at_failure > 0, "the enable must actually be attempted");
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            ticks(),
            ticks_at_failure,
            "setup must wait for the enable to terminate before tearing down"
        );
        let stream_dir = temp.path().join("run-states");
        if stream_dir.exists() {
            assert_eq!(
                std::fs::read_dir(stream_dir).unwrap().count(),
                0,
                "pane-stream FIFOs must not survive a slow-enable timeout"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_fifo_is_cross_user_accessible_after_umask() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\nfor arg do command=$arg; done\nprintf 'pane bytes' | /bin/sh -c \"$command\" >/dev/null 2>&1 &\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_tmux, permissions).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec!["/usr/bin/env".to_string()],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let mut stream = tokio::time::timeout(
            PANE_STREAM_TEST_TIMEOUT,
            start_pane_stream(temp.path(), "%1", &invocation),
        )
        .await
        .expect("stream setup must not block waiting for a writer")
        .unwrap();
        let stream_dir = temp.path().join("run-states");
        let fifo = std::fs::read_dir(&stream_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("fifo"))
            .expect("stream FIFO exists under SILVERBOND_ROOT");

        assert_eq!(
            std::fs::metadata(&stream_dir).unwrap().permissions().mode() & 0o777,
            0o711
        );
        assert_eq!(
            std::fs::metadata(fifo).unwrap().permissions().mode() & 0o777,
            0o622,
            "explicit chmod must restore other-write even when mkfifo applies umask"
        );

        let mut bytes = Vec::new();
        tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, stream.read_to_end(&mut bytes))
            .await
            .expect("pane stream must terminate after forwarding the writer's bytes")
            .unwrap();
        assert_eq!(bytes, b"pane bytes");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_fifo_is_owner_only_for_same_user_writer() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\nfor arg do command=$arg; done\nprintf 'pane bytes' | /bin/sh -c \"$command\" >/dev/null 2>&1 &\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let invocation = TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let mut stream = tokio::time::timeout(
            PANE_STREAM_TEST_TIMEOUT,
            start_pane_stream(temp.path(), "%1", &invocation),
        )
        .await
        .expect("stream setup must not block waiting for a same-user writer")
        .unwrap();
        let fifo = std::fs::read_dir(temp.path().join("run-states"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("fifo"))
            .expect("stream FIFO exists under SILVERBOND_ROOT");

        assert_eq!(
            std::fs::metadata(fifo).unwrap().permissions().mode() & 0o777,
            0o600,
            "same-user pane-stream FIFOs must not be writable by other users"
        );

        let mut bytes = Vec::new();
        tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, stream.read_to_end(&mut bytes))
            .await
            .expect("pane stream must terminate after forwarding the writer's bytes")
            .unwrap();
        assert_eq!(bytes, b"pane bytes");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_fifo_accepts_a_writer_from_a_different_uid() {
        use std::os::unix::fs::PermissionsExt;

        if unsafe { libc::geteuid() } != 0
            || !std::path::Path::new("/usr/bin/sudo").exists()
            || !Command::new("id")
                .args(["-u", "daemon"])
                .status()
                .is_ok_and(|status| status.success())
        {
            eprintln!("different-UID pane stream test requires root, sudo, and the daemon user");
            return;
        }

        let temp = tempfile::Builder::new()
            .prefix("silverbond-pane-stream-")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o711)).unwrap();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\nfor arg do command=$arg; done\nprintf 'cross-user bytes' | /bin/sh -c \"$command\" >/dev/null 2>&1\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_tmux, permissions).unwrap();
        let invocation = TmuxInvocation {
            prefix: vec![
                "/usr/bin/sudo".to_string(),
                "-n".to_string(),
                "-u".to_string(),
                "daemon".to_string(),
                "-H".to_string(),
                "--".to_string(),
            ],
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let mut stream = start_pane_stream(temp.path(), "%1", &invocation)
            .await
            .unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, stream.read_to_end(&mut bytes))
            .await
            .expect("different-UID writer must terminate cleanly")
            .unwrap();

        assert_eq!(bytes, b"cross-user bytes");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_stream_writer_termination_closes_the_broadcast_stream() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\nfor arg do command=$arg; done\n/bin/sh -c \"$command\" </dev/null >/dev/null 2>&1\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_tmux, permissions).unwrap();
        let paths = AppPaths::from_root(temp.path());
        let state = AppState {
            paths: paths.clone(),
            workflows: WorkflowStore::new(paths.workflows_dir.clone()),
            templates: TemplateStore::new(paths.templates_dir.clone()),
            runtime: crate::runtime::RuntimeContext::new(Database::new(
                paths.database_path.clone(),
            )),
            pane_streams: PaneStreamRegistry::default(),
            security: SecurityConfig::default(),
            unlock_throttle: UnlockThrottle::default(),
        };
        let invocation = TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        };

        let (mut receiver, sender, key) =
            subscribe_pane_stream(&state, "run-test", "pane-test", "%1", &invocation)
                .await
                .unwrap();
        drop(sender);

        let result = tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, receiver.recv())
            .await
            .expect("pane death must terminate the stream task and close its sender");
        assert!(matches!(result, Err(broadcast::error::RecvError::Closed)));
        assert!(
            !state.pane_streams.inner.lock().await.contains_key(&key),
            "terminal cleanup removes the dead pane's stream registry entry"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_death_before_live_bytes_sends_pane_stream_unavailable_frame() {
        use std::os::unix::fs::PermissionsExt;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let (temp, mut state) = create_test_state(SecurityConfig::default()).await;
        let fake_tmux = temp.path().join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\nif [ \"$1\" = capture-pane ]; then printf snapshot; exit 0; fi\nif [ \"$#\" -gt 3 ]; then for arg do command=$arg; done; /bin/sh -c \"$command\" </dev/null >/dev/null 2>&1; fi\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_tmux, permissions).unwrap();
        state.runtime.run_invocation = Some(TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        });
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        state
            .runtime
            .registry
            .set_active_pane(&run_id, "active", "%1")
            .await;
        let stream_token = state
            .runtime
            .db
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .stream_token;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router(state.clone())).await.unwrap();
        });
        let mut request = format!("ws://{address}/api/runs/{run_id}/panes/active/stream")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", format!("http://{address}").parse().unwrap());
        request
            .headers_mut()
            .insert("sec-websocket-protocol", stream_token.parse().unwrap());

        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let snapshot = tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let error = tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let tokio_tungstenite::tungstenite::Message::Text(snapshot) = snapshot else {
            panic!("expected snapshot websocket frame");
        };
        let tokio_tungstenite::tungstenite::Message::Text(error) = error else {
            panic!("expected error websocket frame");
        };
        let snapshot: Value = serde_json::from_str(snapshot.as_str()).unwrap();
        let error: Value = serde_json::from_str(error.as_str()).unwrap();

        assert_eq!(snapshot["type"], "snapshot");
        assert_eq!(error["type"], "error");
        assert_eq!(error["error"], "pane stream unavailable");

        server.abort();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pane_writer_that_never_starts_sends_pane_stream_unavailable_frame() {
        use std::os::unix::fs::PermissionsExt;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let (temp, mut state) = create_test_state(SecurityConfig::default()).await;
        let fake_tmux = temp.path().join("tmux");
        let tmux_log = temp.path().join("tmux.log");
        std::fs::write(
            &fake_tmux,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = capture-pane ]; then printf snapshot; fi\n",
                tmux_log.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_tmux, permissions).unwrap();
        state.runtime.run_invocation = Some(TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: fake_tmux.to_string_lossy().into_owned(),
        });
        let run_id = state
            .runtime
            .start_run(approval_only_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_pending_approval(&state.runtime.db, &run_id).await;
        state
            .runtime
            .registry
            .set_active_pane(&run_id, "active", "%1")
            .await;
        let stream_token = state
            .runtime
            .db
            .get_run(&run_id)
            .await
            .unwrap()
            .unwrap()
            .stream_token;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router(state.clone())).await.unwrap();
        });
        let mut request = format!("ws://{address}/api/runs/{run_id}/panes/active/stream")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", format!("http://{address}").parse().unwrap());
        request
            .headers_mut()
            .insert("sec-websocket-protocol", stream_token.parse().unwrap());

        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let snapshot = tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let tokio_tungstenite::tungstenite::Message::Text(snapshot) = snapshot else {
            panic!("expected snapshot websocket frame");
        };
        let snapshot: Value = serde_json::from_str(snapshot.as_str()).unwrap();
        let error = tokio::time::timeout(PANE_STREAM_TEST_TIMEOUT, async {
            loop {
                let message = socket.next().await.unwrap().unwrap();
                let tokio_tungstenite::tungstenite::Message::Text(message) = message else {
                    continue;
                };
                let frame: Value = serde_json::from_str(message.as_str()).unwrap();
                if frame["type"] == "error" {
                    break frame;
                }
            }
        })
        .await
        .expect("missing writer must fail within the pane stream setup timeout");

        assert_eq!(snapshot["type"], "snapshot");
        assert_eq!(error["type"], "error");
        assert_eq!(error["error"], "pane stream unavailable");
        assert!(
            std::fs::read_to_string(tmux_log)
                .unwrap()
                .lines()
                .any(|line| line == "pipe-pane -t %1"),
            "failed setup must turn pipe-pane off"
        );

        server.abort();
    }

    #[tokio::test]
    async fn pane_stream_registry_stale_unsubscribe_keeps_replacement_channel() {
        let registry = PaneStreamRegistry::default();
        let key = test_pane_stream_key("%1");
        let (stale_sender, _stale_receiver) = broadcast::channel::<Vec<u8>>(16);
        let (replacement_sender, _replacement_receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(
                key.clone(),
                PaneStreamEntry::new(replacement_sender.clone(), 1),
            );
        }

        registry.unsubscribe(&key, &stale_sender).await;

        let streams = registry.inner.lock().await;
        let entry = streams.get(&key).expect("replacement channel remains");
        assert!(entry.sender.same_channel(&replacement_sender));
        assert_eq!(entry.refcount, 1);
    }

    #[tokio::test]
    async fn subscribe_pane_stream_rejects_when_subscriber_limit_reached() {
        let mut registry = PaneStreamRegistry::default();
        registry.max_subscribers_per_pane = 1;
        let key = test_pane_stream_key("%1");
        let (sender, _receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(key.clone(), PaneStreamEntry::new(sender.clone(), 1));
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
                .get(&key)
                .map(|entry| entry.refcount),
            Some(1)
        );
    }

    #[tokio::test]
    async fn pane_stream_retry_keeps_receivers_after_transient_read_error() {
        let (sender, mut receiver) = broadcast::channel::<Vec<u8>>(16);
        let (_drain_sender, mut drain_signal) = watch::channel(false);
        let mut reader = ScriptedReader::new(vec![
            Err(io::Error::new(io::ErrorKind::Interrupted, "temporary")),
            Ok(b"chunk".to_vec()),
            Ok(Vec::new()),
        ]);

        let exit = pump_pane_stream(
            &mut reader,
            &sender,
            &mut drain_signal,
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
    async fn pane_stream_pump_honors_explicit_drain_with_receiver_alive() {
        let (sender, _receiver) = broadcast::channel::<Vec<u8>>(16);
        let (drain_sender, mut drain_signal) = watch::channel(false);
        let (_writer, mut reader) = tokio::io::duplex(1);
        drain_sender.send_replace(true);

        let exit = tokio::time::timeout(
            Duration::from_secs(1),
            pump_pane_stream(
                &mut reader,
                &sender,
                &mut drain_signal,
                "run-test",
                "pane-test",
                "%1",
                Duration::ZERO,
            ),
        )
        .await
        .expect("an explicit registry drain must stop the pane-stream pump");

        assert_eq!(exit, PaneStreamTaskExit::NoSubscribers);
    }

    #[tokio::test]
    async fn pane_stream_read_errors_stop_after_retry_limit() {
        let (sender, mut receiver) = broadcast::channel::<Vec<u8>>(16);
        let (_drain_sender, mut drain_signal) = watch::channel(false);
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
            &mut drain_signal,
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
    async fn pane_stream_task_no_subscribers_claims_teardown_before_owner_cleanup() {
        let registry = PaneStreamRegistry::default();
        let key = test_pane_stream_key("%1");
        let (sender, _receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(key.clone(), PaneStreamEntry::new(sender.clone(), 0));
        }

        let action = claim_pane_stream_task_exit(
            &registry,
            &key,
            &sender,
            PaneStreamTaskExit::NoSubscribers,
        )
        .await;
        assert_eq!(action, PaneStreamOwnerAction::Teardown);
        assert!(
            registry
                .inner
                .lock()
                .await
                .get(&key)
                .is_some_and(PaneStreamEntry::is_terminating),
            "the owner keeps its terminating entry until teardown completes"
        );

        registry.remove_terminal_sender(&key, &sender).await;
        assert!(!registry.inner.lock().await.contains_key(&key));

        let (new_sender, _new_receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(key.clone(), PaneStreamEntry::new(new_sender.clone(), 1));
        }

        let action = claim_pane_stream_task_exit(
            &registry,
            &key,
            &sender,
            PaneStreamTaskExit::NoSubscribers,
        )
        .await;
        assert_eq!(action, PaneStreamOwnerAction::Stale);
        let streams = registry.inner.lock().await;
        let entry = streams.get(&key).expect("new pane stream entry remains");
        assert!(entry.sender.same_channel(&new_sender));
        assert_eq!(entry.refcount, 1);
    }

    #[tokio::test]
    async fn pane_stream_stale_no_subscribers_exit_keeps_revived_owner() {
        let registry = PaneStreamRegistry::default();
        let key = test_pane_stream_key("%1");
        let (sender, _receiver) = broadcast::channel::<Vec<u8>>(16);
        {
            let mut streams = registry.inner.lock().await;
            streams.insert(key.clone(), PaneStreamEntry::new(sender.clone(), 1));
        }

        let action = claim_pane_stream_task_exit(
            &registry,
            &key,
            &sender,
            PaneStreamTaskExit::NoSubscribers,
        )
        .await;
        assert_eq!(action, PaneStreamOwnerAction::Continue);

        let streams = registry.inner.lock().await;
        let entry = streams
            .get(&key)
            .expect("revived pane-stream owner remains");
        assert!(entry.sender.same_channel(&sender));
        assert_eq!(entry.refcount, 1);
        assert!(!entry.is_draining());
    }

    #[tokio::test]
    async fn pane_stream_subscribe_waits_for_terminal_owner_exit() {
        let registry = PaneStreamRegistry::default();
        let state = test_app_state(registry.clone());
        let invocation = TmuxInvocation {
            prefix: Vec::new(),
            socket: None,
            tmux_bin: "/usr/bin/false".to_string(),
        };
        let key = PaneStreamKey::new(&invocation, "%1");
        let (old_sender, _old_receiver) = broadcast::channel::<Vec<u8>>(16);
        let owner_done = {
            let mut entry = PaneStreamEntry::new(old_sender.clone(), 1);
            entry.begin_termination();
            let owner_done = entry.owner_done();
            registry.inner.lock().await.insert(key.clone(), entry);
            owner_done
        };
        registry.unsubscribe(&key, &old_sender).await;

        let subscribe_state = state.clone();
        let subscribe_invocation = invocation.clone();
        let subscribe = tokio::spawn(async move {
            subscribe_pane_stream(
                &subscribe_state,
                "run-test",
                "pane-test",
                "%1",
                &subscribe_invocation,
            )
            .await
        });
        tokio::task::yield_now().await;
        assert!(
            !subscribe.is_finished(),
            "a terminal pane-stream owner must not be revived"
        );

        registry.remove_terminal_sender(&key, &old_sender).await;
        owner_done.cancel();
        let (_receiver, new_sender, _new_key) =
            tokio::time::timeout(Duration::from_secs(1), subscribe)
                .await
                .expect("subscription must resume after the terminal owner exits")
                .expect("subscription task must not panic")
                .expect("replacement pane-stream subscription must succeed");

        assert!(!old_sender.same_channel(&new_sender));
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
