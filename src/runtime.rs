use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use chrono::Utc;
use futures::future::BoxFuture;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::{
    sync::{Mutex, broadcast, oneshot},
    task::JoinSet,
};
use uuid::Uuid;

use crate::{
    driver::{self, AgentConfig, NodeOutcome},
    model::{
        self, ContextSource, ResponseFormat, SplitFailurePolicy, WorkflowEdge,
        WorkflowEdgeOutcome, WorkflowGraph, WorkflowNode, WorkflowNodeType, WorkflowV3,
        evaluate_condition, get_nested_field,
    },
    session::SessionManager,
    storage::Database,
    util::{djb2, now_iso, slugify_filename},
};

use model::DEFAULT_AGENT;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

impl RuntimeEvent {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            data: Map::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        self.data.insert(
            key.into(),
            serde_json::to_value(value).unwrap_or(Value::Null),
        );
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeResult {
    pub success: bool,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub stderr: String,
    pub exit_code: i32,
    pub duration: String,
    pub agent: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsed_output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_prompt: Option<String>,
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_from_run_id: Option<String>,
    // --- Agent metadata (populated from AgentOutput when using JSON mode) ---
    #[serde(flatten)]
    pub metadata: AgentExecutionMetadata,
}

/// Shared agent execution metadata — embedded in both `NodeResult` and `NodeExecutionLog`
/// to avoid field duplication.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<NodeOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_used_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cursor_id: String,
    pub node_id: String,
    pub node_name: String,
    pub prompt: String,
    pub last_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CursorRuntimeState {
    #[default]
    Runnable,
    Running,
    WaitingCollector,
    WaitingApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CursorTerminalStatus {
    Success,
    Failure,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CursorState {
    pub cursor_id: String,
    pub node_id: String,
    #[serde(default)]
    pub execution_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_cursor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming_edge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub split_family_ids: Vec<String>,
    #[serde(default)]
    pub last_output: String,
    #[serde(default)]
    pub loop_counters: BTreeMap<String, u32>,
    #[serde(default)]
    pub visit_counters: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_branch_origin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_branch_choice: Option<String>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub state: CursorRuntimeState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SplitFamilyState {
    pub family_id: String,
    pub split_node_id: String,
    pub execution_epoch: u64,
    pub failure_policy: SplitFailurePolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub member_cursor_ids: Vec<String>,
    #[serde(default)]
    pub force_failed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectorInputStatus {
    pub source_node_id: String,
    pub edge_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub split_family_ids: Vec<String>,
    pub status: CursorTerminalStatus,
    pub success: bool,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub stderr: String,
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsed_output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CollectorBarrierState {
    #[serde(default)]
    pub execution_epoch: u64,
    #[serde(default)]
    pub required_inputs: Vec<String>,
    #[serde(default)]
    pub arrivals: BTreeMap<String, CollectorInputStatus>,
    #[serde(default)]
    pub waiting_cursor_ids: Vec<String>,
    #[serde(default)]
    pub released: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueuedApproval {
    #[serde(flatten)]
    pub approval: PendingApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Running,
    Paused,
    Completed,
    Failed,
    Aborted,
    Restarted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutionLog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<String>,
    pub node_id: String,
    pub node_name: String,
    pub node_type: String,
    pub agent: String,
    pub original_prompt: String,
    pub resolved_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refined_prompt: Option<String>,
    pub output: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
    pub duration: String,
    pub iteration: u32,
    pub attempts: u32,
    pub timestamp: String,
    // Agent metadata
    #[serde(flatten)]
    pub metadata: AgentExecutionMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionLog {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(default)]
    pub deterministic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_request: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_response: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionLog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<String>,
    pub from_node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_node_id: Option<String>,
    pub control_type: String,
    #[serde(default)]
    pub reason: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionLog {
    pub run_id: String,
    pub workflow_name: String,
    pub goal: String,
    pub cwd: String,
    pub start_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    pub use_orchestrator: bool,
    pub aborted: bool,
    pub total_duration: String,
    #[serde(default)]
    pub node_executions: Vec<NodeExecutionLog>,
    #[serde(default)]
    pub decisions: Vec<DecisionLog>,
    #[serde(default)]
    pub transitions: Vec<TransitionLog>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCheckpoint {
    pub run_id: String,
    pub status: RuntimeStatus,
    pub workflow_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_name: Option<String>,
    #[serde(default)]
    pub all_results: BTreeMap<String, NodeResult>,
    #[serde(default)]
    pub last_output: String,
    #[serde(default)]
    pub execution_epoch: u64,
    #[serde(default)]
    pub active_cursors: Vec<CursorState>,
    #[serde(default)]
    pub split_families: BTreeMap<String, SplitFamilyState>,
    #[serde(default)]
    pub collector_barriers: BTreeMap<String, CollectorBarrierState>,
    #[serde(default)]
    pub queued_approvals: Vec<QueuedApproval>,
    #[serde(default)]
    pub loop_counters: BTreeMap<String, u32>,
    #[serde(default)]
    pub visit_counters: BTreeMap<String, u32>,
    #[serde(default)]
    pub total_executed: u32,
    #[serde(default)]
    pub output_hashes: BTreeMap<String, Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_branch_origin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_branch_choice: Option<String>,
    #[serde(default)]
    pub var_map: BTreeMap<String, String>,
    pub goal: String,
    pub cwd: String,
    pub use_orchestrator: bool,
    pub max_total_steps: u32,
    pub max_visits_per_node: u32,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_approval: Option<PendingApproval>,
    pub execution_log: ExecutionLog,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedRun {
    pub checkpoint: RuntimeCheckpoint,
    pub workflow: WorkflowV3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedRunSummary {
    pub run_id: String,
    pub status: RuntimeStatus,
    pub workflow_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_name: Option<String>,
    pub total_executed: u32,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_approval: Option<PendingApproval>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogListItem {
    pub id: String,
    pub filename: String,
    pub workflow_name: String,
    pub goal: String,
    pub start_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    pub total_duration: String,
    pub node_execution_count: usize,
    pub decision_count: usize,
    pub aborted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_output_tokens: Option<u64>,
    #[serde(default)]
    pub nodes_succeeded: usize,
    #[serde(default)]
    pub nodes_failed: usize,
}

pub use crate::driver::AgentCapabilities;

#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: &'static str,
    pub capabilities: AgentCapabilities,
}

trait NodeRunner: Send + Sync {
    fn run(
        &self,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>>;

    /// Run with interaction context for escalation ladder support.
    fn run_with_interaction(
        &self,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
        _ctx: RuntimeContext,
        _run_id: String,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        // Default: fall back to basic run (ignoring interaction context)
        self.run(agent, prompt, cwd, timeout_secs, config)
    }
}

struct PtyNodeRunner {
    session_manager: Arc<SessionManager>,
}

impl NodeRunner for PtyNodeRunner {
    fn run(
        &self,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        let sm = self.session_manager.clone();
        Box::pin(async move {
            run_pty_command(sm, &agent, &prompt, &cwd, timeout_secs, config.as_ref()).await
        })
    }

    fn run_with_interaction(
        &self,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
        ctx: RuntimeContext,
        run_id: String,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        let sm = self.session_manager.clone();
        Box::pin(async move {
            let ictx = InteractionContext { ctx, run_id };
            run_pty_command_with_context(
                sm,
                &agent,
                &prompt,
                &cwd,
                timeout_secs,
                config.as_ref(),
                Some(&ictx),
            )
            .await
        })
    }
}

#[derive(Debug)]
struct ApprovalDecision {
    approved: bool,
    user_input: String,
}

/// Response to an interactive agent prompt (permission, question, destructive warning).
#[derive(Debug)]
struct InteractionResponse {
    response: String,
}

#[derive(Debug, Clone)]
struct ActiveRun {
    sender: broadcast::Sender<RuntimeEvent>,
    abort_flag: Arc<AtomicBool>,
    approval_sender: Arc<Mutex<Option<oneshot::Sender<ApprovalDecision>>>>,
    interaction_sender: Arc<Mutex<Option<oneshot::Sender<InteractionResponse>>>>,
}

#[derive(Debug, Clone, Default)]
pub struct RunRegistry {
    inner: Arc<Mutex<HashMap<String, ActiveRun>>>,
}

impl RunRegistry {
    async fn register(&self, run_id: &str) {
        let mut guard = self.inner.lock().await;
        guard.entry(run_id.to_string()).or_insert_with(|| {
            let (sender, _) = broadcast::channel(512);
            ActiveRun {
                sender,
                abort_flag: Arc::new(AtomicBool::new(false)),
                approval_sender: Arc::new(Mutex::new(None)),
                interaction_sender: Arc::new(Mutex::new(None)),
            }
        });
    }

    pub(crate) async fn subscribe(
        &self,
        run_id: &str,
    ) -> Option<broadcast::Receiver<RuntimeEvent>> {
        self.inner
            .lock()
            .await
            .get(run_id)
            .map(|active| active.sender.subscribe())
    }

    async fn send_event(&self, run_id: &str, event: RuntimeEvent) {
        if let Some(active) = self.inner.lock().await.get(run_id).cloned() {
            let _ = active.sender.send(event);
        }
    }

    async fn set_abort(&self, run_id: &str) {
        if let Some(active) = self.inner.lock().await.get(run_id).cloned() {
            active.abort_flag.store(true, Ordering::SeqCst);
        }
    }

    async fn is_aborted(&self, run_id: &str) -> bool {
        self.inner
            .lock()
            .await
            .get(run_id)
            .map(|active| active.abort_flag.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    async fn set_pending_approval(
        &self,
        run_id: &str,
        sender: oneshot::Sender<ApprovalDecision>,
    ) -> anyhow::Result<()> {
        let active = self
            .inner
            .lock()
            .await
            .get(run_id)
            .cloned()
            .context("run is not active")?;
        *active.approval_sender.lock().await = Some(sender);
        Ok(())
    }

    async fn resolve_approval(
        &self,
        run_id: &str,
        approved: bool,
        user_input: String,
    ) -> anyhow::Result<()> {
        let active = self
            .inner
            .lock()
            .await
            .get(run_id)
            .cloned()
            .context("no active approval for this run")?;
        let sender = active.approval_sender.lock().await.take();
        let Some(sender) = sender else {
            anyhow::bail!("no active approval for this run");
        };
        sender
            .send(ApprovalDecision {
                approved,
                user_input,
            })
            .map_err(|_| anyhow::anyhow!("approval receiver dropped"))?;
        Ok(())
    }

    async fn set_pending_interaction(
        &self,
        run_id: &str,
        sender: oneshot::Sender<InteractionResponse>,
    ) -> anyhow::Result<()> {
        let active = self
            .inner
            .lock()
            .await
            .get(run_id)
            .cloned()
            .context("run is not active")?;
        *active.interaction_sender.lock().await = Some(sender);
        Ok(())
    }

    pub(crate) async fn resolve_interaction(
        &self,
        run_id: &str,
        response: String,
    ) -> anyhow::Result<()> {
        let active = self
            .inner
            .lock()
            .await
            .get(run_id)
            .cloned()
            .context("no active interaction for this run")?;
        let sender = active.interaction_sender.lock().await.take();
        let Some(sender) = sender else {
            anyhow::bail!("no active interaction for this run");
        };
        sender
            .send(InteractionResponse { response })
            .map_err(|_| anyhow::anyhow!("interaction receiver dropped"))?;
        Ok(())
    }

    async fn clear(&self, run_id: &str) {
        self.inner.lock().await.remove(run_id);
    }
}

#[derive(Clone)]
pub struct RuntimeContext {
    pub db: Database,
    pub registry: RunRegistry,
    runner: Arc<dyn NodeRunner>,
    pub session_manager: Arc<SessionManager>,
}

impl RuntimeContext {
    pub fn new(db: Database) -> Self {
        let artifact_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("artifacts");
        let session_manager = Arc::new(SessionManager::new(artifact_dir));
        Self {
            db,
            registry: RunRegistry::default(),
            runner: Arc::new(PtyNodeRunner {
                session_manager: session_manager.clone(),
            }),
            session_manager,
        }
    }

    #[cfg(test)]
    fn with_runner(db: Database, runner: Arc<dyn NodeRunner>) -> Self {
        let session_manager = Arc::new(SessionManager::new(PathBuf::from("/tmp/test-artifacts")));
        Self {
            db,
            registry: RunRegistry::default(),
            runner,
            session_manager,
        }
    }

    pub async fn start_run(
        &self,
        workflow: WorkflowV3,
        variable_overrides: BTreeMap<String, String>,
        start_node_id: Option<String>,
    ) -> anyhow::Result<String> {
        let run_id = format!("run_{}", Uuid::now_v7());
        let checkpoint =
            build_initial_checkpoint(&workflow, &run_id, variable_overrides, start_node_id);
        self.db
            .upsert_run(&PersistedRun {
                checkpoint: checkpoint.clone(),
                workflow: workflow.clone(),
            })
            .await?;
        self.registry.register(&run_id).await;
        let ctx = self.clone();
        tokio::spawn(async move {
            let _ = execute_workflow(ctx, workflow, checkpoint, false).await;
        });
        Ok(run_id)
    }

    pub async fn resume_run(&self, run_id: &str) -> anyhow::Result<()> {
        let persisted = self
            .db
            .get_run(run_id)
            .await?
            .context("No checkpoint found for this run")?;
        anyhow::ensure!(
            matches!(
                persisted.checkpoint.status,
                RuntimeStatus::Running | RuntimeStatus::Paused
            ),
            "Run is in terminal state: {:?}",
            persisted.checkpoint.status
        );
        self.registry.register(run_id).await;
        let ctx = self.clone();
        tokio::spawn(async move {
            let _ = execute_workflow(ctx, persisted.workflow, persisted.checkpoint, true).await;
        });
        Ok(())
    }

    pub async fn restart_from(&self, run_id: &str, node_id: &str) -> anyhow::Result<String> {
        let persisted = self
            .db
            .get_run(run_id)
            .await?
            .context("No checkpoint found for this run")?;
        anyhow::ensure!(
            persisted
                .workflow
                .nodes
                .iter()
                .any(|node| node.id == node_id),
            "Node \"{}\" not found in workflow",
            node_id
        );

        let descendants = collect_descendants(&persisted.workflow, node_id);
        let mut checkpoint = persisted.checkpoint.clone();
        checkpoint.execution_epoch = checkpoint.execution_epoch.saturating_add(1);
        checkpoint.current_node_id = Some(node_id.to_string());
        checkpoint.current_node_name = persisted
            .workflow
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .map(|node| node.name.clone());
        checkpoint.status = RuntimeStatus::Running;
        checkpoint.pending_approval = None;
        checkpoint.queued_approvals.clear();
        checkpoint.active_cursors = vec![CursorState {
            cursor_id: new_cursor_id(),
            node_id: node_id.to_string(),
            execution_epoch: checkpoint.execution_epoch,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: String::new(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::Runnable,
        }];
        checkpoint.split_families.clear();
        checkpoint.collector_barriers.clear();
        checkpoint.last_branch_choice = None;
        checkpoint.last_branch_origin_id = None;

        for descendant in descendants {
            checkpoint.all_results.remove(&descendant);
            checkpoint.loop_counters.remove(&descendant);
            checkpoint.visit_counters.remove(&descendant);
            checkpoint.output_hashes.remove(&descendant);
        }

        for result in checkpoint.all_results.values_mut() {
            result.stale = true;
            result.preserved_from_run_id = Some(run_id.to_string());
        }

        checkpoint.last_output = checkpoint
            .all_results
            .values()
            .last()
            .map(|result| result.output.clone())
            .unwrap_or_default();
        if let Some(cursor) = checkpoint.active_cursors.first_mut() {
            cursor.last_output = checkpoint.last_output.clone();
        }
        checkpoint.run_id = format!("run_{}", Uuid::now_v7());
        checkpoint.started_at = now_iso();
        checkpoint.updated_at = checkpoint.started_at.clone();
        checkpoint.execution_log.run_id = checkpoint.run_id.clone();
        checkpoint.execution_log.start_time = checkpoint.started_at.clone();
        checkpoint.execution_log.end_time = None;
        checkpoint.execution_log.total_duration = "0".to_string();
        checkpoint.execution_log.terminal_reason = None;
        checkpoint.execution_log.aborted = false;

        let new_run_id = checkpoint.run_id.clone();
        self.db
            .upsert_run(&PersistedRun {
                checkpoint: checkpoint.clone(),
                workflow: persisted.workflow.clone(),
            })
            .await?;
        self.db
            .mark_run_status(
                run_id,
                RuntimeStatus::Restarted,
                Some("restarted".to_string()),
            )
            .await?;
        self.registry.register(&new_run_id).await;

        let ctx = self.clone();
        tokio::spawn(async move {
            let _ = execute_workflow(ctx, persisted.workflow, checkpoint, true).await;
        });
        Ok(new_run_id)
    }

    pub async fn approve_run(
        &self,
        run_id: &str,
        approved: bool,
        user_input: String,
    ) -> anyhow::Result<()> {
        self.registry
            .resolve_approval(run_id, approved, user_input)
            .await
    }

    pub async fn abort_run(&self, run_id: &str) -> anyhow::Result<()> {
        self.registry.set_abort(run_id).await;
        Ok(())
    }

    pub async fn respond_interaction(
        &self,
        run_id: &str,
        response: String,
    ) -> anyhow::Result<()> {
        self.registry
            .resolve_interaction(run_id, response)
            .await
    }
}

pub fn available_agents() -> Vec<AgentSpec> {
    driver::all_drivers()
        .into_iter()
        .map(|d| AgentSpec {
            // SAFETY: driver names are static strings from driver implementations
            name: match d.name() {
                "claude" => "claude",
                "codex" => "codex",
                "gemini" => "gemini",
                other => {
                    // Leak the string to get a &'static str — only called once at startup
                    Box::leak(other.to_string().into_boxed_str())
                }
            },
            capabilities: d.capabilities(),
        })
        .collect()
}

pub fn find_agent(name: &str) -> Option<AgentSpec> {
    let drv = driver::get_driver(name)?;
    Some(AgentSpec {
        name: match name {
            "claude" => "claude",
            "codex" => "codex",
            "gemini" => "gemini",
            other => Box::leak(other.to_string().into_boxed_str()),
        },
        capabilities: drv.capabilities(),
    })
}

pub async fn check_cli(command: &str) -> anyhow::Result<(bool, String)> {
    let search_paths = executable_search_paths(std::env::var_os("PATH").as_deref());
    let resolved = resolve_executable(command, &search_paths);
    Ok((
        resolved.is_some(),
        resolved
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    ))
}

fn executable_search_paths(path_var: Option<&OsStr>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    let mut push = |entry: PathBuf| {
        if !entry.as_os_str().is_empty() && seen.insert(entry.clone()) {
            paths.push(entry);
        }
    };
    if let Some(path_var) = path_var {
        for entry in std::env::split_paths(path_var) {
            push(entry);
        }
    }
    #[cfg(target_os = "macos")]
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        push(PathBuf::from(extra));
    }
    paths
}

fn resolve_executable(command: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return is_executable_file(path).then(|| path.to_path_buf());
    }
    search_paths
        .iter()
        .map(|base| base.join(command))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolve executable path for an agent command, searching the given paths.
fn resolve_agent_executable(
    agent_name: &str,
    search_paths: &[PathBuf],
) -> anyhow::Result<PathBuf> {
    resolve_executable(agent_name, search_paths).with_context(|| {
        format!(
            "Could not find '{}' on PATH. Install the CLI and ensure it is available to SilverBond.",
            agent_name
        )
    })
}

pub async fn run_node_preview(
    node: &WorkflowNode,
    cwd: &str,
    mock_context: NodeTestContext,
) -> anyhow::Result<NodePreviewResult> {
    let inbound: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_results = BTreeMap::new();
    for (node_id, output) in mock_context.node_outputs {
        all_results.insert(
            node_id,
            NodeResult {
                success: true,
                output,
                duration: "0.0".to_string(),
                agent: "mock".to_string(),
                ..Default::default()
            },
        );
    }

    let resolved_prompt = resolve_template_vars(
        &node.prompt,
        &TemplateRuntimeContext {
            current_node_id: &node.id,
            current_node: node,
            all_results: &all_results,
            last_output: &mock_context.previous_output,
            var_map: &mock_context.variables,
            inbound_map: &inbound,
            last_branch_origin_id: mock_context.branch_origin.as_deref(),
            last_branch_choice: mock_context.branch_choice.as_deref(),
        },
    );

    let agent = node.agent.clone().unwrap_or_else(|| DEFAULT_AGENT.to_string());
    let config = crate::model::resolve_agent_config(
        &BTreeMap::new(),
        cwd,
        &agent,
        node,
        None,
        false,
        None,
    );
    let resolved_prompt = wrap_prompt_for_json(node, resolved_prompt, &None);
    let sm = Arc::new(SessionManager::new(PathBuf::from("/tmp/silverbond-preview")));
    let mut result = run_pty_command(sm, &agent, &resolved_prompt, cwd, node.timeout, Some(&config)).await?;
    parse_structured_output(node, &mut result);

    let routing_preview = preview_routing(node, &result.parsed_output);

    Ok(NodePreviewResult {
        resolved_prompt,
        parsed_output: result.parsed_output.clone(),
        parse_error: result.parse_error.clone(),
        routing_preview,
        result,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTestContext {
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub node_outputs: BTreeMap<String, String>,
    #[serde(default, alias = "previous_output")]
    pub previous_output: String,
    #[serde(
        default,
        alias = "branch_origin",
        skip_serializing_if = "Option::is_none"
    )]
    pub branch_origin: Option<String>,
    #[serde(
        default,
        alias = "branch_choice",
        skip_serializing_if = "Option::is_none"
    )]
    pub branch_choice: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePreviewResult {
    pub resolved_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_preview: Option<Value>,
    #[serde(flatten)]
    pub result: NodeResult,
}


struct ActiveApprovalWait {
    cursor_id: String,
    receiver: oneshot::Receiver<ApprovalDecision>,
}

struct CursorTaskResult {
    cursor_id: String,
    node: WorkflowNode,
    result: NodeResult,
    resolved_prompt: String,
    refined_prompt: Option<String>,
    iteration: u32,
    attempts: u32,
}

struct NextDecision {
    next_edge: Option<WorkflowEdge>,
    control_type: String,
    reason: String,
}

#[derive(Clone)]
struct CollectorTarget {
    collector_id: String,
    inbound_edge: WorkflowEdge,
}

/// Constant data shared across all cursor tasks for a single run execution.
#[derive(Clone)]
struct RunConstantData {
    inbound_map: HashMap<String, Vec<String>>,
    agent_defaults: BTreeMap<String, crate::model::AgentDefaults>,
    session_persistence_nodes: HashSet<String>,
}

struct CursorTaskExecutionContext {
    run_id: String,
    workflow_goal: String,
    workflow_nodes_len: usize,
    cwd: String,
    use_orchestrator: bool,
    total_executed: u32,
    all_results: BTreeMap<String, NodeResult>,
    var_map: BTreeMap<String, String>,
    /// Constant-per-run data shared via Arc to avoid cloning on each dispatch.
    shared: Arc<RunConstantData>,
}

fn new_cursor_id() -> String {
    format!("cursor_{}", Uuid::now_v7())
}

fn new_split_family_id() -> String {
    format!("family_{}", Uuid::now_v7())
}

fn merge_key_for_edge(edge: &WorkflowEdge) -> String {
    edge.label.clone().unwrap_or_else(|| edge.from.clone())
}

/// Pre-scan workflow nodes: collect node IDs referenced by `continue_session_from` so
/// those nodes run with persistent sessions (ephemeral_session = false).
fn build_session_persistence_set(workflow: &WorkflowV3) -> HashSet<String> {
    workflow
        .nodes
        .iter()
        .filter_map(|n| n.continue_session_from.clone())
        .collect()
}

fn build_inbound_source_map(graph: &WorkflowGraph) -> HashMap<String, Vec<String>> {
    graph
        .inbound
        .iter()
        .map(|(node_id, edges)| {
            (
                (*node_id).to_string(),
                edges.iter().map(|edge| edge.from.clone()).collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn build_initial_checkpoint(
    workflow: &WorkflowV3,
    run_id: &str,
    variable_overrides: BTreeMap<String, String>,
    start_node_id: Option<String>,
) -> RuntimeCheckpoint {
    let workflow_name = workflow.name.clone().unwrap_or_else(|| {
        if workflow.goal.is_empty() {
            "Untitled".to_string()
        } else {
            workflow.goal.clone()
        }
    });
    let started_at = now_iso();
    let start_node_id = start_node_id.unwrap_or_else(|| workflow.entry_node_id.clone());
    let current_node_name = workflow
        .nodes
        .iter()
        .find(|node| node.id == start_node_id)
        .map(|node| node.name.clone());
    let mut var_map = BTreeMap::new();
    for variable in &workflow.variables {
        var_map.insert(
            variable.name.clone(),
            variable_overrides
                .get(&variable.name)
                .cloned()
                .unwrap_or_else(|| variable.default.clone()),
        );
    }

    RuntimeCheckpoint {
        run_id: run_id.to_string(),
        status: RuntimeStatus::Running,
        workflow_name: workflow_name.clone(),
        current_node_id: Some(start_node_id.clone()),
        current_node_name,
        all_results: BTreeMap::new(),
        last_output: String::new(),
        execution_epoch: 1,
        active_cursors: vec![CursorState {
            cursor_id: new_cursor_id(),
            node_id: start_node_id,
            execution_epoch: 1,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: String::new(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::Runnable,
        }],
        split_families: BTreeMap::new(),
        collector_barriers: BTreeMap::new(),
        queued_approvals: Vec::new(),
        loop_counters: BTreeMap::new(),
        visit_counters: BTreeMap::new(),
        total_executed: 0,
        output_hashes: BTreeMap::new(),
        last_branch_origin_id: None,
        last_branch_choice: None,
        var_map,
        goal: workflow.goal.clone(),
        cwd: workflow.cwd.clone(),
        use_orchestrator: workflow.use_orchestrator,
        max_total_steps: workflow.limits.max_total_steps,
        max_visits_per_node: workflow.limits.max_visits_per_node,
        started_at: started_at.clone(),
        updated_at: started_at.clone(),
        pending_approval: None,
        execution_log: ExecutionLog {
            run_id: run_id.to_string(),
            workflow_name,
            goal: workflow.goal.clone(),
            cwd: workflow.cwd.clone(),
            start_time: started_at,
            end_time: None,
            use_orchestrator: workflow.use_orchestrator,
            aborted: false,
            total_duration: "0".to_string(),
            node_executions: Vec::new(),
            decisions: Vec::new(),
            transitions: Vec::new(),
            terminal_reason: None,
        },
    }
}

fn rehydrate_checkpoint_for_execution(workflow: &WorkflowV3, checkpoint: &mut RuntimeCheckpoint) {
    if checkpoint.execution_epoch == 0 {
        checkpoint.execution_epoch = 1;
    }
    if checkpoint.active_cursors.is_empty() {
        if let Some(node_id) = checkpoint.current_node_id.clone() {
            checkpoint.active_cursors.push(CursorState {
                cursor_id: new_cursor_id(),
                node_id,
                execution_epoch: checkpoint.execution_epoch,
                parent_cursor_id: None,
                incoming_edge_id: None,
                incoming_node_id: None,
                split_family_ids: Vec::new(),
                last_output: checkpoint.last_output.clone(),
                loop_counters: checkpoint.loop_counters.clone(),
                visit_counters: checkpoint.visit_counters.clone(),
                last_branch_origin_id: checkpoint.last_branch_origin_id.clone(),
                last_branch_choice: checkpoint.last_branch_choice.clone(),
                cancel_requested: false,
                state: if checkpoint.pending_approval.is_some() {
                    CursorRuntimeState::WaitingApproval
                } else {
                    CursorRuntimeState::Runnable
                },
            });
        }
    }
    checkpoint.active_cursors.retain(|cursor| !cursor.cancel_requested);
    for cursor in &mut checkpoint.active_cursors {
        if cursor.state == CursorRuntimeState::Running {
            cursor.state = CursorRuntimeState::Runnable;
        }
    }
    if checkpoint.pending_approval.is_some()
        && !checkpoint
            .active_cursors
            .iter()
            .any(|cursor| cursor.state == CursorRuntimeState::WaitingApproval)
    {
        checkpoint.pending_approval = None;
    }
    update_checkpoint_summary(workflow, checkpoint);
}

fn update_checkpoint_summary(workflow: &WorkflowV3, checkpoint: &mut RuntimeCheckpoint) {
    if let Some(pending) = &checkpoint.pending_approval {
        checkpoint.current_node_id = Some(pending.node_id.clone());
        checkpoint.current_node_name = Some(pending.node_name.clone());
        return;
    }
    let Some(cursor) = checkpoint
        .active_cursors
        .iter()
        .find(|cursor| !cursor.cancel_requested)
    else {
        checkpoint.current_node_id = None;
        checkpoint.current_node_name = None;
        return;
    };
    checkpoint.current_node_id = Some(cursor.node_id.clone());
    checkpoint.current_node_name = workflow
        .nodes
        .iter()
        .find(|node| node.id == cursor.node_id)
        .map(|node| node.name.clone());
}

fn find_cursor_index(checkpoint: &RuntimeCheckpoint, cursor_id: &str) -> Option<usize> {
    checkpoint
        .active_cursors
        .iter()
        .position(|cursor| cursor.cursor_id == cursor_id)
}

fn cursor_snapshot(checkpoint: &RuntimeCheckpoint, cursor_id: &str) -> Option<CursorState> {
    checkpoint
        .active_cursors
        .iter()
        .find(|cursor| cursor.cursor_id == cursor_id)
        .cloned()
}

fn select_success_edge(graph: &WorkflowGraph, node_id: &str) -> Option<WorkflowEdge> {
    graph
        .outgoing_for(node_id)
        .iter()
        .find(|edge| edge.outcome == WorkflowEdgeOutcome::Success)
        .map(|edge| (*edge).clone())
}

fn should_skip_cursor_node(
    node: &WorkflowNode,
    cursor: &CursorState,
    checkpoint: &RuntimeCheckpoint,
) -> bool {
    let Some(skip) = &node.skip_condition else {
        return false;
    };
    let source_text = if skip.source == "previous_output" {
        cursor.last_output.clone()
    } else {
        checkpoint
            .all_results
            .get(&skip.source)
            .map(|result| result.output.clone())
            .unwrap_or_default()
    };
    match skip.kind.as_str() {
        "contains" => source_text.contains(&skip.value),
        "not_contains" => !source_text.contains(&skip.value),
        "regex" => Regex::new(&skip.value)
            .map(|regex| regex.is_match(&source_text))
            .unwrap_or(false),
        _ => false,
    }
}

fn nearest_collectors_for_node(graph: &WorkflowGraph, node_id: &str) -> Vec<CollectorTarget> {
    let mut queue = VecDeque::from([(node_id.to_string(), 0usize)]);
    let mut visited = BTreeSet::new();
    let mut found_distance = None;
    let mut targets = BTreeMap::new();

    while let Some((current, distance)) = queue.pop_front() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if found_distance.is_some_and(|best| distance > best) {
            continue;
        }
        for edge in graph.outgoing_for(&current) {
            if edge.outcome != WorkflowEdgeOutcome::Success {
                continue;
            }
            let Some(target_node) = graph.node_map.get(edge.to.as_str()) else {
                continue;
            };
            if target_node.node_type == WorkflowNodeType::Collector {
                found_distance.get_or_insert(distance + 1);
                targets.insert(
                    (edge.to.clone(), merge_key_for_edge(edge)),
                    CollectorTarget {
                        collector_id: edge.to.clone(),
                        inbound_edge: (*edge).clone(),
                    },
                );
                continue;
            }
            if found_distance.is_none() {
                queue.push_back((edge.to.clone(), distance + 1));
            }
        }
    }

    targets.into_values().collect()
}

async fn execute_workflow(
    ctx: RuntimeContext,
    workflow: WorkflowV3,
    mut checkpoint: RuntimeCheckpoint,
    resumed: bool,
) -> anyhow::Result<()> {
    rehydrate_checkpoint_for_execution(&workflow, &mut checkpoint);
    let start_instant = std::time::Instant::now();
    let run_id = checkpoint.run_id.clone();
    let graph = workflow.graph();
    let run_shared = Arc::new(RunConstantData {
        inbound_map: build_inbound_source_map(&graph),
        agent_defaults: workflow.agent_defaults.clone(),
        session_persistence_nodes: build_session_persistence_set(&workflow),
    });
    let mut running_tasks = JoinSet::new();
    let mut active_approval: Option<ActiveApprovalWait> = None;

    if resumed {
        emit_event(
            &ctx,
            &run_id,
            RuntimeEvent::new("run_resumed").with("runId", run_id.clone()),
        )
        .await?;
        if checkpoint.pending_approval.is_some() {
            restore_pending_approval(&ctx, &mut checkpoint, &mut active_approval).await?;
        }
    } else {
        emit_event(
            &ctx,
            &run_id,
            RuntimeEvent::new("run_start").with("runId", run_id.clone()),
        )
        .await?;
    }
    persist_checkpoint(&ctx, &workflow, &mut checkpoint).await?;

    loop {
        if ctx.registry.is_aborted(&run_id).await {
            emit_event(
                &ctx,
                &run_id,
                RuntimeEvent::new("workflow_error").with("message", "Workflow aborted by user."),
            )
            .await?;
            checkpoint.status = RuntimeStatus::Aborted;
            checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
            running_tasks.abort_all();
            while running_tasks.join_next().await.is_some() {}
            checkpoint.active_cursors.clear();
            checkpoint.pending_approval = None;
            checkpoint.queued_approvals.clear();
            break;
        }

        let mut changed = false;
        while process_immediate_cursors(
            &ctx,
            &workflow,
            &graph,
            &mut checkpoint,
            &mut active_approval,
        )
        .await?
        {
            changed = true;
            if checkpoint.execution_log.terminal_reason.is_some() {
                break;
            }
        }

        if checkpoint.execution_log.terminal_reason.is_some() {
            running_tasks.abort_all();
            while running_tasks.join_next().await.is_some() {}
            checkpoint.active_cursors.clear();
            checkpoint.pending_approval = None;
            checkpoint.queued_approvals.clear();
            break;
        }

        let dispatchable = checkpoint
            .active_cursors
            .iter()
            .filter(|cursor| cursor.state == CursorRuntimeState::Runnable && !cursor.cancel_requested)
            .map(|cursor| cursor.cursor_id.clone())
            .collect::<Vec<_>>();

        for cursor_id in dispatchable {
            let Some(cursor) = cursor_snapshot(&checkpoint, &cursor_id) else {
                continue;
            };
            let Some(node) = graph
                .node_map
                .get(cursor.node_id.as_str())
                .map(|node| (*node).clone())
            else {
                emit_event(
                    &ctx,
                    &run_id,
                    RuntimeEvent::new("workflow_error")
                        .with("cursorId", cursor_id.clone())
                        .with(
                            "message",
                            format!("Node \"{}\" not found — aborting.", cursor.node_id),
                        ),
                )
                .await?;
                checkpoint.status = RuntimeStatus::Aborted;
                checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
                break;
            };
            if node.node_type != WorkflowNodeType::Task {
                continue;
            }
            let Some(iteration) =
                prepare_cursor_visit(&ctx, &workflow, &mut checkpoint, &cursor_id, &node).await?
            else {
                break;
            };
            if let Some(index) = find_cursor_index(&checkpoint, &cursor_id) {
                checkpoint.active_cursors[index].state = CursorRuntimeState::Running;
            }
            let dispatch_cursor = cursor_snapshot(&checkpoint, &cursor_id).unwrap_or(cursor);
            let run_ctx = CursorTaskExecutionContext {
                run_id: run_id.clone(),
                workflow_goal: workflow.goal.clone(),
                workflow_nodes_len: workflow.nodes.len(),
                cwd: checkpoint.cwd.clone(),
                use_orchestrator: checkpoint.use_orchestrator,
                total_executed: checkpoint.total_executed,
                all_results: checkpoint.all_results.clone(),
                var_map: checkpoint.var_map.clone(),
                shared: run_shared.clone(),
            };
            running_tasks.spawn(run_cursor_task(
                ctx.clone(),
                run_ctx,
                dispatch_cursor,
                node,
                iteration,
            ));
            changed = true;
        }

        update_checkpoint_summary(&workflow, &mut checkpoint);
        if changed {
            persist_checkpoint(&ctx, &workflow, &mut checkpoint).await?;
        }

        if running_tasks.is_empty() {
            if active_approval.is_some() {
                wait_for_approval(
                    &ctx,
                    &graph,
                    &mut checkpoint,
                    &mut active_approval,
                )
                .await?;
                update_checkpoint_summary(&workflow, &mut checkpoint);
                persist_checkpoint(&ctx, &workflow, &mut checkpoint).await?;
                continue;
            }

            if checkpoint.active_cursors.is_empty() {
                break;
            }

            if checkpoint
                .active_cursors
                .iter()
                .all(|cursor| cursor.state == CursorRuntimeState::WaitingCollector)
            {
                emit_event(
                    &ctx,
                    &run_id,
                    RuntimeEvent::new("workflow_error")
                        .with("message", "Collector is blocked waiting on missing inputs."),
                )
                .await?;
                checkpoint.status = RuntimeStatus::Failed;
                checkpoint.execution_log.terminal_reason = Some("failed".to_string());
                checkpoint.active_cursors.clear();
                break;
            }

            continue;
        }

        if active_approval.is_some() {
            let wait = active_approval.as_mut().expect("approval state should exist");
            tokio::select! {
                task = running_tasks.join_next() => {
                    if let Some(task) = task {
                        apply_join_result(&ctx, &workflow, &graph, &mut checkpoint, task).await?;
                    }
                }
                decision = &mut wait.receiver => {
                    let decision = decision.ok();
                    if let Some(wait) = active_approval.take() {
                        handle_approval_resolution(&ctx, &graph, &mut checkpoint, wait.cursor_id, decision).await?;
                    }
                }
            }
        } else if let Some(task) = running_tasks.join_next().await {
            apply_join_result(&ctx, &workflow, &graph, &mut checkpoint, task).await?;
        }

        update_checkpoint_summary(&workflow, &mut checkpoint);
        persist_checkpoint(&ctx, &workflow, &mut checkpoint).await?;
    }

    finalize_run(&ctx, &workflow, checkpoint, start_instant.elapsed()).await
}

async fn process_immediate_cursors(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    active_approval: &mut Option<ActiveApprovalWait>,
) -> anyhow::Result<bool> {
    if active_approval.is_none() && checkpoint.pending_approval.is_none() {
        if activate_next_approval(ctx, checkpoint, active_approval).await? {
            return Ok(true);
        }
    }
    let runnable_ids = checkpoint
        .active_cursors
        .iter()
        .filter(|cursor| cursor.state == CursorRuntimeState::Runnable && !cursor.cancel_requested)
        .map(|cursor| cursor.cursor_id.clone())
        .collect::<Vec<_>>();

    for cursor_id in runnable_ids {
        let Some(cursor) = cursor_snapshot(checkpoint, &cursor_id) else {
            continue;
        };
        let Some(node) = graph
            .node_map
            .get(cursor.node_id.as_str())
            .map(|node| (*node).clone())
        else {
            emit_event(
                ctx,
                &checkpoint.run_id,
                RuntimeEvent::new("workflow_error")
                    .with("cursorId", cursor_id.clone())
                    .with(
                        "message",
                        format!("Node \"{}\" not found — aborting.", cursor.node_id),
                    ),
            )
            .await?;
            checkpoint.status = RuntimeStatus::Aborted;
            checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
            return Ok(true);
        };

        match node.node_type {
            WorkflowNodeType::Task => {
                if should_skip_cursor_node(&node, &cursor, checkpoint) {
                    let Some(_) =
                        prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                    else {
                        return Ok(true);
                    };
                    handle_skipped_task(ctx, workflow, graph, checkpoint, cursor_id, node).await?;
                    return Ok(true);
                }
            }
            WorkflowNodeType::Approval => {
                let Some(_) =
                    prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                else {
                    return Ok(true);
                };
                queue_approval(ctx, checkpoint, cursor_id, node).await?;
                return Ok(true);
            }
            WorkflowNodeType::Split => {
                let Some(_) =
                    prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                else {
                    return Ok(true);
                };
                handle_split_node(ctx, graph, checkpoint, cursor_id, node).await?;
                return Ok(true);
            }
            WorkflowNodeType::Collector => {
                let Some(_) =
                    prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                else {
                    return Ok(true);
                };
                handle_collector_entry(ctx, graph, checkpoint, cursor_id, node).await?;
                return Ok(true);
            }
        }
    }

    Ok(false)
}

async fn prepare_cursor_visit(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: &str,
    node: &WorkflowNode,
) -> anyhow::Result<Option<u32>> {
    checkpoint.total_executed += 1;
    if checkpoint.total_executed > checkpoint.max_total_steps {
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_error")
                .with("cursorId", cursor_id.to_string())
                .with(
                    "message",
                    format!(
                        "Global execution cap ({}) reached — aborting to prevent infinite loop.",
                        checkpoint.max_total_steps
                    ),
                ),
        )
        .await?;
        checkpoint.status = RuntimeStatus::Aborted;
        checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
        checkpoint.active_cursors.clear();
        update_checkpoint_summary(workflow, checkpoint);
        return Ok(None);
    }

    let Some(index) = find_cursor_index(checkpoint, cursor_id) else {
        return Ok(None);
    };
    let visit_count = {
        let visits = checkpoint.active_cursors[index]
            .visit_counters
            .entry(node.id.clone())
            .or_default();
        *visits += 1;
        *visits
    };
    if visit_count > checkpoint.max_visits_per_node {
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_error")
                .with("cursorId", cursor_id.to_string())
                .with(
                    "message",
                    format!(
                        "Node \"{}\" visited {} times — aborting to prevent infinite loop.",
                        node.name, checkpoint.max_visits_per_node
                    ),
                ),
        )
        .await?;
        checkpoint.status = RuntimeStatus::Aborted;
        checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
        checkpoint.active_cursors.clear();
        update_checkpoint_summary(workflow, checkpoint);
        return Ok(None);
    }

    let iteration = {
        let loop_counter = checkpoint.active_cursors[index]
            .loop_counters
            .entry(node.id.clone())
            .or_default();
        *loop_counter += 1;
        *loop_counter
    };
    checkpoint.last_branch_origin_id =
        checkpoint.active_cursors[index].last_branch_origin_id.clone();
    checkpoint.last_branch_choice = checkpoint.active_cursors[index].last_branch_choice.clone();
    Ok(Some(iteration))
}

async fn handle_skipped_task(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
) -> anyhow::Result<()> {
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("node_skipped")
            .with("cursorId", cursor_id.clone())
            .with("nodeId", node.id.clone())
            .with("nodeName", node.name.clone())
            .with("reason", "Skip condition matched"),
    )
    .await?;

    let next_edge = select_success_edge(graph, &node.id);
    let next_node_id = next_edge.as_ref().map(|edge| edge.to.clone());
    record_transition_for_cursor(
        ctx,
        checkpoint,
        Some(cursor_id.clone()),
        node.id.clone(),
        next_node_id.clone(),
        "skip",
        "skip condition",
    )
    .await?;

    if let Some(index) = find_cursor_index(checkpoint, &cursor_id) {
        if let Some(edge) = next_edge {
            checkpoint.active_cursors[index].node_id = edge.to.clone();
            checkpoint.active_cursors[index].incoming_edge_id = Some(edge.id.clone());
            checkpoint.active_cursors[index].incoming_node_id = Some(node.id);
            checkpoint.active_cursors[index].state = CursorRuntimeState::Runnable;
        } else {
            checkpoint.active_cursors.remove(index);
        }
    }
    update_checkpoint_summary(workflow, checkpoint);
    Ok(())
}

async fn queue_approval(
    ctx: &RuntimeContext,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
) -> anyhow::Result<()> {
    let Some(index) = find_cursor_index(checkpoint, &cursor_id) else {
        return Ok(());
    };
    let last_output = checkpoint.active_cursors[index].last_output.clone();
    checkpoint.active_cursors[index].state = CursorRuntimeState::WaitingApproval;
    checkpoint.queued_approvals.push(QueuedApproval {
        approval: PendingApproval {
            cursor_id: cursor_id.clone(),
            node_id: node.id.clone(),
            node_name: node.name.clone(),
            prompt: node.prompt.clone(),
            last_output: last_output.clone(),
        },
    });
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("approval_queued")
            .with("cursorId", cursor_id)
            .with("nodeId", node.id)
            .with("nodeName", node.name)
            .with("lastOutput", last_output),
    )
    .await?;
    Ok(())
}

async fn activate_next_approval(
    ctx: &RuntimeContext,
    checkpoint: &mut RuntimeCheckpoint,
    active_approval: &mut Option<ActiveApprovalWait>,
) -> anyhow::Result<bool> {
    if checkpoint.pending_approval.is_some() || checkpoint.queued_approvals.is_empty() {
        return Ok(false);
    }
    let queued = checkpoint.queued_approvals.remove(0);
    let (sender, receiver) = oneshot::channel();
    ctx.registry
        .set_pending_approval(&checkpoint.run_id, sender)
        .await?;
    checkpoint.pending_approval = Some(queued.approval.clone());
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("approval_required")
            .with("runId", checkpoint.run_id.clone())
            .with("cursorId", queued.approval.cursor_id.clone())
            .with("nodeId", queued.approval.node_id.clone())
            .with("nodeName", queued.approval.node_name.clone())
            .with("prompt", queued.approval.prompt.clone())
            .with("lastOutput", queued.approval.last_output.clone()),
    )
    .await?;
    *active_approval = Some(ActiveApprovalWait {
        cursor_id: queued.approval.cursor_id.clone(),
        receiver,
    });
    Ok(true)
}

async fn restore_pending_approval(
    ctx: &RuntimeContext,
    checkpoint: &mut RuntimeCheckpoint,
    active_approval: &mut Option<ActiveApprovalWait>,
) -> anyhow::Result<()> {
    let Some(pending) = checkpoint.pending_approval.clone() else {
        return Ok(());
    };
    let Some(cursor_id) = checkpoint
        .active_cursors
        .iter()
        .find(|cursor| {
            cursor.state == CursorRuntimeState::WaitingApproval
                && (pending.cursor_id.is_empty() || cursor.cursor_id == pending.cursor_id)
        })
        .or_else(|| {
            checkpoint
                .active_cursors
                .iter()
                .find(|cursor| cursor.state == CursorRuntimeState::WaitingApproval)
        })
        .map(|cursor| cursor.cursor_id.clone())
    else {
        checkpoint.pending_approval = None;
        return Ok(());
    };
    let (sender, receiver) = oneshot::channel();
    ctx.registry
        .set_pending_approval(&checkpoint.run_id, sender)
        .await?;
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("approval_required")
            .with("runId", checkpoint.run_id.clone())
            .with("cursorId", cursor_id.clone())
            .with("nodeId", pending.node_id.clone())
            .with("nodeName", pending.node_name.clone())
            .with("prompt", pending.prompt.clone())
            .with("lastOutput", pending.last_output.clone()),
    )
    .await?;
    *active_approval = Some(ActiveApprovalWait { cursor_id, receiver });
    Ok(())
}

async fn wait_for_approval(
    ctx: &RuntimeContext,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    active_approval: &mut Option<ActiveApprovalWait>,
) -> anyhow::Result<()> {
    if let Some(wait) = active_approval.as_mut() {
        tokio::select! {
            decision = &mut wait.receiver => {
                let cursor_id = wait.cursor_id.clone();
                *active_approval = None;
                handle_approval_resolution(ctx, graph, checkpoint, cursor_id, decision.ok()).await?;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
    Ok(())
}

async fn run_cursor_task(
    ctx: RuntimeContext,
    run_ctx: CursorTaskExecutionContext,
    cursor: CursorState,
    node: WorkflowNode,
    iteration: u32,
) -> anyhow::Result<CursorTaskResult> {
    let mut resolved_prompt = resolve_template_vars(
        &node.prompt,
        &TemplateRuntimeContext {
            current_node_id: &node.id,
            current_node: &node,
            all_results: &run_ctx.all_results,
            last_output: &cursor.last_output,
            var_map: &run_ctx.var_map,
            inbound_map: &run_ctx.shared.inbound_map,
            last_branch_origin_id: cursor.last_branch_origin_id.as_deref(),
            last_branch_choice: cursor.last_branch_choice.as_deref(),
        },
    );

    let stale_predecessors = run_ctx
        .shared
        .inbound_map
        .get(&node.id)
        .into_iter()
        .flat_map(|sources| sources.iter())
        .filter_map(|source_id| run_ctx.all_results.get(source_id))
        .filter(|result| result.stale)
        .count();
    if stale_predecessors > 0 {
        emit_event(
            &ctx,
            &run_ctx.run_id,
            RuntimeEvent::new("sys_warn").with(
                "message",
                format!(
                    "Node \"{}\" uses preserved outputs from a previous run.",
                    node.name
                ),
            ),
        )
        .await?;
    }

    let mut refined_prompt = None;
    if run_ctx.use_orchestrator && !cursor.last_output.is_empty() {
        emit_event(
            &ctx,
            &run_ctx.run_id,
            RuntimeEvent::new("orchestrator_start")
                .with("cursorId", cursor.cursor_id.clone())
                .with("nodeId", node.id.clone())
                .with("nodeName", node.name.clone()),
        )
        .await?;
        let orchestrator = run_orchestrator_refinement(
            ctx.session_manager.clone(),
            &run_ctx.workflow_goal,
            &node,
            &resolved_prompt,
            &cursor.last_output,
            run_ctx.total_executed.saturating_sub(1),
            run_ctx.workflow_nodes_len,
            &run_ctx.cwd,
        )
        .await?;
        if orchestrator.success && !orchestrator.output.is_empty() {
            refined_prompt = Some(orchestrator.output.clone());
            emit_event(
                &ctx,
                &run_ctx.run_id,
                RuntimeEvent::new("orchestrator_done")
                    .with("cursorId", cursor.cursor_id.clone())
                    .with("nodeId", node.id.clone())
                    .with("originalPrompt", resolved_prompt.clone())
                    .with("refinedPrompt", orchestrator.output.clone())
                    .with("duration", orchestrator.duration.clone()),
            )
            .await?;
            resolved_prompt = orchestrator.output.clone();
        } else {
            emit_event(
                &ctx,
                &run_ctx.run_id,
                RuntimeEvent::new("orchestrator_warn")
                    .with("cursorId", cursor.cursor_id.clone())
                    .with("nodeId", node.id.clone())
                    .with(
                        "message",
                        "Orchestrator refinement failed — using original prompt",
                    )
                    .with("error", orchestrator.stderr),
            )
            .await?;
        }
    }

    let agent_name = node.agent.clone().unwrap_or_else(|| DEFAULT_AGENT.to_string());

    // Session reuse: resolve resume_session_id from referenced node's result
    let resume_session_id = node
        .continue_session_from
        .as_ref()
        .and_then(|source_id| {
            run_ctx
                .all_results
                .get(source_id)
                .and_then(|r| r.metadata.agent_session_id.clone())
        });
    let needs_session_persistence = run_ctx.shared.session_persistence_nodes.contains(&node.id);

    // Artifact-based JSON output: no native schema passing, all agents use artifact files
    let json_schema = None;
    let agent_config = crate::model::resolve_agent_config(
        &run_ctx.shared.agent_defaults,
        &run_ctx.cwd,
        &agent_name,
        &node,
        resume_session_id,
        needs_session_persistence,
        json_schema,
    );
    // For JSON response nodes, wrap prompt with artifact instructions
    resolved_prompt = wrap_prompt_for_json(&node, resolved_prompt, &None);
    emit_event(
        &ctx,
        &run_ctx.run_id,
        RuntimeEvent::new("node_start")
            .with("cursorId", cursor.cursor_id.clone())
            .with("nodeId", node.id.clone())
            .with("nodeName", node.name.clone())
            .with("agent", agent_name.clone())
            .with("resolvedPrompt", resolved_prompt.clone())
            .with("iteration", iteration),
    )
    .await?;

    let mut attempts = 0;
    let max_attempts = node.retry_count.unwrap_or(0) + 1;
    let mut result = loop {
        attempts += 1;
        let step_result = ctx
            .runner
            .run_with_interaction(
                agent_name.clone(),
                resolved_prompt.clone(),
                run_ctx.cwd.clone(),
                node.timeout,
                Some(agent_config.clone()),
                ctx.clone(),
                run_ctx.run_id.clone(),
            )
            .await?;
        if step_result.success || attempts >= max_attempts {
            break step_result;
        }
        emit_event(
            &ctx,
            &run_ctx.run_id,
            RuntimeEvent::new("node_retry")
                .with("cursorId", cursor.cursor_id.clone())
                .with("nodeId", node.id.clone())
                .with("nodeName", node.name.clone())
                .with("attempt", attempts)
                .with("maxAttempts", max_attempts)
                .with("delay", node.retry_delay.unwrap_or(2)),
        )
        .await?;
        tokio::time::sleep(Duration::from_secs(node.retry_delay.unwrap_or(2))).await;
    };

    parse_structured_output(&node, &mut result);
    result.resolved_prompt = Some(resolved_prompt.clone());

    Ok(CursorTaskResult {
        cursor_id: cursor.cursor_id,
        node,
        result,
        resolved_prompt,
        refined_prompt,
        iteration,
        attempts,
    })
}

async fn apply_join_result(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    task: Result<anyhow::Result<CursorTaskResult>, tokio::task::JoinError>,
) -> anyhow::Result<()> {
    let task_result = match task {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            emit_event(
                ctx,
                &checkpoint.run_id,
                RuntimeEvent::new("workflow_error").with("message", error.to_string()),
            )
            .await?;
            checkpoint.status = RuntimeStatus::Failed;
            checkpoint.execution_log.terminal_reason = Some("failed".to_string());
            return Ok(());
        }
        Err(error) if error.is_cancelled() => {
            return Ok(());
        }
        Err(error) => {
            emit_event(
                ctx,
                &checkpoint.run_id,
                RuntimeEvent::new("workflow_error").with("message", error.to_string()),
            )
            .await?;
            checkpoint.status = RuntimeStatus::Failed;
            checkpoint.execution_log.terminal_reason = Some("failed".to_string());
            return Ok(());
        }
    };

    let Some(index) = find_cursor_index(checkpoint, &task_result.cursor_id) else {
        return Ok(());
    };
    checkpoint.active_cursors[index].state = CursorRuntimeState::Runnable;
    checkpoint.active_cursors[index].last_output = task_result.result.output.clone();
    checkpoint.last_output = task_result.result.output.clone();
    checkpoint
        .all_results
        .insert(task_result.node.id.clone(), task_result.result.clone());

    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("node_done")
            .with("cursorId", task_result.cursor_id.clone())
            .with("nodeId", task_result.node.id.clone())
            .with("result", {
                let mut event_result = json!({
                    "success": task_result.result.success,
                    "output": task_result.result.output,
                    "stderr": task_result.result.stderr,
                    "exitCode": task_result.result.exit_code,
                    "duration": task_result.result.duration,
                    "nodeName": task_result.node.name,
                    "resolvedPrompt": task_result.resolved_prompt,
                    "parsedOutput": task_result.result.parsed_output,
                    "parseError": task_result.result.parse_error,
                });
                // Merge agent metadata fields into the event result
                if let Ok(meta_val) = serde_json::to_value(&task_result.result.metadata) {
                    if let (Some(base), Some(meta)) = (event_result.as_object_mut(), meta_val.as_object()) {
                        base.extend(meta.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                }
                event_result
            }),
    )
    .await?;

    checkpoint.execution_log.node_executions.push(NodeExecutionLog {
        cursor_id: Some(task_result.cursor_id.clone()),
        node_id: task_result.node.id.clone(),
        node_name: task_result.node.name.clone(),
        node_type: "task".to_string(),
        agent: task_result
            .node
            .agent
            .clone()
            .unwrap_or_else(|| DEFAULT_AGENT.to_string()),
        original_prompt: task_result.node.prompt.clone(),
        resolved_prompt: task_result.resolved_prompt.clone(),
        refined_prompt: task_result.refined_prompt.clone(),
        output: task_result.result.output.clone(),
        stderr: task_result.result.stderr.clone(),
        exit_code: task_result.result.exit_code,
        success: task_result.result.success,
        duration: task_result.result.duration.clone(),
        iteration: task_result.iteration,
        attempts: task_result.attempts,
        timestamp: now_iso(),
        metadata: task_result.result.metadata.clone(),
    });

    if !task_result.result.success {
        let status = if task_result.result.exit_code == -2 {
            CursorTerminalStatus::Timeout
        } else {
            CursorTerminalStatus::Failure
        };
        handle_terminal_cursor_status(
            ctx,
            graph,
            checkpoint,
            task_result.cursor_id,
            task_result.node,
            task_result.result,
            status,
        )
        .await?;
        return Ok(());
    }

    let output_hashes = checkpoint
        .output_hashes
        .entry(task_result.node.id.clone())
        .or_default();
    output_hashes.push(djb2(&task_result.result.output));
    if output_hashes.len() >= 3
        && output_hashes[output_hashes.len() - 1] == output_hashes[output_hashes.len() - 2]
        && output_hashes[output_hashes.len() - 2] == output_hashes[output_hashes.len() - 3]
    {
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_error")
                .with("cursorId", task_result.cursor_id.clone())
                .with("nodeId", task_result.node.id.clone())
                .with(
                    "message",
                    format!(
                        "Node \"{}\" produced identical output 3 times consecutively — aborting (stagnation detected).",
                        task_result.node.name
                    ),
                ),
        )
        .await?;
        checkpoint.status = RuntimeStatus::Aborted;
        checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
        checkpoint.active_cursors.clear();
        return Ok(());
    }

    let decision = select_next_decision(
        ctx,
        workflow,
        graph,
        checkpoint,
        &task_result.cursor_id,
        &task_result.node,
        &task_result.result,
        task_result.iteration,
    )
    .await?;
    let next_node_id = decision.next_edge.as_ref().map(|edge| edge.to.clone());
    record_transition_for_cursor(
        ctx,
        checkpoint,
        Some(task_result.cursor_id.clone()),
        task_result.node.id.clone(),
        next_node_id.clone(),
        &decision.control_type,
        &decision.reason,
    )
    .await?;

    if let Some(index) = find_cursor_index(checkpoint, &task_result.cursor_id) {
        if let Some(edge) = decision.next_edge {
            let cursor = &mut checkpoint.active_cursors[index];
            let node_id = task_result.node.id.clone();
            cursor.node_id = edge.to.clone();
            cursor.incoming_edge_id = Some(edge.id.clone());
            cursor.incoming_node_id = Some(node_id.clone());
            if decision.control_type == "branch" {
                cursor.last_branch_origin_id = Some(node_id);
                cursor.last_branch_choice = edge.label.clone();
            }
            cursor.state = CursorRuntimeState::Runnable;
        } else {
            checkpoint.active_cursors.remove(index);
        }
    }

    Ok(())
}

async fn handle_approval_resolution(
    ctx: &RuntimeContext,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    decision: Option<ApprovalDecision>,
) -> anyhow::Result<()> {
    checkpoint.pending_approval = None;
    let Some(index) = find_cursor_index(checkpoint, &cursor_id) else {
        return Ok(());
    };
    let node_id = checkpoint.active_cursors[index].node_id.clone();
    let Some(node) = graph.node_map.get(node_id.as_str()).map(|n| (*n).clone()) else {
        return Ok(());
    };
    let approved = decision.as_ref().map(|value| value.approved).unwrap_or(false);
    let user_input = decision
        .map(|value| value.user_input)
        .unwrap_or_default();
    let output = if approved {
        if user_input.trim().is_empty() {
            "[Approved by user]".to_string()
        } else {
            user_input
        }
    } else {
        String::new()
    };
    let result = NodeResult {
        success: approved,
        output: output.clone(),
        stderr: if approved {
            String::new()
        } else {
            "Rejected by user".to_string()
        },
        exit_code: if approved { 0 } else { 1 },
        duration: "0".to_string(),
        agent: "user".to_string(),
        prompt: node.prompt.clone(),
        raw_output: None,
        parsed_output: None,
        parse_error: None,
        resolved_prompt: Some(node.prompt.clone()),
        stale: false,
        preserved_from_run_id: None,
        ..Default::default()
    };
    checkpoint.all_results.insert(node.id.clone(), result.clone());
    checkpoint.execution_log.node_executions.push(NodeExecutionLog {
        cursor_id: Some(cursor_id.clone()),
        node_id: node.id.clone(),
        node_name: node.name.clone(),
        node_type: "approval".to_string(),
        agent: "user".to_string(),
        original_prompt: node.prompt.clone(),
        resolved_prompt: node.prompt.clone(),
        refined_prompt: None,
        output: output.clone(),
        stderr: result.stderr.clone(),
        exit_code: result.exit_code,
        success: result.success,
        duration: "0".to_string(),
        iteration: *checkpoint.active_cursors[index]
            .loop_counters
            .get(&node.id)
            .unwrap_or(&1),
        attempts: 1,
        timestamp: now_iso(),
        ..Default::default()
    });
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("node_done")
            .with("cursorId", cursor_id.clone())
            .with("nodeId", node.id.clone())
            .with(
                "result",
                json!({
                    "success": result.success,
                    "output": result.output,
                    "stderr": result.stderr,
                    "exitCode": result.exit_code,
                    "duration": result.duration,
                    "nodeName": node.name,
                    "resolvedPrompt": node.prompt,
                }),
            ),
    )
    .await?;

    if approved {
        let next_edge = select_success_edge(graph, &node.id);
        let next_node_id = next_edge.as_ref().map(|edge| edge.to.clone());
        record_transition_for_cursor(
            ctx,
            checkpoint,
            Some(cursor_id.clone()),
            node.id.clone(),
            next_node_id.clone(),
            "success",
            "approved",
        )
        .await?;
        if let Some(edge) = next_edge {
            checkpoint.active_cursors[index].node_id = edge.to.clone();
            checkpoint.active_cursors[index].incoming_edge_id = Some(edge.id.clone());
            checkpoint.active_cursors[index].incoming_node_id = Some(node.id);
            checkpoint.active_cursors[index].last_output = output;
            checkpoint.active_cursors[index].state = CursorRuntimeState::Runnable;
        } else {
            checkpoint.active_cursors.remove(index);
        }
        return Ok(());
    }

    let reject_edge = graph
        .outgoing_for(&node.id)
        .iter()
        .find(|edge| edge.outcome == WorkflowEdgeOutcome::Reject)
        .map(|edge| (*edge).clone());
    if let Some(edge) = reject_edge {
        record_transition_for_cursor(
            ctx,
            checkpoint,
            Some(cursor_id.clone()),
            node.id.clone(),
            Some(edge.to.clone()),
            "reject",
            "rejected",
        )
        .await?;
        checkpoint.active_cursors[index].node_id = edge.to.clone();
        checkpoint.active_cursors[index].incoming_edge_id = Some(edge.id.clone());
        checkpoint.active_cursors[index].incoming_node_id = Some(node.id);
        checkpoint.active_cursors[index].state = CursorRuntimeState::Runnable;
    } else {
        handle_terminal_cursor_status(
            ctx,
            graph,
            checkpoint,
            cursor_id,
            node,
            result,
            CursorTerminalStatus::Failure,
        )
        .await?;
    }
    Ok(())
}

async fn handle_split_node(
    ctx: &RuntimeContext,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
) -> anyhow::Result<()> {
    let Some(index) = find_cursor_index(checkpoint, &cursor_id) else {
        return Ok(());
    };
    let cursor = checkpoint.active_cursors.remove(index);
    let edges = graph
        .outgoing_for(&node.id)
        .iter()
        .filter(|edge| edge.outcome == WorkflowEdgeOutcome::Success)
        .map(|edge| (*edge).clone())
        .collect::<Vec<_>>();
    let family_id = new_split_family_id();
    let child_cursor_ids = edges.iter().map(|_| new_cursor_id()).collect::<Vec<_>>();
    checkpoint.split_families.insert(
        family_id.clone(),
        SplitFamilyState {
            family_id: family_id.clone(),
            split_node_id: node.id.clone(),
            execution_epoch: checkpoint.execution_epoch,
            failure_policy: node.split_failure_policy.clone(),
            member_cursor_ids: child_cursor_ids.clone(),
            force_failed: false,
        },
    );
    checkpoint.all_results.insert(
        node.id.clone(),
        NodeResult {
            success: true,
            output: format!("Spawned {} branches.", edges.len()),
            stderr: String::new(),
            exit_code: 0,
            duration: "0".to_string(),
            agent: "system".to_string(),
            prompt: node.prompt.clone(),
            raw_output: None,
            parsed_output: Some(json!({
                "branchCount": edges.len(),
                "failurePolicy": format!("{:?}", node.split_failure_policy).to_lowercase(),
            })),
            parse_error: None,
            resolved_prompt: Some(node.prompt.clone()),
            stale: false,
            preserved_from_run_id: None,
            ..Default::default()
        },
    );
    checkpoint.execution_log.node_executions.push(NodeExecutionLog {
        cursor_id: Some(cursor_id.clone()),
        node_id: node.id.clone(),
        node_name: node.name.clone(),
        node_type: "split".to_string(),
        agent: "system".to_string(),
        original_prompt: node.prompt.clone(),
        resolved_prompt: node.prompt.clone(),
        refined_prompt: None,
        output: format!("Spawned {} branches.", edges.len()),
        stderr: String::new(),
        exit_code: 0,
        success: true,
        duration: "0".to_string(),
        iteration: *cursor.loop_counters.get(&node.id).unwrap_or(&1),
        attempts: 1,
        timestamp: now_iso(),
        ..Default::default()
    });
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("node_done")
            .with("cursorId", cursor_id.clone())
            .with("nodeId", node.id.clone())
            .with(
                "result",
                json!({
                    "success": true,
                    "output": format!("Spawned {} branches.", edges.len()),
                    "stderr": "",
                    "exitCode": 0,
                    "duration": "0",
                    "nodeName": node.name,
                    "resolvedPrompt": node.prompt,
                }),
            ),
    )
    .await?;
    for (branch_index, edge) in edges.iter().enumerate() {
        let child_cursor_id = child_cursor_ids[branch_index].clone();
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("cursor_spawned")
                .with("cursorId", child_cursor_id.clone())
                .with("parentCursorId", cursor_id.clone())
                .with("familyId", family_id.clone())
                .with("fromNodeId", node.id.clone())
                .with("toNodeId", edge.to.clone()),
        )
        .await?;
        record_transition_for_cursor(
            ctx,
            checkpoint,
            Some(cursor_id.clone()),
            node.id.clone(),
            Some(edge.to.clone()),
            "split",
            edge.label.clone().unwrap_or_else(|| edge.id.clone()).as_str(),
        )
        .await?;
        let mut split_family_ids = cursor.split_family_ids.clone();
        split_family_ids.push(family_id.clone());
        checkpoint.active_cursors.push(CursorState {
            cursor_id: child_cursor_id,
            node_id: edge.to.clone(),
            execution_epoch: checkpoint.execution_epoch,
            parent_cursor_id: Some(cursor.cursor_id.clone()),
            incoming_edge_id: Some(edge.id.clone()),
            incoming_node_id: Some(node.id.clone()),
            split_family_ids,
            last_output: cursor.last_output.clone(),
            loop_counters: cursor.loop_counters.clone(),
            visit_counters: cursor.visit_counters.clone(),
            last_branch_origin_id: cursor.last_branch_origin_id.clone(),
            last_branch_choice: cursor.last_branch_choice.clone(),
            cancel_requested: false,
            state: CursorRuntimeState::Runnable,
        });
    }
    Ok(())
}

async fn handle_collector_entry(
    ctx: &RuntimeContext,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
) -> anyhow::Result<()> {
    let Some(index) = find_cursor_index(checkpoint, &cursor_id) else {
        return Ok(());
    };
    let incoming_edge_id = checkpoint.active_cursors[index].incoming_edge_id.clone();
    let incoming_edge = incoming_edge_id
        .as_deref()
        .and_then(|edge_id| {
            graph
                .inbound_for(&node.id)
                .iter()
                .find(|edge| edge.id == edge_id)
                .map(|edge| (*edge).clone())
        })
        .or_else(|| {
            graph
                .inbound_for(&node.id)
                .first()
                .map(|edge| (*edge).clone())
        });
    let Some(incoming_edge) = incoming_edge else {
        checkpoint.status = RuntimeStatus::Failed;
        checkpoint.execution_log.terminal_reason = Some("failed".to_string());
        return Ok(());
    };

    checkpoint.active_cursors[index].state = CursorRuntimeState::WaitingCollector;
    let barrier_key = format!("{}:{}", node.id, checkpoint.execution_epoch);
    let barrier = checkpoint
        .collector_barriers
        .entry(barrier_key)
        .or_insert_with(|| CollectorBarrierState {
            execution_epoch: checkpoint.execution_epoch,
            required_inputs: graph
                .inbound_for(&node.id)
                .iter()
                .map(|edge| merge_key_for_edge(edge))
                .collect(),
            arrivals: BTreeMap::new(),
            waiting_cursor_ids: Vec::new(),
            released: false,
        });
    let merge_key = merge_key_for_edge(&incoming_edge);
    let source_result = checkpoint
        .all_results
        .get(&incoming_edge.from)
        .cloned()
        .unwrap_or(NodeResult {
            success: true,
            output: checkpoint.active_cursors[index].last_output.clone(),
            stderr: String::new(),
            exit_code: 0,
            duration: "0".to_string(),
            agent: "system".to_string(),
            prompt: String::new(),
            raw_output: None,
            parsed_output: None,
            parse_error: None,
            resolved_prompt: None,
            stale: false,
            preserved_from_run_id: None,
            ..Default::default()
        });
    barrier.arrivals.insert(
        merge_key.clone(),
        CollectorInputStatus {
            source_node_id: incoming_edge.from.clone(),
            edge_id: incoming_edge.id.clone(),
            edge_label: incoming_edge.label.clone(),
            split_family_ids: checkpoint.active_cursors[index].split_family_ids.clone(),
            status: CursorTerminalStatus::Success,
            success: true,
            output: source_result.output,
            stderr: source_result.stderr,
            exit_code: source_result.exit_code,
            parsed_output: source_result.parsed_output,
        },
    );
    if !barrier.waiting_cursor_ids.contains(&cursor_id) {
        barrier.waiting_cursor_ids.push(cursor_id.clone());
    }
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("collector_waiting")
            .with("cursorId", cursor_id)
            .with("nodeId", node.id.clone())
            .with("nodeName", node.name.clone())
            .with("arrived", barrier.arrivals.len())
            .with("required", barrier.required_inputs.len()),
    )
    .await?;
    release_collectors_if_ready(ctx, graph, checkpoint).await
}

async fn release_collectors_if_ready(
    ctx: &RuntimeContext,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
) -> anyhow::Result<()> {
    let ready_barriers = checkpoint
        .collector_barriers
        .iter()
        .filter(|(_, barrier)| {
            !barrier.released
                && barrier
                    .required_inputs
                    .iter()
                    .all(|key| barrier.arrivals.contains_key(key))
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();

    for barrier_key in ready_barriers {
        let Some(barrier) = checkpoint.collector_barriers.get_mut(&barrier_key) else {
            continue;
        };
        barrier.released = true;
        let waiting_cursor_ids = barrier.waiting_cursor_ids.clone();
        let arrivals = barrier.arrivals.clone();
        let required_len = barrier.required_inputs.len();
        let collector_id = barrier_key
            .split(':')
            .next()
            .unwrap_or_default()
            .to_string();
        let Some(node) = graph.node_map.get(collector_id.as_str()).map(|n| (*n).clone()) else {
            continue;
        };
        let parsed_output = json!({
            "inputs": arrivals.clone(),
            "summary": {
                "total": required_len,
                "succeeded": arrivals.values().filter(|item| item.status == CursorTerminalStatus::Success).count(),
                "failed": arrivals.values().filter(|item| item.status == CursorTerminalStatus::Failure).count(),
                "timedOut": arrivals.values().filter(|item| item.status == CursorTerminalStatus::Timeout).count(),
                "cancelled": arrivals.values().filter(|item| item.status == CursorTerminalStatus::Cancelled).count(),
            }
        });
        let output = serde_json::to_string_pretty(&parsed_output)
            .unwrap_or_else(|_| parsed_output.to_string());
        checkpoint.all_results.insert(
            collector_id.clone(),
            NodeResult {
                success: true,
                output: output.clone(),
                duration: "0".to_string(),
                agent: "system".to_string(),
                prompt: node.prompt.clone(),
                parsed_output: Some(parsed_output.clone()),
                resolved_prompt: Some(node.prompt.clone()),
                ..Default::default()
            },
        );
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("aggregate_merged")
                .with("nodeId", collector_id.clone())
                .with("inputs", parsed_output["inputs"].clone()),
        )
        .await?;
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("collector_released")
                .with("nodeId", collector_id.clone())
                .with("nodeName", node.name.clone()),
        )
        .await?;

        let representative_cursor_id = waiting_cursor_ids
            .first()
            .cloned()
            .unwrap_or_else(new_cursor_id);
        for waiting_cursor_id in &waiting_cursor_ids {
            if let Some(index) = find_cursor_index(checkpoint, waiting_cursor_id) {
                checkpoint.active_cursors.remove(index);
            }
        }

        checkpoint.execution_log.node_executions.push(NodeExecutionLog {
            cursor_id: Some(representative_cursor_id.clone()),
            node_id: collector_id.clone(),
            node_name: node.name.clone(),
            node_type: "collector".to_string(),
            agent: "system".to_string(),
            original_prompt: node.prompt.clone(),
            resolved_prompt: node.prompt.clone(),
            refined_prompt: None,
            output: output.clone(),
            stderr: String::new(),
            exit_code: 0,
            success: true,
            duration: "0".to_string(),
            iteration: 1,
            attempts: 1,
            timestamp: now_iso(),
            ..Default::default()
        });

        let next_edge = select_success_edge(graph, &collector_id);
        let next_node_id = next_edge.as_ref().map(|edge| edge.to.clone());
        record_transition_for_cursor(
            ctx,
            checkpoint,
            Some(representative_cursor_id.clone()),
            collector_id.clone(),
            next_node_id.clone(),
            "success",
            "",
        )
        .await?;
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("node_done")
                .with("cursorId", representative_cursor_id.clone())
                .with("nodeId", collector_id.clone())
                .with(
                    "result",
                    json!({
                        "success": true,
                        "output": output,
                        "stderr": "",
                        "exitCode": 0,
                        "duration": "0",
                        "nodeName": node.name,
                        "resolvedPrompt": node.prompt,
                        "parsedOutput": parsed_output,
                    }),
                ),
        )
        .await?;

        if let Some(edge) = next_edge {
            checkpoint.active_cursors.push(CursorState {
                cursor_id: representative_cursor_id,
                node_id: edge.to.clone(),
                execution_epoch: checkpoint.execution_epoch,
                parent_cursor_id: None,
                incoming_edge_id: Some(edge.id.clone()),
                incoming_node_id: Some(collector_id),
                split_family_ids: Vec::new(),
                last_output: output,
                loop_counters: BTreeMap::new(),
                visit_counters: BTreeMap::new(),
                last_branch_origin_id: None,
                last_branch_choice: None,
                cancel_requested: false,
                state: CursorRuntimeState::Runnable,
            });
        }
    }
    Ok(())
}

async fn handle_terminal_cursor_status(
    ctx: &RuntimeContext,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
    result: NodeResult,
    status: CursorTerminalStatus,
) -> anyhow::Result<()> {
    if let Some(index) = find_cursor_index(checkpoint, &cursor_id) {
        let cursor = checkpoint.active_cursors[index].clone();
        for target in nearest_collectors_for_node(graph, &node.id) {
            let barrier_key = format!("{}:{}", target.collector_id, checkpoint.execution_epoch);
            let barrier = checkpoint
                .collector_barriers
                .entry(barrier_key)
                .or_insert_with(|| CollectorBarrierState {
                    execution_epoch: checkpoint.execution_epoch,
                    required_inputs: graph
                        .inbound_for(&target.collector_id)
                        .iter()
                        .map(|edge| merge_key_for_edge(edge))
                        .collect(),
                    arrivals: BTreeMap::new(),
                    waiting_cursor_ids: Vec::new(),
                    released: false,
                });
            barrier
                .arrivals
                .entry(merge_key_for_edge(&target.inbound_edge))
                .or_insert_with(|| CollectorInputStatus {
                    source_node_id: target.inbound_edge.from.clone(),
                    edge_id: target.inbound_edge.id.clone(),
                    edge_label: target.inbound_edge.label.clone(),
                    split_family_ids: cursor.split_family_ids.clone(),
                    status: status.clone(),
                    success: false,
                    output: result.output.clone(),
                    stderr: result.stderr.clone(),
                    exit_code: result.exit_code,
                    parsed_output: result.parsed_output.clone(),
                });
        }

        let mut fail_run = cursor.split_family_ids.is_empty();
        for family_id in &cursor.split_family_ids {
            if let Some(family) = checkpoint.split_families.get_mut(family_id) {
                match family.failure_policy {
                    SplitFailurePolicy::BestEffortContinue => {}
                    SplitFailurePolicy::DrainThenFail => family.force_failed = true,
                    SplitFailurePolicy::FailFastCancel => {
                        family.force_failed = true;
                        fail_run = true;
                    }
                }
            }
        }
        if fail_run {
            checkpoint.status = RuntimeStatus::Failed;
            checkpoint.execution_log.terminal_reason = Some("failed".to_string());
            checkpoint
                .active_cursors
                .iter_mut()
                .filter(|active| active.cursor_id != cursor_id)
                .for_each(|active| active.cancel_requested = true);
        }
        checkpoint.active_cursors.remove(index);
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("cursor_cancelled")
                .with("cursorId", cursor_id.clone())
                .with("nodeId", node.id.clone())
                .with("status", status),
        )
        .await?;
        release_collectors_if_ready(ctx, graph, checkpoint).await?;
    }
    Ok(())
}

async fn record_transition_for_cursor(
    ctx: &RuntimeContext,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: Option<String>,
    from: String,
    to: Option<String>,
    control_type: &str,
    reason: &str,
) -> anyhow::Result<()> {
    checkpoint.execution_log.transitions.push(TransitionLog {
        cursor_id: cursor_id.clone(),
        from_node_id: from.clone(),
        to_node_id: to.clone(),
        control_type: control_type.to_string(),
        reason: reason.to_string(),
        timestamp: now_iso(),
    });
    let mut event = RuntimeEvent::new("transition")
        .with("fromNodeId", from)
        .with("toNodeId", to)
        .with("controlType", control_type)
        .with("reason", reason);
    if let Some(cursor_id) = cursor_id {
        event = event.with("cursorId", cursor_id);
    }
    emit_event(ctx, &checkpoint.run_id, event).await
}

async fn select_next_decision(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: &str,
    node: &WorkflowNode,
    result: &NodeResult,
    iteration: u32,
) -> anyhow::Result<NextDecision> {
    let default_success = select_success_edge(graph, &node.id);
    let edges = graph.outgoing_for(&node.id);
    let branch_edges = edges
        .iter()
        .filter(|edge| edge.outcome == WorkflowEdgeOutcome::Branch)
        .copied()
        .collect::<Vec<_>>();
    let loop_continue = edges
        .iter()
        .find(|edge| edge.outcome == WorkflowEdgeOutcome::LoopContinue)
        .map(|edge| (*edge).clone());
    let loop_exit = edges
        .iter()
        .find(|edge| edge.outcome == WorkflowEdgeOutcome::LoopExit)
        .map(|edge| (*edge).clone());

    if let Some(loop_continue) = loop_continue {
        let max_iterations = node.loop_max_iterations.unwrap_or(5);
        if iteration >= max_iterations {
            emit_event(
                ctx,
                &checkpoint.run_id,
                RuntimeEvent::new("loop_max_reached")
                    .with("cursorId", cursor_id.to_string())
                    .with("nodeId", node.id.clone())
                    .with("nodeName", node.name.clone())
                    .with("maxIterations", max_iterations),
            )
            .await?;
            return Ok(NextDecision {
                next_edge: loop_exit,
                control_type: "loop_exit".to_string(),
                reason: format!("max iterations ({}) reached", max_iterations),
            });
        }

        if let (Some(condition), Some(parsed)) = (&node.loop_condition, &result.parsed_output) {
            let (matched, error) = evaluate_condition(parsed, condition);
            let verdict = if matched { "CONTINUE" } else { "EXIT" };
            emit_event(
                ctx,
                &checkpoint.run_id,
                RuntimeEvent::new("loop_decision")
                    .with("cursorId", cursor_id.to_string())
                    .with("nodeId", node.id.clone())
                    .with("verdict", verdict)
                    .with("iteration", iteration)
                    .with("deterministic", true),
            )
            .await?;
            checkpoint.execution_log.decisions.push(DecisionLog {
                kind: "loop".to_string(),
                node_id: node.id.clone(),
                chosen_branch: None,
                chosen_label: None,
                verdict: Some(verdict.to_string()),
                duration: None,
                deterministic: true,
                raw_request: error,
                raw_response: None,
                timestamp: now_iso(),
            });
            return Ok(NextDecision {
                next_edge: if matched { Some(loop_continue) } else { loop_exit },
                control_type: if matched {
                    "loop_continue".to_string()
                } else {
                    "loop_exit".to_string()
                },
                reason: format!("iteration {}", iteration),
            });
        }
    }

    if !branch_edges.is_empty() {
        let mut chosen = branch_edges.first().copied().map(|edge| edge.clone());
        if let Some(parsed) = &result.parsed_output {
            chosen = branch_edges
                .iter()
                .find(|edge| {
                    edge.condition.as_ref().is_some_and(|condition| {
                        let (matched, _) = evaluate_condition(parsed, condition);
                        matched
                    })
                })
                .map(|edge| (*edge).clone())
                .or(chosen);
        }
        if chosen.is_none() && checkpoint.use_orchestrator {
            let orchestration =
                run_orchestrator_branch(ctx.session_manager.clone(), &workflow.goal, node, &result.output, &branch_edges, &checkpoint.cwd)
                    .await?;
            let chosen_id = orchestration.output.trim().trim_matches('"').trim_matches('\'');
            chosen = branch_edges
                .iter()
                .find(|edge| edge.branch_id.as_deref() == Some(chosen_id) || edge.id == chosen_id)
                .map(|edge| (*edge).clone())
                .or_else(|| branch_edges.first().copied().map(|edge| edge.clone()));
        }
        if let Some(edge) = chosen {
            emit_event(
                ctx,
                &checkpoint.run_id,
                RuntimeEvent::new("branch_decision")
                    .with("cursorId", cursor_id.to_string())
                    .with("nodeId", node.id.clone())
                    .with(
                        "chosenBranch",
                        edge.branch_id.clone().unwrap_or_else(|| edge.id.clone()),
                    )
                    .with(
                        "chosenLabel",
                        edge.label.clone().unwrap_or_else(|| edge.id.clone()),
                    ),
            )
            .await?;
            return Ok(NextDecision {
                next_edge: Some(edge.clone()),
                control_type: "branch".to_string(),
                reason: edge.label.clone().unwrap_or_else(|| edge.id.clone()),
            });
        }
    }

    Ok(NextDecision {
        next_edge: default_success,
        control_type: "success".to_string(),
        reason: String::new(),
    })
}

async fn finalize_run(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    mut checkpoint: RuntimeCheckpoint,
    duration: Duration,
) -> anyhow::Result<()> {
    let run_id = checkpoint.run_id.clone();
    if checkpoint.execution_log.terminal_reason.is_none()
        && checkpoint
            .split_families
            .values()
            .any(|family| family.force_failed)
    {
        checkpoint.status = RuntimeStatus::Failed;
        checkpoint.execution_log.terminal_reason = Some("failed".to_string());
    } else if checkpoint.execution_log.terminal_reason.is_none() {
        checkpoint.status = RuntimeStatus::Completed;
    }
    if matches!(checkpoint.status, RuntimeStatus::Completed) {
        checkpoint.execution_log.terminal_reason = Some("completed".to_string());
    }

    checkpoint.execution_log.aborted = matches!(checkpoint.status, RuntimeStatus::Aborted);
    checkpoint.execution_log.end_time = Some(now_iso());
    checkpoint.execution_log.total_duration = format!("{:.1}", duration.as_secs_f64());
    checkpoint.execution_log.terminal_reason = checkpoint
        .execution_log
        .terminal_reason
        .clone()
        .or_else(|| Some("completed".to_string()));

    let log_id = build_log_id(&checkpoint.execution_log.workflow_name);
    ctx.db
        .save_execution_log(&log_id, &checkpoint.execution_log)
        .await?;
    emit_event(
        ctx,
        &run_id,
        RuntimeEvent::new("log_saved")
            .with("logId", log_id.clone())
            .with("filename", format!("{}.json", log_id)),
    )
    .await?;

    checkpoint.current_node_id = None;
    checkpoint.current_node_name = None;
    checkpoint.pending_approval = None;
    checkpoint.updated_at = now_iso();
    ctx.db
        .upsert_run(&PersistedRun {
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await?;
    emit_event(
        ctx,
        &run_id,
        RuntimeEvent::new("done")
            .with(
                "aborted",
                matches!(checkpoint.status, RuntimeStatus::Aborted),
            )
            .with("status", checkpoint.status),
    )
    .await?;
    ctx.registry.clear(&run_id).await;
    Ok(())
}

async fn emit_event(ctx: &RuntimeContext, run_id: &str, event: RuntimeEvent) -> anyhow::Result<()> {
    ctx.db.append_event(run_id, &event).await?;
    ctx.registry.send_event(run_id, event).await;
    Ok(())
}

async fn persist_checkpoint(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
) -> anyhow::Result<()> {
    checkpoint.updated_at = now_iso();
    ctx.db
        .upsert_run(&PersistedRun {
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await
}

fn resolve_template_vars(prompt: &str, context: &TemplateRuntimeContext<'_>) -> String {
    let mut resolved = prompt.to_string();
    for (name, value) in context.var_map {
        resolved = resolved.replace(&format!("{{{{var:{}}}}}", name), value);
    }
    for (node_id, result) in context.all_results {
        resolved = resolved.replace(&format!("{{{{{}}}}}", node_id), &result.output);
        resolved = resolved.replace(&format!("{{{{node:{}.output}}}}", node_id), &result.output);
    }

    static PARSED_OUTPUT_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let parsed_re = PARSED_OUTPUT_RE.get_or_init(|| {
        Regex::new(r"\{\{node:([^.}]+)\.parsedOutput\.([^}]+)\}\}").unwrap()
    });
    resolved = parsed_re
        .replace_all(&resolved, |captures: &regex::Captures<'_>| {
            let node_id = captures
                .get(1)
                .map(|capture| capture.as_str())
                .unwrap_or_default();
            let field_path = captures
                .get(2)
                .map(|capture| capture.as_str())
                .unwrap_or_default();
            context
                .all_results
                .get(node_id)
                .and_then(|result| result.parsed_output.as_ref())
                .and_then(|parsed| get_nested_field(parsed, field_path))
                .map(value_to_template_string)
                .unwrap_or_default()
        })
        .into_owned();

    for ContextSource { name, node_id } in &context.current_node.context_sources {
        if let Some(result) = context.all_results.get(node_id) {
            resolved = resolved.replace(&format!("{{{{context:{}}}}}", name), &result.output);
        }
    }

    resolved = resolved.replace("{{previous_output}}", context.last_output);
    resolved = resolved.replace(
        "{{branch_origin}}",
        context.last_branch_origin_id.unwrap_or_default(),
    );
    resolved = resolved.replace(
        "{{branch_choice}}",
        context.last_branch_choice.unwrap_or_default(),
    );

    let predecessor_outputs = context
        .inbound_map
        .get(context.current_node_id)
        .into_iter()
        .flat_map(|predecessors| predecessors.iter())
        .filter_map(|predecessor| context.all_results.get(predecessor))
        .filter_map(|result| {
            if result.output.is_empty() {
                None
            } else if result.stale {
                Some(format!("[preserved from prior run]\n{}", result.output))
            } else {
                Some(result.output.clone())
            }
        })
        .collect::<Vec<_>>()
        .join("\n---\n");
    resolved = resolved.replace("{{all_predecessors}}", &predecessor_outputs);
    resolved
}

fn value_to_template_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

struct TemplateRuntimeContext<'a> {
    current_node_id: &'a str,
    current_node: &'a WorkflowNode,
    all_results: &'a BTreeMap<String, NodeResult>,
    last_output: &'a str,
    var_map: &'a BTreeMap<String, String>,
    inbound_map: &'a HashMap<String, Vec<String>>,
    last_branch_origin_id: Option<&'a str>,
    last_branch_choice: Option<&'a str>,
}

/// Checks if a given agent supports native JSON schema output.

/// Appends JSON format instructions to the prompt when the agent can't enforce
/// the schema natively (i.e. when `native_json_schema` is `None`).
fn wrap_prompt_for_json(
    node: &WorkflowNode,
    mut resolved_prompt: String,
    native_json_schema: &Option<Value>,
) -> String {
    if node.response_format != Some(ResponseFormat::Json) {
        return resolved_prompt;
    }
    // If the driver will enforce the schema natively, skip prompt injection.
    if native_json_schema.is_some() {
        return resolved_prompt;
    }
    resolved_prompt.push_str(
        "\n\nIMPORTANT: You MUST respond with valid JSON only. No markdown, no explanation — just a single JSON object.",
    );
    if let Some(schema) = &node.output_schema {
        let hint = schema_to_prompt_hint(schema);
        if !hint.is_empty() {
            resolved_prompt.push_str(&hint);
        }
    }
    resolved_prompt
}

/// Converts a JSON Schema Value into a human-readable field description for prompt injection.
fn schema_to_prompt_hint(schema: &Value) -> String {
    driver::schema_to_prompt_hint(schema)
}

fn parse_structured_output(node: &WorkflowNode, result: &mut NodeResult) {
    if result.raw_output.is_none() {
        result.raw_output = Some(result.output.clone());
    }
    if node.response_format != Some(ResponseFormat::Json) {
        return;
    }
    // If the driver already extracted structured_output (native JSON schema),
    // use it directly — no need to parse from text.
    if result.parsed_output.is_some() {
        return;
    }
    let mut text = result.output.trim().to_string();
    if let Some(stripped) = strip_markdown_json_fence(&text) {
        text = stripped;
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(parsed) => result.parsed_output = Some(parsed),
        Err(error) => result.parse_error = Some(error.to_string()),
    }
}

fn strip_markdown_json_fence(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !trimmed.starts_with("```") || !trimmed.ends_with("```") {
        return None;
    }
    let lines = trimmed.lines().collect::<Vec<_>>();
    if lines.len() < 3 {
        return None;
    }
    Some(lines[1..lines.len() - 1].join("\n"))
}

fn preview_routing(node: &WorkflowNode, parsed_output: &Option<Value>) -> Option<Value> {
    let parsed = parsed_output.as_ref()?;
    if let Some(condition) = &node.loop_condition {
        let (matched, error) = evaluate_condition(parsed, condition);
        return Some(json!({
            "type": "loop",
            "shouldContinue": matched,
            "error": error,
        }));
    }
    None
}

fn collect_descendants(workflow: &WorkflowV3, start_node_id: &str) -> BTreeSet<String> {
    let graph = workflow.graph();
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([start_node_id.to_string()]);
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.clone()) {
            continue;
        }
        for edge in graph.outgoing_for(&node_id) {
            queue.push_back(edge.to.clone());
        }
    }
    visited
}

fn build_log_id(workflow_name: &str) -> String {
    let slug = slugify_filename(workflow_name);
    format!("{}_{}", slug, Utc::now().format("%Y-%m-%dT%H-%M-%S"))
}

/// Optional context for interactive prompt escalation during PTY command execution.
/// When provided, allows the escalation ladder to emit events and wait for human responses.
struct InteractionContext {
    ctx: RuntimeContext,
    run_id: String,
}

/// Run an agent command via PTY session with the 4-tier escalation ladder.
///
/// Tier 0: CLI flags already suppress most prompts (handled by driver args).
/// Tier 1: Regex auto-respond for known prompts (warmup + interaction patterns).
/// Tier 2: Auto-approve mode sends affirmative to detected prompts (unless destructive).
/// Tier 3: Orchestrator LLM classifies stale output (future: via separate PTY session).
/// Tier 4: Human-in-the-loop via UI (emits event, waits on channel).
async fn run_pty_command(
    session_manager: Arc<SessionManager>,
    agent: &str,
    prompt: &str,
    cwd: &str,
    timeout_secs: Option<u64>,
    config: Option<&AgentConfig>,
) -> anyhow::Result<NodeResult> {
    run_pty_command_with_context(session_manager, agent, prompt, cwd, timeout_secs, config, None)
        .await
}

/// Inner implementation that optionally accepts interaction context for event emission.
async fn run_pty_command_with_context(
    session_manager: Arc<SessionManager>,
    agent: &str,
    prompt: &str,
    cwd: &str,
    timeout_secs: Option<u64>,
    config: Option<&AgentConfig>,
    interaction_ctx: Option<&InteractionContext>,
) -> anyhow::Result<NodeResult> {
    let agent_spec = find_agent(agent).with_context(|| format!("Unknown agent: {}", agent))?;
    anyhow::ensure!(
        agent_spec.capabilities.worker_execution,
        "Agent {} cannot execute workflow nodes",
        agent_spec.name
    );
    let work_dir = if !cwd.is_empty() && Path::new(cwd).exists() {
        cwd.to_string()
    } else {
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
    };
    let search_paths = executable_search_paths(std::env::var_os("PATH").as_deref());
    let executable = resolve_agent_executable(agent_spec.name, &search_paths)?;

    let drv = driver::get_driver(agent_spec.name)
        .with_context(|| format!("No driver for agent: {}", agent_spec.name))?;

    let default_config = AgentConfig::default();
    let cfg = config.unwrap_or(&default_config);
    let cmd = drv.build_session_args(cfg)?;
    let temp_dir = cmd.temp_dir;

    let start = std::time::Instant::now();

    // Create a PTY session
    let session_id = session_manager
        .create_session(
            agent_spec.name,
            &executable.display().to_string(),
            cmd.args,
            cmd.env,
            &work_dir,
        )
        .await
        .with_context(|| format!("Failed to create PTY session for {}", agent_spec.name))?;

    // --- Phase 2: Warmup — auto-respond to startup prompts ---
    let interaction_patterns = drv.interaction_patterns();
    let auto_respond_patterns: Vec<(regex::Regex, String)> = interaction_patterns
        .iter()
        .filter_map(|p| {
            if let driver::InteractionKind::AutoRespond { response } = &p.kind {
                regex::Regex::new(&p.pattern)
                    .ok()
                    .map(|r| (r, response.clone()))
            } else {
                None
            }
        })
        .collect();

    if !auto_respond_patterns.is_empty() {
        if let Err(e) = session_manager
            .warmup_session(&session_id, auto_respond_patterns, Duration::from_secs(8))
            .await
        {
            tracing::warn!("Warmup phase failed (non-fatal): {}", e);
        }
    }

    // --- Build interaction pattern regexes for the sentinel loop ---
    let compiled_patterns: Vec<(regex::Regex, driver::InteractionKind, String)> =
        interaction_patterns
            .iter()
            .filter_map(|p| {
                // Skip AutoRespond patterns — those are handled in warmup
                if matches!(p.kind, driver::InteractionKind::AutoRespond { .. }) {
                    return None;
                }
                regex::Regex::new(&p.pattern)
                    .ok()
                    .map(|r| (r, p.kind.clone(), p.description.clone()))
            })
            .collect();

    // Build destructive pattern regexes
    let destructive_regexes: Vec<regex::Regex> = drv
        .destructive_blocklist()
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();

    // Wrap prompt with sentinel for completion detection
    let sentinel = format!("SILVERBOND_DONE_{}", uuid::Uuid::new_v4());
    let wrapped_prompt = drv.wrap_prompt_with_sentinel(prompt, &sentinel);

    // --- Phase 3: Interactive sentinel loop ---
    let total_timeout = Duration::from_secs(timeout_secs.unwrap_or(300));
    let intermediate_timeout = Duration::from_secs(10);
    let subagent_timeout = cfg
        .orchestrator
        .as_ref()
        .and_then(|o| o.subagent_timeout_secs)
        .map(|s| Duration::from_secs(s as u64))
        .unwrap_or(Duration::from_secs(600));

    let mut total_deadline = std::time::Instant::now() + total_timeout;
    let mut stale_count = 0u32;

    let response = loop {
        let remaining = total_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            let elapsed = start.elapsed().as_secs_f64();
            let _ = session_manager.close_session(&session_id).await;
            if let Some(dir) = &temp_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            return Ok(NodeResult {
                success: false,
                output: String::new(),
                stderr: "Total timeout exceeded".to_string(),
                exit_code: -1,
                duration: format!("{:.1}", elapsed),
                agent: agent_spec.name.to_string(),
                prompt: prompt.to_string(),
                metadata: AgentExecutionMetadata {
                    outcome: Some(NodeOutcome::ErrorTimeout),
                    error_type: Some("timeout".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            });
        }

        let event = session_manager
            .send_prompt_interactive(
                &session_id,
                &wrapped_prompt,
                &sentinel,
                compiled_patterns.clone(),
                intermediate_timeout,
                remaining,
            )
            .await?;

        match event {
            crate::session::PromptEvent::Completed(resp) => {
                break resp;
            }

            crate::session::PromptEvent::InteractionRequired { kind, description, output_so_far } => {
                match kind {
                    // Tier 1: Auto-respond (shouldn't reach here since handled in warmup, but safety net)
                    driver::InteractionKind::AutoRespond { response } => {
                        session_manager.respond_to_interaction(&session_id, &response).await?;
                        continue;
                    }

                    // Subagent detected — extend timeout and continue
                    driver::InteractionKind::SubagentActive => {
                        total_deadline = std::time::Instant::now() + subagent_timeout;
                        continue;
                    }

                    // Permission request
                    driver::InteractionKind::PermissionRequest => {
                        // Check if output contains destructive patterns
                        let is_destructive = destructive_regexes
                            .iter()
                            .any(|r| r.is_match(&output_so_far));

                        if is_destructive {
                            // Tier 4: Always escalate destructive patterns
                            if let Some(ictx) = interaction_ctx {
                                let response = escalate_to_human(
                                    ictx,
                                    &session_id,
                                    "destructive_warning",
                                    &description,
                                    &output_so_far,
                                )
                                .await?;
                                session_manager
                                    .respond_to_interaction(&session_id, &response)
                                    .await?;
                                continue;
                            }
                            // No interaction context — auto-reject destructive
                            session_manager
                                .respond_to_interaction(&session_id, "n")
                                .await?;
                            continue;
                        }

                        if cfg.auto_approve {
                            // Tier 2: Auto-approve non-destructive
                            session_manager
                                .respond_to_interaction(&session_id, "y")
                                .await?;
                            continue;
                        }

                        // Tier 4: Escalate to human
                        if let Some(ictx) = interaction_ctx {
                            let response = escalate_to_human(
                                ictx,
                                &session_id,
                                "permission",
                                &description,
                                &output_so_far,
                            )
                            .await?;
                            session_manager
                                .respond_to_interaction(&session_id, &response)
                                .await?;
                            continue;
                        }
                        // No interaction context — auto-approve as fallback
                        session_manager
                            .respond_to_interaction(&session_id, "y")
                            .await?;
                        continue;
                    }

                    // Destructive warning (from pattern matching directly)
                    driver::InteractionKind::DestructiveWarning => {
                        if let Some(ictx) = interaction_ctx {
                            let response = escalate_to_human(
                                ictx,
                                &session_id,
                                "destructive_warning",
                                &description,
                                &output_so_far,
                            )
                            .await?;
                            session_manager
                                .respond_to_interaction(&session_id, &response)
                                .await?;
                            continue;
                        }
                        session_manager
                            .respond_to_interaction(&session_id, "n")
                            .await?;
                        continue;
                    }
                }
            }

            crate::session::PromptEvent::StaleDetected { output_so_far } => {
                stale_count += 1;

                // Check if process is still alive
                if !session_manager.is_alive(&session_id).await {
                    let elapsed = start.elapsed().as_secs_f64();
                    let _ = session_manager.close_session(&session_id).await;
                    if let Some(dir) = &temp_dir {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                    return Ok(NodeResult {
                        success: !output_so_far.is_empty(),
                        output: output_so_far,
                        stderr: "Agent process exited without sentinel".to_string(),
                        exit_code: -1,
                        duration: format!("{:.1}", elapsed),
                        agent: agent_spec.name.to_string(),
                        prompt: prompt.to_string(),
                        metadata: AgentExecutionMetadata {
                            outcome: Some(NodeOutcome::ErrorExecution),
                            error_type: Some("process_exited".to_string()),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }

                // First few stale events — just continue waiting (agent may be thinking)
                if stale_count <= 3 {
                    continue;
                }

                // Tier 4: Escalate to human after repeated stale detections
                if let Some(ictx) = interaction_ctx {
                    let response = escalate_to_human(
                        ictx,
                        &session_id,
                        "question",
                        "Agent output appears stale — it may be waiting for input",
                        &output_so_far,
                    )
                    .await?;
                    session_manager
                        .respond_to_interaction(&session_id, &response)
                        .await?;
                    stale_count = 0;
                    continue;
                }

                // No interaction context — continue waiting
                continue;
            }

            crate::session::PromptEvent::Timeout { output_so_far } => {
                let elapsed = start.elapsed().as_secs_f64();
                let _ = session_manager.close_session(&session_id).await;
                if let Some(dir) = &temp_dir {
                    let _ = std::fs::remove_dir_all(dir);
                }
                return Ok(NodeResult {
                    success: false,
                    output: output_so_far,
                    stderr: "Timeout waiting for agent response".to_string(),
                    exit_code: -1,
                    duration: format!("{:.1}", elapsed),
                    agent: agent_spec.name.to_string(),
                    prompt: prompt.to_string(),
                    metadata: AgentExecutionMetadata {
                        outcome: Some(NodeOutcome::ErrorTimeout),
                        error_type: Some("timeout".to_string()),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }
        }
    };

    let elapsed = start.elapsed().as_secs_f64();

    // Query cost metadata if supported
    let cost = if let Some(cost_cmd) = drv.cost_command() {
        session_manager
            .send_command(&session_id, cost_cmd)
            .await
            .ok()
            .and_then(|out| drv.parse_cost_response(&out))
    } else {
        None
    };

    // Query context usage if supported
    let context_pct = if let Some(ctx_cmd) = drv.context_command() {
        session_manager
            .send_command(&session_id, ctx_cmd)
            .await
            .ok()
            .and_then(|out| drv.parse_context_response(&out))
            .and_then(|info| info.used_percentage)
    } else {
        None
    };

    // Clean up temp dir from driver args
    if let Some(dir) = &temp_dir {
        let _ = std::fs::remove_dir_all(dir);
    }

    Ok(NodeResult {
        success: true,
        output: response.text,
        stderr: String::new(),
        exit_code: 0,
        duration: format!("{:.1}", elapsed),
        agent: agent_spec.name.to_string(),
        prompt: prompt.to_string(),
        raw_output: Some(String::from_utf8_lossy(&response.raw).to_string()),
        metadata: AgentExecutionMetadata {
            outcome: Some(NodeOutcome::Success),
            agent_session_id: Some(session_id),
            cost_usd: cost.as_ref().and_then(|c| c.total_cost_usd),
            input_tokens: cost.as_ref().and_then(|c| c.input_tokens),
            output_tokens: cost.as_ref().and_then(|c| c.output_tokens),
            thinking_tokens: cost.as_ref().and_then(|c| c.thinking_tokens),
            cache_read_tokens: cost.as_ref().and_then(|c| c.cache_read_tokens),
            cache_write_tokens: cost.as_ref().and_then(|c| c.cache_write_tokens),
            context_used_pct: context_pct,
            ..Default::default()
        },
        ..Default::default()
    })
}

/// Escalate an interaction to the human via UI events and wait for their response.
async fn escalate_to_human(
    ictx: &InteractionContext,
    session_id: &str,
    interaction_type: &str,
    description: &str,
    output_so_far: &str,
) -> anyhow::Result<String> {
    // Emit event to UI
    emit_event(
        &ictx.ctx,
        &ictx.run_id,
        RuntimeEvent::new("agent_interaction_required")
            .with("sessionId", session_id)
            .with("interactionType", interaction_type)
            .with("description", description)
            .with("outputSoFar", output_so_far),
    )
    .await?;

    // Wait for human response via oneshot channel
    let (sender, receiver) = oneshot::channel();
    ictx.ctx
        .registry
        .set_pending_interaction(&ictx.run_id, sender)
        .await?;

    let interaction_response = receiver
        .await
        .map_err(|_| anyhow::anyhow!("Interaction channel closed"))?;

    // Emit resolved event
    emit_event(
        &ictx.ctx,
        &ictx.run_id,
        RuntimeEvent::new("agent_interaction_resolved")
            .with("sessionId", session_id)
            .with("description", description)
            .with("response", &interaction_response.response),
    )
    .await?;

    Ok(interaction_response.response)
}

async fn run_orchestrator_refinement(
    session_manager: Arc<SessionManager>,
    goal: &str,
    node: &WorkflowNode,
    original_prompt: &str,
    previous_output: &str,
    step_index: u32,
    total_steps: usize,
    cwd: &str,
) -> anyhow::Result<NodeResult> {
    let prompt = format!(
        "You are an AI orchestrator managing a multi-step agentic workflow.\n\nWORKFLOW GOAL: {goal}\nCURRENT STEP: {} of {} — \"{}\"\nORIGINAL PROMPT: {original_prompt}\nPREVIOUS OUTPUT: {}\n\nRewrite or refine the prompt for this step to make it as effective as possible given the workflow goal and the previous output. Keep it if it is already optimal.\nRespond with ONLY the final prompt text. No explanation, no markdown, no preamble.",
        step_index + 1,
        total_steps,
        node.name,
        if previous_output.is_empty() {
            "(first step — no prior output)"
        } else {
            previous_output
        }
    );
    run_pty_command(session_manager, DEFAULT_AGENT, &prompt, cwd, None, None).await
}

async fn run_orchestrator_branch(
    session_manager: Arc<SessionManager>,
    goal: &str,
    node: &WorkflowNode,
    output: &str,
    branches: &[&WorkflowEdge],
    cwd: &str,
) -> anyhow::Result<NodeResult> {
    let branch_list = branches
        .iter()
        .map(|edge| {
            format!(
                "- \"{}\": {}",
                edge.branch_id.clone().unwrap_or_else(|| edge.id.clone()),
                edge.label.clone().unwrap_or_else(|| edge.id.clone())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "You are an AI orchestrator deciding which branch a workflow should take.\n\nWORKFLOW GOAL: {goal}\nSTEP JUST COMPLETED: \"{}\"\nOUTPUT OF THAT STEP:\n{}\n\nAVAILABLE BRANCHES:\n{}\n\nBased on the output and the workflow goal, choose the most appropriate branch.\nRespond with ONLY the branch id string (e.g. branch_a). Nothing else.",
        node.name, output, branch_list
    );
    run_pty_command(session_manager, DEFAULT_AGENT, &prompt, cwd, None, None).await
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        path::{Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant},
    };

    use serde_json::{Value, json};
    use tempfile::TempDir;

    use crate::{
        driver::{get_driver, AccessMode, AgentConfig, ReasoningLevel},
        model::{
            normalize_workflow_value, resolve_agent_config, AgentDefaults, AgentNodeConfig,
            SplitFailurePolicy, WorkflowEdge, WorkflowEdgeOutcome, WorkflowLimits, WorkflowNode,
            WorkflowNodeType, WorkflowV3,
        },
        storage::Database,
    };

    use super::*;

    #[derive(Debug, Clone)]
    struct ScriptedStep {
        success: bool,
        output: String,
        stderr: String,
        exit_code: i32,
        delay_ms: u64,
        parsed_output: Option<Value>,
    }

    impl ScriptedStep {
        fn success(output: &str) -> Self {
            Self {
                success: true,
                output: output.to_string(),
                stderr: String::new(),
                exit_code: 0,
                delay_ms: 0,
                parsed_output: None,
            }
        }

        fn failure(output: &str, stderr: &str) -> Self {
            Self {
                success: false,
                output: output.to_string(),
                stderr: stderr.to_string(),
                exit_code: 1,
                delay_ms: 0,
                parsed_output: None,
            }
        }

        fn with_delay(mut self, delay_ms: u64) -> Self {
            self.delay_ms = delay_ms;
            self
        }
    }

    #[derive(Clone)]
    struct ScriptedRunner {
        steps: Arc<Mutex<HashMap<String, VecDeque<ScriptedStep>>>>,
    }

    impl ScriptedRunner {
        fn new(script: impl IntoIterator<Item = (String, Vec<ScriptedStep>)>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(
                    script
                        .into_iter()
                        .map(|(prompt, steps)| (prompt, VecDeque::from(steps)))
                        .collect(),
                )),
            }
        }
    }

    impl NodeRunner for ScriptedRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            let steps = self.steps.clone();
            Box::pin(async move {
                let step = {
                    let mut guard = steps.lock().await;
                    let Some(queue) = guard.get_mut(&prompt) else {
                        return Err(anyhow::anyhow!("No scripted step for prompt \"{}\"", prompt));
                    };
                    let Some(step) = queue.pop_front() else {
                        return Err(anyhow::anyhow!(
                            "No scripted steps remaining for prompt \"{}\"",
                            prompt
                        ));
                    };
                    step
                };
                if step.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
                }
                Ok(NodeResult {
                    success: step.success,
                    output: step.output,
                    stderr: step.stderr,
                    exit_code: step.exit_code,
                    duration: format!("{:.3}", step.delay_ms as f64 / 1000.0),
                    agent: "mock".to_string(),
                    prompt: prompt.clone(),
                    parsed_output: step.parsed_output,
                    resolved_prompt: Some(prompt),
                    ..Default::default()
                })
            })
        }
    }

    fn task_node(id: &str, name: &str, prompt: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: name.to_string(),
            node_type: WorkflowNodeType::Task,
            agent: Some("mock".to_string()),
            prompt: prompt.to_string(),
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
            agent_config: None,
            cwd: None,
            continue_session_from: None,
        }
    }

    fn approval_node(id: &str, name: &str, prompt: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: name.to_string(),
            node_type: WorkflowNodeType::Approval,
            agent: None,
            prompt: prompt.to_string(),
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
            agent_config: None,
            cwd: None,
            continue_session_from: None,
        }
    }

    fn split_node(id: &str, policy: SplitFailurePolicy) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: format!("Split {}", id),
            node_type: WorkflowNodeType::Split,
            agent: None,
            prompt: String::new(),
            context_sources: Vec::new(),
            response_format: None,
            output_schema: None,
            retry_count: None,
            retry_delay: None,
            timeout: None,
            skip_condition: None,
            loop_max_iterations: None,
            loop_condition: None,
            split_failure_policy: policy,
            agent_config: None,
            cwd: None,
            continue_session_from: None,
        }
    }

    fn collector_node(id: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: format!("Collector {}", id),
            node_type: WorkflowNodeType::Collector,
            agent: None,
            prompt: String::new(),
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
            agent_config: None,
            cwd: None,
            continue_session_from: None,
        }
    }

    fn success_edge(id: &str, from: &str, to: &str, label: Option<&str>) -> WorkflowEdge {
        WorkflowEdge {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            outcome: WorkflowEdgeOutcome::Success,
            label: label.map(str::to_string),
            branch_id: None,
            condition: None,
        }
    }

    fn workflow_from_parts(
        entry_node_id: &str,
        nodes: Vec<WorkflowNode>,
        edges: Vec<WorkflowEdge>,
    ) -> WorkflowV3 {
        WorkflowV3 {
            version: 3,
            name: Some("test".to_string()),
            goal: "goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            entry_node_id: entry_node_id.to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 20,
                max_visits_per_node: 10,
            },
            nodes,
            edges,
            agent_defaults: BTreeMap::new(),
            ui: None,
        }
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
                "timed out waiting for run {}",
                run_id
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_terminal_run(db: &Database, run_id: &str) -> PersistedRun {
        wait_for_run(db, run_id, |persisted| {
            matches!(
                persisted.checkpoint.status,
                RuntimeStatus::Completed | RuntimeStatus::Failed | RuntimeStatus::Aborted
            )
        })
        .await
    }

    fn basic_workflow() -> WorkflowV3 {
        WorkflowV3 {
            version: 3,
            name: Some("test".to_string()),
            goal: "goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 10,
                max_visits_per_node: 5,
            },
            nodes: vec![
                WorkflowNode {
                    id: "n1".to_string(),
                    name: "Step 1".to_string(),
                    node_type: WorkflowNodeType::Task,
                    agent: Some("claude".to_string()),
                    prompt: "hello".to_string(),
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
                    agent_config: None,
                    cwd: None,
                    continue_session_from: None,
                },
                WorkflowNode {
                    id: "n2".to_string(),
                    name: "Step 2".to_string(),
                    node_type: WorkflowNodeType::Task,
                    agent: Some("claude".to_string()),
                    prompt: "world".to_string(),
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
                    agent_config: None,
                    cwd: None,
                    continue_session_from: None,
                },
            ],
            edges: vec![WorkflowEdge {
                id: "e1".to_string(),
                from: "n1".to_string(),
                to: "n2".to_string(),
                outcome: WorkflowEdgeOutcome::Success,
                label: None,
                branch_id: None,
                condition: None,
            }],
            agent_defaults: BTreeMap::new(),
            ui: None,
        }
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn resolves_executable_from_search_paths() {
        let temp = TempDir::new().unwrap();
        let executable = temp.path().join("claude");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        make_executable(&executable);

        let resolved = resolve_executable("claude", &[temp.path().to_path_buf()]).unwrap();

        assert_eq!(resolved, executable);
    }

    #[cfg(unix)]
    #[test]
    fn resolves_agent_executable_finds_binary() {
        let temp = TempDir::new().unwrap();
        let executable = temp.path().join("codex");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        make_executable(&executable);
        let search_paths = vec![PathBuf::from(temp.path())];

        let resolved = super::resolve_agent_executable("codex", &search_paths).unwrap();
        assert_eq!(resolved, executable);
    }

    #[tokio::test]
    async fn starts_and_persists_run() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let run_id = runtime
            .start_run(basic_workflow(), BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = db.get_run(&run_id).await.unwrap();
        assert!(persisted.is_some());
    }

    #[test]
    fn resolves_template_vars() {
        let node = WorkflowNode {
            id: "n2".to_string(),
            name: "Step 2".to_string(),
            node_type: WorkflowNodeType::Task,
            agent: Some("claude".to_string()),
            prompt: "Prev {{previous_output}} Var {{var:name}} {{node:n1.output}}".to_string(),
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
            agent_config: None,
            cwd: None,
            continue_session_from: None,
        };
        let mut results = BTreeMap::new();
        results.insert(
            "n1".to_string(),
            NodeResult {
                success: true,
                output: "done".to_string(),
                agent: "claude".to_string(),
                prompt: "p".to_string(),
                ..Default::default()
            },
        );
        let mut vars = BTreeMap::new();
        vars.insert("name".to_string(), "world".to_string());
        let value = resolve_template_vars(
            &node.prompt,
            &TemplateRuntimeContext {
                current_node_id: "n2",
                current_node: &node,
                all_results: &results,
                last_output: "last",
                var_map: &vars,
                inbound_map: &HashMap::new(),
                last_branch_origin_id: None,
                last_branch_choice: None,
            },
        );
        assert!(value.contains("last"));
        assert!(value.contains("world"));
        assert!(value.contains("done"));
    }

    #[tokio::test]
    async fn split_spawns_collects_and_continues() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                ("branch-a".to_string(), vec![ScriptedStep::success("alpha output").with_delay(60)]),
                ("branch-b".to_string(), vec![ScriptedStep::success("beta output").with_delay(5)]),
                ("after".to_string(), vec![ScriptedStep::success("done")]),
            ])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("branch_a", "Branch A", "branch-a"),
                task_node("branch_b", "Branch B", "branch-b"),
                collector_node("collector"),
                task_node("after", "After", "after"),
            ],
            vec![
                success_edge("split_a", "split", "branch_a", Some("alpha")),
                success_edge("split_b", "split", "branch_b", Some("beta")),
                success_edge("join_a", "branch_a", "collector", Some("alpha")),
                success_edge("join_b", "branch_b", "collector", Some("beta")),
                success_edge("after_edge", "collector", "after", None),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert!(persisted.checkpoint.all_results.contains_key("after"));

        let collector = persisted.checkpoint.all_results.get("collector").unwrap();
        let parsed = collector.parsed_output.as_ref().unwrap();
        assert_eq!(parsed["summary"]["total"], json!(2));
        assert_eq!(parsed["summary"]["succeeded"], json!(2));
        assert_eq!(parsed["inputs"]["alpha"]["output"], json!("alpha output"));
        assert_eq!(parsed["inputs"]["beta"]["output"], json!("beta output"));

        let events = db.list_events(&run_id).await.unwrap();
        assert_eq!(
            events.iter().filter(|event| event.kind == "cursor_spawned").count(),
            2
        );
        assert!(events.iter().any(|event| event.kind == "aggregate_merged"));
        assert!(events.iter().any(|event| event.kind == "collector_released"));
        assert!(events
            .iter()
            .filter(|event| event.kind == "transition")
            .all(|event| event.data.contains_key("cursorId")));
    }

    #[tokio::test]
    async fn best_effort_continue_keeps_run_completable_after_branch_failure() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                ("branch-a".to_string(), vec![ScriptedStep::success("alpha ok")]),
                (
                    "branch-b".to_string(),
                    vec![ScriptedStep::failure("beta failed", "boom").with_delay(5)],
                ),
                ("after".to_string(), vec![ScriptedStep::success("done")]),
            ])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("branch_a", "Branch A", "branch-a"),
                task_node("branch_b", "Branch B", "branch-b"),
                collector_node("collector"),
                task_node("after", "After", "after"),
            ],
            vec![
                success_edge("split_a", "split", "branch_a", Some("alpha")),
                success_edge("split_b", "split", "branch_b", Some("beta")),
                success_edge("join_a", "branch_a", "collector", Some("alpha")),
                success_edge("join_b", "branch_b", "collector", Some("beta")),
                success_edge("after_edge", "collector", "after", None),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert!(persisted.checkpoint.all_results.contains_key("after"));

        let collector = persisted.checkpoint.all_results.get("collector").unwrap();
        let parsed = collector.parsed_output.as_ref().unwrap();
        assert_eq!(parsed["summary"]["succeeded"], json!(1));
        assert_eq!(parsed["summary"]["failed"], json!(1));
        assert_eq!(parsed["inputs"]["beta"]["status"], json!("failure"));
        assert_eq!(parsed["inputs"]["beta"]["stderr"], json!("boom"));
    }

    #[tokio::test]
    async fn drain_then_fail_waits_for_siblings_before_failing_run() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                ("branch-a".to_string(), vec![ScriptedStep::success("alpha ok").with_delay(30)]),
                (
                    "branch-b".to_string(),
                    vec![ScriptedStep::failure("beta failed", "boom").with_delay(5)],
                ),
                ("after".to_string(), vec![ScriptedStep::success("done")]),
            ])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::DrainThenFail),
                task_node("branch_a", "Branch A", "branch-a"),
                task_node("branch_b", "Branch B", "branch-b"),
                collector_node("collector"),
                task_node("after", "After", "after"),
            ],
            vec![
                success_edge("split_a", "split", "branch_a", Some("alpha")),
                success_edge("split_b", "split", "branch_b", Some("beta")),
                success_edge("join_a", "branch_a", "collector", Some("alpha")),
                success_edge("join_b", "branch_b", "collector", Some("beta")),
                success_edge("after_edge", "collector", "after", None),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Failed);
        assert!(persisted.checkpoint.all_results.contains_key("collector"));
        assert!(persisted.checkpoint.all_results.contains_key("after"));
        assert!(persisted
            .checkpoint
            .split_families
            .values()
            .all(|family| family.force_failed));
    }

    #[tokio::test]
    async fn fail_fast_cancel_stops_slow_siblings() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "slow-branch".to_string(),
                    vec![ScriptedStep::success("slow ok").with_delay(200)],
                ),
                (
                    "fast-fail".to_string(),
                    vec![ScriptedStep::failure("failed", "boom").with_delay(5)],
                ),
            ])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::FailFastCancel),
                task_node("slow", "Slow", "slow-branch"),
                task_node("fast", "Fast", "fast-fail"),
            ],
            vec![
                success_edge("split_slow", "split", "slow", Some("slow")),
                success_edge("split_fast", "split", "fast", Some("fast")),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Failed);
        assert!(persisted.checkpoint.all_results.contains_key("fast"));
        assert!(!persisted.checkpoint.all_results.contains_key("slow"));
    }

    #[tokio::test]
    async fn approvals_are_queued_one_cursor_at_a_time() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([(
                "after".to_string(),
                vec![ScriptedStep::success("done")],
            )])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                approval_node("approve_a", "Approve A", "approve alpha"),
                approval_node("approve_b", "Approve B", "approve beta"),
                collector_node("collector"),
                task_node("after", "After", "after"),
            ],
            vec![
                success_edge("split_a", "split", "approve_a", Some("alpha")),
                success_edge("split_b", "split", "approve_b", Some("beta")),
                success_edge("join_a", "approve_a", "collector", Some("alpha")),
                success_edge("join_b", "approve_b", "collector", Some("beta")),
                success_edge("after_edge", "collector", "after", None),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let pending_a = wait_for_run(&db, &run_id, |persisted| {
            persisted
                .checkpoint
                .pending_approval
                .as_ref()
                .is_some_and(|pending| pending.node_id == "approve_a")
                && persisted.checkpoint.queued_approvals.len() == 1
        })
        .await;
        assert_eq!(pending_a.checkpoint.queued_approvals[0].approval.node_id, "approve_b");

        runtime
            .approve_run(&run_id, true, "approved alpha".to_string())
            .await
            .unwrap();
        let pending_b = wait_for_run(&db, &run_id, |persisted| {
            persisted
                .checkpoint
                .pending_approval
                .as_ref()
                .is_some_and(|pending| pending.node_id == "approve_b")
                && persisted.checkpoint.queued_approvals.is_empty()
        })
        .await;
        assert_eq!(
            pending_b
                .checkpoint
                .pending_approval
                .as_ref()
                .unwrap()
                .last_output,
            String::new()
        );

        runtime
            .approve_run(&run_id, true, "approved beta".to_string())
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        let events = db.list_events(&run_id).await.unwrap();
        assert_eq!(
            events.iter().filter(|event| event.kind == "approval_queued").count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "approval_required")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn restart_from_advances_epoch_and_drops_stale_collector_arrivals() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "fail-now".to_string(),
                    vec![ScriptedStep::failure("failed", "boom").with_delay(5)],
                ),
                ("after".to_string(), vec![ScriptedStep::success("done")]),
            ])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("fail_branch", "Fail Branch", "fail-now"),
                approval_node("approve_branch", "Approve Branch", "approve"),
                collector_node("collector"),
                task_node("after", "After", "after"),
            ],
            vec![
                success_edge("split_fail", "split", "fail_branch", Some("left")),
                success_edge("split_approve", "split", "approve_branch", Some("right")),
                success_edge("join_fail", "fail_branch", "collector", Some("left")),
                success_edge("join_approve", "approve_branch", "collector", Some("right")),
                success_edge("after_edge", "collector", "after", None),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let original = wait_for_run(&db, &run_id, |persisted| {
            persisted
                .checkpoint
                .pending_approval
                .as_ref()
                .is_some_and(|pending| pending.node_id == "approve_branch")
                && !persisted.checkpoint.collector_barriers.is_empty()
        })
        .await;
        let original_epoch = original.checkpoint.execution_epoch;

        let restarted_run_id = runtime.restart_from(&run_id, "approve_branch").await.unwrap();
        let restarted = wait_for_run(&db, &restarted_run_id, |persisted| {
            persisted
                .checkpoint
                .pending_approval
                .as_ref()
                .is_some_and(|pending| pending.node_id == "approve_branch")
        })
        .await;

        assert_eq!(restarted.checkpoint.execution_epoch, original_epoch + 1);
        assert!(restarted.checkpoint.collector_barriers.is_empty());

        runtime
            .approve_run(&restarted_run_id, true, "approved".to_string())
            .await
            .unwrap();
        let finished = wait_for_terminal_run(&db, &restarted_run_id).await;

        assert_eq!(finished.checkpoint.status, RuntimeStatus::Failed);
        assert!(!finished.checkpoint.all_results.contains_key("after"));
        assert!(
            !finished
                .checkpoint
                .all_results
                .contains_key("collector")
        );
    }

    // -----------------------------------------------------------------------
    // Stage 4: JSON output parsing & metadata extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn wrap_prompt_injects_when_no_native_json_schema() {
        let mut node = task_node("n1", "Test", "prompt");
        node.response_format = Some(ResponseFormat::Json);
        node.output_schema = Some(json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        }));

        let prompt = "Do something".to_string();
        let result = super::wrap_prompt_for_json(&node, prompt.clone(), &None);
        assert!(result.contains("MUST respond with valid JSON"));
        assert!(result.contains("  - name (string, required)"));
    }

    #[test]
    fn wrap_prompt_injects_when_no_schema_configured() {
        let mut node = task_node("n1", "Test", "prompt");
        node.response_format = Some(ResponseFormat::Json);
        node.output_schema = None;

        let prompt = "Do something".to_string();
        // No native schema available (None) → falls back to prompt injection
        let result = super::wrap_prompt_for_json(&node, prompt.clone(), &None);
        assert!(result.contains("MUST respond with valid JSON"));
    }

    #[test]
    fn wrap_prompt_noop_for_text_format() {
        let node = task_node("n1", "Test", "prompt");
        let prompt = "Do something".to_string();
        let result = super::wrap_prompt_for_json(&node, prompt.clone(), &None);
        assert_eq!(result, prompt);
    }

    fn mock_node_result(output: &str) -> NodeResult {
        NodeResult {
            success: true,
            output: output.to_string(),
            duration: "1.0".to_string(),
            agent: "mock".to_string(),
            prompt: "test".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn parse_structured_output_uses_existing_parsed_output() {
        let mut node = task_node("n1", "Test", "prompt");
        node.response_format = Some(ResponseFormat::Json);

        let mut result = mock_node_result("raw text not json");
        result.raw_output = Some("original raw".to_string());
        result.parsed_output = Some(json!({"name": "Alice"}));

        super::parse_structured_output(&node, &mut result);
        // Should keep the existing parsed_output, not try to parse "raw text not json"
        assert_eq!(result.parsed_output, Some(json!({"name": "Alice"})));
        assert!(result.parse_error.is_none());
    }

    #[test]
    fn parse_structured_output_falls_back_to_text_parsing() {
        let mut node = task_node("n1", "Test", "prompt");
        node.response_format = Some(ResponseFormat::Json);

        let mut result = mock_node_result(r#"{"name": "Bob"}"#);

        super::parse_structured_output(&node, &mut result);
        assert_eq!(result.parsed_output, Some(json!({"name": "Bob"})));
    }

    // -----------------------------------------------------------------------
    // Stage 5: Output schema enhancement tests
    // -----------------------------------------------------------------------

    #[test]
    fn schema_to_prompt_hint_generates_rich_descriptions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "The person's name" },
                "age": { "type": "integer" }
            },
            "required": ["name"]
        });
        let hint = super::schema_to_prompt_hint(&schema);
        // Uses GeminiDriver's bulleted list format
        assert!(hint.contains("name (string, required) — The person's name"));
        assert!(hint.contains("age (integer)"));
        assert!(!hint.contains("age (integer, required)"));
    }

    #[test]
    fn schema_to_prompt_hint_empty_for_no_properties() {
        let schema = json!({"type": "object"});
        let hint = super::schema_to_prompt_hint(&schema);
        assert!(hint.is_empty());
    }

    #[test]
    fn schema_to_prompt_hint_empty_for_non_object() {
        let hint = super::schema_to_prompt_hint(&json!("not an object"));
        assert!(hint.is_empty());
    }

    #[test]
    fn wrap_prompt_generates_rich_hints_from_json_schema() {
        let mut node = task_node("n1", "Test", "prompt");
        node.response_format = Some(ResponseFormat::Json);
        node.output_schema = Some(json!({
            "type": "object",
            "properties": {
                "result": { "type": "string", "description": "The result" }
            },
            "required": ["result"]
        }));

        let prompt = "Do something".to_string();
        let result = super::wrap_prompt_for_json(&node, prompt, &None);
        assert!(result.contains("  - result (string, required) — The result"));
    }

    // -----------------------------------------------------------------------
    // Stage 8: Session reuse tests
    // -----------------------------------------------------------------------

    #[test]
    fn build_session_persistence_set_collects_referenced_nodes() {
        let n1 = task_node("n1", "Step 1", "prompt1");
        let mut n2 = task_node("n2", "Step 2", "prompt2");
        n2.continue_session_from = Some("n1".to_string());
        let mut n3 = task_node("n3", "Step 3", "prompt3");
        n3.continue_session_from = Some("n2".to_string());

        let wf = WorkflowV3 {
            version: 3,
            name: None,
            goal: "test".to_string(),
            cwd: "/tmp".to_string(),
            use_orchestrator: false,
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits { max_total_steps: 10, max_visits_per_node: 5 },
            nodes: vec![n1, n2, n3],
            edges: vec![],
            agent_defaults: BTreeMap::new(),
            ui: None,
        };

        let set = super::build_session_persistence_set(&wf);
        assert!(set.contains("n1"), "n1 should need persistent session");
        assert!(set.contains("n2"), "n2 should need persistent session");
        assert!(!set.contains("n3"), "n3 is not referenced by anyone");
    }

    // -----------------------------------------------------------------------
    // Stage 9: Integration — config resolution + driver build_args pipeline
    // -----------------------------------------------------------------------

    #[test]
    fn integration_mixed_agents_config_resolution() {
        // Workflow with Claude and Codex nodes using different configs
        let mut defaults = BTreeMap::new();
        defaults.insert("claude".to_string(), AgentDefaults {
            model: Some("sonnet".to_string()),
            max_turns: Some(5),
            ..Default::default()
        });
        defaults.insert("codex".to_string(), AgentDefaults {
            model: Some("o3-mini".to_string()),
            reasoning_level: Some(ReasoningLevel::Medium),
            ..Default::default()
        });

        // Claude node with node-level override
        let mut claude_node = task_node("n1", "Claude Step", "prompt1");
        claude_node.agent = Some("claude".to_string());
        claude_node.agent_config = Some(AgentNodeConfig {
            base: AgentDefaults {
                max_budget_usd: Some(1.5),
                ..Default::default()
            },
            ..Default::default()
        });

        let claude_config = resolve_agent_config(&defaults, "/work", "claude", &claude_node, None, false, None);
        assert_eq!(claude_config.model.as_deref(), Some("sonnet")); // from defaults
        assert_eq!(claude_config.max_turns, Some(5)); // from defaults
        assert_eq!(claude_config.max_budget_usd, Some(1.5)); // from node override

        let claude_driver = get_driver("claude").unwrap();
        let claude_cmd = claude_driver.build_session_args(&claude_config).unwrap();
        assert!(claude_cmd.args.iter().any(|a| a == "--model"));
        assert!(claude_cmd.args.iter().any(|a| a == "--max-budget-usd"));

        // Codex node uses only defaults
        let mut codex_node = task_node("n2", "Codex Step", "prompt2");
        codex_node.agent = Some("codex".to_string());

        let codex_config = resolve_agent_config(&defaults, "/work", "codex", &codex_node, None, false, None);
        assert_eq!(codex_config.model.as_deref(), Some("o3-mini"));
        assert_eq!(codex_config.reasoning_level, Some(ReasoningLevel::Medium));

        let codex_driver = get_driver("codex").unwrap();
        let codex_cmd = codex_driver.build_session_args(&codex_config).unwrap();
        assert!(codex_cmd.args.iter().any(|a| a == "--model"));
        assert!(codex_cmd.args.iter().any(|a| a.contains("model_reasoning_effort=medium")));
    }

    #[test]
    fn integration_session_args_no_json_mode_flags() {
        // With PTY mode, no agent should have --json-schema, --print, --json, etc.
        let claude_driver = get_driver("claude").unwrap();
        let claude_cmd = claude_driver.build_session_args(&Default::default()).unwrap();
        assert!(!claude_cmd.args.iter().any(|a| a == "--json-schema"));
        assert!(!claude_cmd.args.iter().any(|a| a == "--print"));
        assert!(!claude_cmd.args.iter().any(|a| a == "--output-format"));

        let codex_driver = get_driver("codex").unwrap();
        let codex_cmd = codex_driver.build_session_args(&Default::default()).unwrap();
        assert!(!codex_cmd.args.iter().any(|a| a == "--json"));
        assert!(!codex_cmd.args.iter().any(|a| a == "--output-schema"));

        let gemini_driver = get_driver("gemini").unwrap();
        let gemini_cmd = gemini_driver.build_session_args(&Default::default()).unwrap();
        assert!(!gemini_cmd.args.iter().any(|a| a == "--output-format"));
        assert!(!gemini_cmd.args.iter().any(|a| a == "--prompt"));

        if let Some(dir) = gemini_cmd.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn integration_session_reuse_config_resolution() {
        let defaults = BTreeMap::new();
        let node = task_node("n2", "Step 2", "continue");

        // With session ID and persistence needed
        let config = resolve_agent_config(
            &defaults, "/work", "claude", &node,
            Some("sess-abc".to_string()), true, None,
        );
        assert_eq!(config.resume_session_id.as_deref(), Some("sess-abc"));
        assert!(!config.ephemeral_session);

        let driver = get_driver("claude").unwrap();
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(cmd.args.iter().any(|a| a == "--resume"));
        assert!(!cmd.args.iter().any(|a| a == "--no-session-persistence"));
    }

    #[test]
    fn integration_access_modes_all_agents() {
        let modes = [
            AccessMode::ReadOnly,
            AccessMode::Edit,
            AccessMode::Execute,
            AccessMode::Unrestricted,
        ];

        for mode in &modes {
            let config = AgentConfig {
                access_mode: mode.clone(),
                ..Default::default()
            };

            // All three agents should handle all access modes without error
            for agent_name in &["claude", "codex", "gemini"] {
                let driver = get_driver(agent_name).unwrap();
                let result = driver.build_session_args(&config);
                assert!(result.is_ok(), "{} failed with {:?}", agent_name, mode);

                let cmd = result.unwrap();
                // Verify the mode is reflected in args
                match (agent_name, mode) {
                    (&"claude", AccessMode::ReadOnly) => {
                        assert!(cmd.args.iter().any(|a| a == "plan"));
                    }
                    (&"codex", AccessMode::ReadOnly) => {
                        assert!(cmd.args.iter().any(|a| a == "read-only"));
                    }
                    (&"gemini", AccessMode::ReadOnly) => {
                        assert!(cmd.args.iter().any(|a| a == "plan"));
                    }
                    _ => {} // Other combos already tested individually
                }

                if let Some(dir) = cmd.temp_dir {
                    let _ = std::fs::remove_dir_all(dir);
                }
            }
        }
    }

    #[test]
    fn integration_per_node_cwd_override() {
        let defaults = BTreeMap::new();
        let mut node = task_node("n1", "Step", "prompt");

        // Without node cwd → uses workflow cwd
        let config = resolve_agent_config(&defaults, "/workspace", "claude", &node, None, false, None);
        assert_eq!(config.cwd, "/workspace");

        // With node cwd → uses node cwd
        node.cwd = Some("/other/project".to_string());
        let config = resolve_agent_config(&defaults, "/workspace", "claude", &node, None, false, None);
        assert_eq!(config.cwd, "/other/project");
    }

    #[test]
    fn integration_workflow_with_agent_defaults_roundtrip() {
        let json = json!({
            "version": 3,
            "goal": "test",
            "cwd": "/work",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "agentDefaults": {
                "claude": { "model": "sonnet", "maxTurns": 5, "accessMode": "edit" },
                "codex": { "model": "o3-mini", "reasoningLevel": "high" }
            },
            "nodes": [{
                "id": "n1",
                "name": "Step 1",
                "type": "task",
                "agent": "claude",
                "prompt": "hello",
                "agentConfig": {
                    "maxBudgetUsd": 1.5,
                    "toolToggles": { "webSearch": false }
                },
                "cwd": "/custom"
            }, {
                "id": "n2",
                "name": "Step 2",
                "type": "task",
                "agent": "codex",
                "prompt": "world",
                "continueSessionFrom": null
            }],
            "edges": [{ "id": "e1", "from": "n1", "to": "n2", "outcome": "success" }]
        });
        let result = normalize_workflow_value(json);
        assert!(result.is_ok(), "Workflow with agent config should parse: {:?}", result.err());
        let w = result.unwrap().workflow;

        // Agent defaults parsed
        assert_eq!(w.agent_defaults.len(), 2);
        assert_eq!(w.agent_defaults["claude"].model.as_deref(), Some("sonnet"));
        assert_eq!(w.agent_defaults["claude"].max_turns, Some(5));
        assert_eq!(w.agent_defaults["codex"].reasoning_level, Some(ReasoningLevel::High));

        // Node config parsed
        let n1 = &w.nodes[0];
        assert!(n1.agent_config.is_some());
        assert_eq!(n1.agent_config.as_ref().unwrap().base.max_budget_usd, Some(1.5));
        assert_eq!(n1.cwd.as_deref(), Some("/custom"));
    }

    #[test]
    fn build_session_persistence_set_empty_when_no_references() {
        let wf = WorkflowV3 {
            version: 3,
            name: None,
            goal: "test".to_string(),
            cwd: "/tmp".to_string(),
            use_orchestrator: false,
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits { max_total_steps: 10, max_visits_per_node: 5 },
            nodes: vec![task_node("n1", "Step 1", "prompt1")],
            edges: vec![],
            agent_defaults: BTreeMap::new(),
            ui: None,
        };

        let set = super::build_session_persistence_set(&wf);
        assert!(set.is_empty());
    }
}
