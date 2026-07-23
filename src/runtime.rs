use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use futures::{FutureExt, future::BoxFuture};
use regex::Regex;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value, json};
use tmux_tools_core::TmuxInvocation;
use tokio::{
    sync::{Mutex, Notify, broadcast, oneshot},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    driver::{self, AgentConfig, NodeOutcome},
    model::{
        self, ContextSource, NodeKind, ResponseFormat, SplitFailurePolicy, WorkflowEdge,
        WorkflowEdgeOutcome, WorkflowGraph, WorkflowNode, WorkflowV3, agent_name_for_node,
        evaluate_condition, get_nested_field,
    },
    storage::Database,
    util::{djb2, now_iso, slugify_filename},
};

use model::{DEFAULT_AGENT, MAX_PARALLEL_BATCH_CONCURRENT, max_node_retry_attempts};

const STAGNATION_WINDOW: usize = 3;

/// Bound for `abort_and_wait` when the run task never reaches `finalize_run` (panic or wedged tmux cleanup).
fn abort_and_wait_drain_timeout() -> Duration {
    if cfg!(test) {
        Duration::from_millis(250)
    } else {
        Duration::from_secs(600)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

impl RuntimeEvent {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            seq: None,
            data: Map::new(),
        }
    }

    pub fn with_seq(mut self, seq: u64) -> Self {
        self.seq = Some(seq);
        self
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CallFrameState {
    #[serde(default)]
    pub frame_id: String,
    pub call_node_id: String,
    pub call_node_name: String,
    pub subflow_name: String,
    pub exit_node_id: String,
    #[serde(default)]
    pub parent_last_output: String,
    #[serde(default)]
    pub parent_loop_counters: BTreeMap<String, u32>,
    #[serde(default)]
    pub parent_visit_counters: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_last_branch_origin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_last_branch_choice: Option<String>,
    #[serde(default)]
    pub parent_var_map: BTreeMap<String, String>,
    #[serde(default)]
    pub subflow_results: BTreeMap<String, NodeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub var_map: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub call_stack: Vec<CallFrameState>,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollectorBarrierKey {
    pub scope: String,
    pub collector_id: String,
    pub execution_epoch: u64,
}

impl CollectorBarrierKey {
    pub fn new(
        scope: impl Into<String>,
        collector_id: impl Into<String>,
        execution_epoch: u64,
    ) -> Self {
        Self {
            scope: scope.into(),
            collector_id: collector_id.into(),
            execution_epoch,
        }
    }

    pub fn from_cursor(cursor: &CursorState, collector_id: &str, execution_epoch: u64) -> Self {
        Self::new(
            collector_barrier_scope(cursor),
            collector_id,
            execution_epoch,
        )
    }
}

impl Serialize for CollectorBarrierKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CollectorBarrierKey", 3)?;
        state.serialize_field("scope", &self.scope)?;
        state.serialize_field("collectorId", &self.collector_id)?;
        state.serialize_field("executionEpoch", &self.execution_epoch)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CollectorBarrierKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct CollectorBarrierKeyFields {
            scope: String,
            collector_id: String,
            execution_epoch: u64,
        }

        let fields = CollectorBarrierKeyFields::deserialize(deserializer)?;
        Ok(Self::new(
            fields.scope,
            fields.collector_id,
            fields.execution_epoch,
        ))
    }
}

fn parse_legacy_collector_barrier_key(barrier_key: &str) -> CollectorBarrierKey {
    let (head, execution_epoch) = barrier_key
        .rsplit_once(':')
        .map(|(head, epoch)| (head, epoch.parse().unwrap_or(0)))
        .unwrap_or((barrier_key, 0));
    let (scope, collector_id) = head
        .split_once(':')
        .map(|(scope, collector_id)| (scope.to_string(), collector_id.to_string()))
        .unwrap_or_else(|| ("root".to_string(), head.to_string()));
    CollectorBarrierKey::new(scope, collector_id, execution_epoch)
}

mod collector_barriers_serde {
    use super::*;

    #[derive(Serialize)]
    struct CollectorBarrierEntrySer<'a> {
        scope: &'a str,
        #[serde(rename = "collectorId")]
        collector_id: &'a str,
        #[serde(rename = "executionEpoch")]
        execution_epoch: u64,
        #[serde(flatten)]
        state: &'a CollectorBarrierState,
    }

    #[derive(Deserialize)]
    struct CollectorBarrierEntryDe {
        scope: String,
        #[serde(rename = "collectorId")]
        collector_id: String,
        #[serde(rename = "executionEpoch")]
        execution_epoch: u64,
        #[serde(flatten)]
        state: CollectorBarrierState,
    }

    pub fn serialize<S>(
        map: &BTreeMap<CollectorBarrierKey, CollectorBarrierState>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries = map
            .iter()
            .map(|(key, state)| CollectorBarrierEntrySer {
                scope: &key.scope,
                collector_id: &key.collector_id,
                execution_epoch: key.execution_epoch,
                state,
            })
            .collect::<Vec<_>>();
        entries.serialize(serializer)
    }

    struct CollectorBarriersVisitor;

    impl<'de> Visitor<'de> for CollectorBarriersVisitor {
        type Value = BTreeMap<CollectorBarrierKey, CollectorBarrierState>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("collector barrier array or legacy string-keyed map")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut map = BTreeMap::new();
            while let Some(entry) = seq.next_element::<CollectorBarrierEntryDe>()? {
                map.insert(
                    CollectorBarrierKey::new(
                        entry.scope,
                        entry.collector_id,
                        entry.execution_epoch,
                    ),
                    entry.state,
                );
            }
            Ok(map)
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut barriers = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, CollectorBarrierState>()? {
                barriers.insert(parse_legacy_collector_barrier_key(&key), value);
            }
            Ok(barriers)
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<CollectorBarrierKey, CollectorBarrierState>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(CollectorBarriersVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CollectorBarrierState {
    #[serde(default)]
    pub required_inputs: BTreeSet<String>,
    #[serde(default)]
    pub arrivals: BTreeMap<String, CollectorInputStatus>,
    #[serde(default)]
    pub waiting_cursor_ids: Vec<String>,
    /// The first terminal arrival supplies cursor-scoped state when no live waiter survives.
    /// Its variable snapshot may be stale if another branch mutates globals after capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representative_snapshot: Option<CursorState>,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub batch_item_results: BTreeMap<String, BTreeMap<usize, Value>>,
    #[serde(default)]
    pub last_output: String,
    #[serde(default)]
    pub execution_epoch: u64,
    #[serde(default)]
    pub active_cursors: Vec<CursorState>,
    #[serde(default)]
    pub split_families: BTreeMap<String, SplitFamilyState>,
    #[serde(default, with = "collector_barriers_serde")]
    pub collector_barriers: BTreeMap<CollectorBarrierKey, CollectorBarrierState>,
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

pub(crate) const PANE_ALIAS_KEYS: [&str; 3] = ["paneId", "pane_id", "target"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneCandidate {
    pub(crate) key: String,
    pub(crate) target: String,
}

impl RuntimeCheckpoint {
    pub(crate) fn pane_candidates(&self) -> Vec<PaneCandidate> {
        let mut candidates = Vec::new();
        let trusted_targets = trusted_pane_targets(self);
        for (node_id, result) in &self.all_results {
            push_pane_candidate(
                &mut candidates,
                node_id,
                result.metadata.agent_session_id.as_deref(),
            );
            if let Some(value) = &result.parsed_output {
                push_output_pane_candidate(
                    &mut candidates,
                    &trusted_targets,
                    &self.run_id,
                    node_id,
                    pane_target_from_value(value),
                );
            }
            if let Ok(value) = serde_json::from_str::<Value>(&result.output) {
                push_output_pane_candidate(
                    &mut candidates,
                    &trusted_targets,
                    &self.run_id,
                    node_id,
                    pane_target_from_value(&value),
                );
            }
        }

        for execution in &self.execution_log.node_executions {
            push_pane_candidate(
                &mut candidates,
                &execution.node_id,
                execution.metadata.agent_session_id.as_deref(),
            );
        }

        candidates
    }
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
    PANE_ALIAS_KEYS
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedRun {
    #[serde(default = "new_stream_token")]
    pub stream_token: String,
    #[serde(skip)]
    pub tmux_invocation: Option<TmuxInvocation>,
    pub checkpoint: RuntimeCheckpoint,
    pub workflow: WorkflowV3,
}

pub fn new_stream_token() -> String {
    Uuid::new_v4().simple().to_string()
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
    pub name: String,
    pub binary: String,
    pub capabilities: AgentCapabilities,
    pub access_profiles: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RunControlError {
    #[error("run not found")]
    RunNotFound,
    #[error("run is in terminal state")]
    TerminalState,
    #[error("run is already actively executing")]
    AlreadyActive,
    #[error("node not found in workflow")]
    NodeNotFound,
    #[error("no active interaction for this run")]
    NoActiveInteraction,
    #[error("interaction session is stale")]
    StaleInteractionSession,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub(crate) trait NodeRunner: Send + Sync {
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

    fn run_node_with_interaction(
        &self,
        node: WorkflowNode,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
        previous_output: String,
        ctx: RuntimeContext,
        run_id: String,
        cursor_id: String,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        let _ = node;
        let _ = previous_output;
        let _ = cursor_id;
        self.run_with_interaction(agent, prompt, cwd, timeout_secs, config, ctx, run_id)
    }
}

#[derive(Debug)]
struct ApprovalDecision {
    approved: bool,
    user_input: String,
}

#[derive(Debug, Clone)]
struct ActiveRun {
    sender: broadcast::Sender<RuntimeEvent>,
    abort_token: CancellationToken,
    drained_token: CancellationToken,
    approval_sender: Arc<Mutex<Option<oneshot::Sender<ApprovalDecision>>>>,
    interaction_senders: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    active_panes: Arc<Mutex<BTreeMap<String, ActivePaneTarget>>>,
    active_pane_sequence: Arc<AtomicU64>,
    active_pane_notify: Arc<Notify>,
    owned_tmux_targets: Arc<Mutex<OwnedTmuxTargets>>,
}

#[derive(Debug, Default)]
struct OwnedTmuxTargets {
    pane_ids: HashSet<String>,
    session_names: HashSet<String>,
}

#[derive(Debug, Clone)]
struct ActivePaneTarget {
    target: String,
    session_name: Option<String>,
    sequence: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivePaneEntry {
    pub key: String,
    pub target: String,
    pub session_name: Option<String>,
    pub sequence: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RunRegistry {
    inner: Arc<Mutex<HashMap<String, ActiveRun>>>,
    checkpoint_persist_hashes: Arc<Mutex<HashMap<String, u32>>>,
}

pub(crate) fn active_pane_key(cursor_id: &str, node_id: &str) -> String {
    format!("{cursor_id}:{node_id}")
}

fn active_pane_key_matches_node(key: &str, node_id: &str) -> bool {
    key == node_id
        || key
            .strip_suffix(node_id)
            .is_some_and(|prefix| prefix.ends_with(':'))
}

impl RunRegistry {
    async fn register(&self, run_id: &str) -> bool {
        let mut guard = self.inner.lock().await;
        if guard.contains_key(run_id) {
            return false;
        }
        let (sender, _) = broadcast::channel(512);
        guard.insert(
            run_id.to_string(),
            ActiveRun {
                sender,
                abort_token: CancellationToken::new(),
                drained_token: CancellationToken::new(),
                approval_sender: Arc::new(Mutex::new(None)),
                interaction_senders: Arc::new(Mutex::new(HashMap::new())),
                active_panes: Arc::new(Mutex::new(BTreeMap::new())),
                active_pane_sequence: Arc::new(AtomicU64::new(0)),
                active_pane_notify: Arc::new(Notify::new()),
                owned_tmux_targets: Arc::new(Mutex::new(OwnedTmuxTargets::default())),
            },
        );
        true
    }

    #[cfg(test)]
    pub(crate) async fn register_test_run(&self, run_id: &str) {
        let _ = self.register(run_id).await;
    }

    #[cfg(test)]
    pub(crate) async fn send_test_event(&self, run_id: &str, event: RuntimeEvent) {
        self.send_event(run_id, event).await;
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
            active.abort_token.cancel();
        }
    }

    async fn abort_and_wait(&self, run_id: &str) {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return;
        };
        active.abort_token.cancel();
        if tokio::time::timeout(abort_and_wait_drain_timeout(), active.drained_token.cancelled())
            .await
            .is_err()
        {
            tracing::warn!(
                run_id = %run_id,
                timeout_secs = abort_and_wait_drain_timeout().as_secs(),
                "abort_and_wait drain timed out; force-clearing registry entry"
            );
            self.clear(run_id).await;
        }
    }

    async fn is_aborted(&self, run_id: &str) -> bool {
        self.inner
            .lock()
            .await
            .get(run_id)
            .map(|active| active.abort_token.is_cancelled())
            .unwrap_or(false)
    }

    pub(crate) async fn abort_signal(&self, run_id: &str) -> Option<CancellationToken> {
        self.inner
            .lock()
            .await
            .get(run_id)
            .map(|active| active.abort_token.clone())
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
        session_id: &str,
        sender: oneshot::Sender<String>,
    ) -> anyhow::Result<()> {
        let active = self
            .inner
            .lock()
            .await
            .get(run_id)
            .cloned()
            .context("run is not active")?;
        let mut interactions = active.interaction_senders.lock().await;
        anyhow::ensure!(
            !interactions.contains_key(session_id),
            "interaction already pending for this session"
        );
        interactions.insert(session_id.to_string(), sender);
        Ok(())
    }

    async fn clear_pending_interaction(&self, run_id: &str, session_id: &str) {
        if let Some(active) = self.inner.lock().await.get(run_id).cloned() {
            active.interaction_senders.lock().await.remove(session_id);
        }
    }

    pub(crate) async fn resolve_interaction(
        &self,
        run_id: &str,
        session_id: &str,
        response: String,
    ) -> Result<(), RunControlError> {
        let active = self
            .inner
            .lock()
            .await
            .get(run_id)
            .cloned()
            .ok_or(RunControlError::NoActiveInteraction)?;
        let sender = active.interaction_senders.lock().await.remove(session_id);
        let Some(sender) = sender else {
            return Err(RunControlError::StaleInteractionSession);
        };
        sender.send(response).map_err(|_| {
            RunControlError::Internal(anyhow::anyhow!("interaction receiver dropped"))
        })?;
        Ok(())
    }

    async fn last_persist_hash(&self, run_id: &str) -> Option<u32> {
        self.checkpoint_persist_hashes
            .lock()
            .await
            .get(run_id)
            .copied()
    }

    async fn record_persist_hash(&self, run_id: &str, hash: u32) {
        self.checkpoint_persist_hashes
            .lock()
            .await
            .insert(run_id.to_string(), hash);
    }

    async fn clear(&self, run_id: &str) {
        if let Some(active) = self.inner.lock().await.remove(run_id) {
            active.drained_token.cancel();
        }
        self.checkpoint_persist_hashes.lock().await.remove(run_id);
    }

    pub async fn active_run_ids(&self) -> HashSet<String> {
        self.inner.lock().await.keys().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) async fn set_active_pane(&self, run_id: &str, key: &str, target: &str) {
        self.set_active_pane_with_session(run_id, key, target, None)
            .await;
    }

    pub(crate) async fn set_active_pane_with_session(
        &self,
        run_id: &str,
        key: &str,
        target: &str,
        session_name: Option<String>,
    ) {
        if let Some(active) = self.inner.lock().await.get(run_id).cloned() {
            let mut panes = active.active_panes.lock().await;
            let sequence = active.active_pane_sequence.fetch_add(1, Ordering::SeqCst) + 1;
            panes.insert(
                key.to_string(),
                ActivePaneTarget {
                    target: target.to_string(),
                    session_name,
                    sequence,
                },
            );
            drop(panes);
            active.active_pane_notify.notify_waiters();
        }
    }

    pub(crate) async fn active_pane_notify(&self, run_id: &str) -> Option<Arc<Notify>> {
        self.inner
            .lock()
            .await
            .get(run_id)
            .map(|active| active.active_pane_notify.clone())
    }

    pub(crate) async fn register_owned_tmux_target(
        &self,
        run_id: &str,
        pane_id: &str,
        session_name: Option<&str>,
    ) {
        if let Some(active) = self.inner.lock().await.get(run_id).cloned() {
            let mut targets = active.owned_tmux_targets.lock().await;
            targets.pane_ids.insert(pane_id.to_string());
            if let Some(session_name) = session_name {
                targets.session_names.insert(session_name.to_string());
            }
        }
    }

    pub(crate) async fn owns_tmux_pane(&self, run_id: &str, pane_id: &str) -> bool {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return false;
        };
        active
            .owned_tmux_targets
            .lock()
            .await
            .pane_ids
            .contains(pane_id)
    }

    pub(crate) async fn owns_tmux_session(&self, run_id: &str, session_name: &str) -> bool {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return false;
        };
        active
            .owned_tmux_targets
            .lock()
            .await
            .session_names
            .contains(session_name)
    }

    pub(crate) async fn clear_active_pane(&self, run_id: &str, key: &str) {
        if let Some(active) = self.inner.lock().await.get(run_id).cloned() {
            active.active_panes.lock().await.remove(key);
        }
    }

    pub(crate) async fn clear_active_pane_target(&self, run_id: &str, target: &str) {
        if let Some(active) = self.inner.lock().await.get(run_id).cloned() {
            active
                .active_panes
                .lock()
                .await
                .retain(|_, pane_target| pane_target.target != target);
        }
    }

    pub(crate) async fn resolve_active_pane(&self, run_id: &str, pane: &str) -> Option<String> {
        let active = self.inner.lock().await.get(run_id).cloned()?;
        let panes = active.active_panes.lock().await;
        if let Some(target) = panes.get(pane) {
            return Some(target.target.clone());
        }
        if panes.values().any(|target| target.target == pane) {
            return Some(pane.to_string());
        }
        if matches!(pane, "active" | "current") {
            if panes.len() == 1 {
                return panes.values().next().map(|target| target.target.clone());
            }
            return None;
        }
        if let Some(target) = panes
            .iter()
            .filter(|(key, _)| active_pane_key_matches_node(key, pane))
            .max_by_key(|(_, target)| target.sequence)
            .map(|(_, target)| target.target.clone())
        {
            return Some(target);
        }
        None
    }

    pub(crate) async fn active_pane_entries(&self, run_id: &str) -> Vec<ActivePaneEntry> {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return Vec::new();
        };
        let panes = active.active_panes.lock().await;
        panes
            .iter()
            .map(|(key, entry)| ActivePaneEntry {
                key: key.clone(),
                target: entry.target.clone(),
                session_name: entry.session_name.clone(),
                sequence: entry.sequence,
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) async fn active_pane_targets(&self, run_id: &str) -> Vec<String> {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return Vec::new();
        };
        let panes = active.active_panes.lock().await;
        panes
            .values()
            .map(|entry| entry.target.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) async fn active_pane_cleanup_targets(
        &self,
        run_id: &str,
    ) -> Vec<crate::tmux_exec::PaneCleanupTarget> {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return Vec::new();
        };
        let panes = active.active_panes.lock().await;
        dedupe_cleanup_targets(panes.values())
    }

    pub(crate) async fn active_pane_cleanup_targets_except_keys(
        &self,
        run_id: &str,
        retained_keys: &HashSet<String>,
    ) -> Vec<crate::tmux_exec::PaneCleanupTarget> {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return Vec::new();
        };
        let panes = active.active_panes.lock().await;
        if retained_keys.is_empty() {
            return dedupe_cleanup_targets(panes.values());
        }

        let retained_pane_ids = panes
            .iter()
            .filter(|(pane_key, _)| {
                retained_keys
                    .iter()
                    .any(|key| active_pane_key_matches_node(pane_key, key))
            })
            .map(|(_, entry)| entry.target.clone())
            .collect::<BTreeSet<_>>();
        let retained_session_names = panes
            .iter()
            .filter(|(pane_key, _)| {
                retained_keys
                    .iter()
                    .any(|key| active_pane_key_matches_node(pane_key, key))
            })
            .filter_map(|(_, entry)| entry.session_name.clone())
            .collect::<BTreeSet<_>>();

        dedupe_cleanup_targets(panes.values().filter(|entry| {
            !retained_pane_ids.contains(&entry.target)
                && !entry
                    .session_name
                    .as_ref()
                    .is_some_and(|session_name| retained_session_names.contains(session_name))
        }))
    }

    #[cfg(test)]
    pub(crate) async fn active_pane_targets_for_keys(
        &self,
        run_id: &str,
        keys: &HashSet<String>,
    ) -> Vec<String> {
        let Some(active) = self.inner.lock().await.get(run_id).cloned() else {
            return Vec::new();
        };
        let panes = active.active_panes.lock().await;
        keys.iter()
            .flat_map(|key| {
                panes
                    .iter()
                    .filter(move |(pane_key, _)| active_pane_key_matches_node(pane_key, key))
                    .map(|(_, entry)| entry.target.clone())
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

fn dedupe_cleanup_targets<'a>(
    targets: impl IntoIterator<Item = &'a ActivePaneTarget>,
) -> Vec<crate::tmux_exec::PaneCleanupTarget> {
    let mut seen = BTreeSet::new();
    let mut cleanup_targets = Vec::new();
    for target in targets {
        let key = (target.session_name.clone(), target.target.clone());
        if seen.insert(key) {
            cleanup_targets.push(crate::tmux_exec::PaneCleanupTarget::new(
                target.target.clone(),
                target.session_name.clone(),
            ));
        }
    }
    cleanup_targets
}

#[derive(Clone)]
pub struct RuntimeContext {
    pub db: Database,
    pub registry: RunRegistry,
    runner: Arc<dyn NodeRunner>,
    pub run_invocation: Option<TmuxInvocation>,
    /// Test-only override for the tmux binary used when reconstructing legacy invocations without `run_as`.
    pub(crate) legacy_reaper_tmux_bin: Option<String>,
}

impl RuntimeContext {
    pub fn new(db: Database) -> Self {
        let runner: Arc<dyn NodeRunner> = Arc::new(crate::tmux_exec::TmuxNodeRunner::new());
        Self {
            db,
            registry: RunRegistry::default(),
            runner,
            run_invocation: None,
            legacy_reaper_tmux_bin: None,
        }
    }

    #[cfg(test)]
    fn with_runner(db: Database, runner: Arc<dyn NodeRunner>) -> Self {
        Self {
            db,
            registry: RunRegistry::default(),
            runner,
            run_invocation: None,
            legacy_reaper_tmux_bin: None,
        }
    }

    pub async fn start_run(
        &self,
        workflow: WorkflowV3,
        variable_overrides: BTreeMap<String, String>,
        start_node_id: Option<String>,
    ) -> Result<String, RunControlError> {
        let run_id = format!("run_{}", Uuid::now_v7());
        let checkpoint =
            build_initial_checkpoint(&workflow, &run_id, variable_overrides, start_node_id);
        let tmux_invocation = resolve_workflow_invocation(
            self.run_invocation.clone(),
            workflow.run_as.clone(),
            run_id.clone(),
        )
        .await
        .map_err(RunControlError::Internal)?
        .unwrap_or_default();
        if !self.registry.register(&run_id).await {
            return Err(RunControlError::AlreadyActive);
        }
        if let Err(error) = self
            .db
            .upsert_run(&PersistedRun {
                stream_token: new_stream_token(),
                tmux_invocation: Some(tmux_invocation.clone()),
                checkpoint: checkpoint.clone(),
                workflow: workflow.clone(),
            })
            .await
        {
            self.registry.clear(&run_id).await;
            return Err(RunControlError::Internal(error));
        }
        let ctx = RuntimeContext {
            run_invocation: Some(tmux_invocation),
            ..self.clone()
        };
        spawn_supervised_run(ctx, workflow, checkpoint, false);
        Ok(run_id)
    }

    pub async fn resume_run(&self, run_id: &str) -> Result<(), RunControlError> {
        let persisted = self
            .db
            .get_run(run_id)
            .await
            .map_err(RunControlError::Internal)?
            .ok_or(RunControlError::RunNotFound)?;
        if !matches!(
            persisted.checkpoint.status,
            RuntimeStatus::Running | RuntimeStatus::Paused
        ) {
            return Err(RunControlError::TerminalState);
        }
        if !self.registry.register(run_id).await {
            return Err(RunControlError::AlreadyActive);
        }
        let tmux_invocation = match load_or_resolve_run_tmux_invocation(
            &self.db,
            &persisted,
            self.run_invocation.clone(),
        )
        .await
        {
            Ok(invocation) => invocation,
            Err(error) => {
                self.registry.clear(run_id).await;
                return Err(RunControlError::Internal(error));
            }
        };
        let ctx = RuntimeContext {
            run_invocation: Some(tmux_invocation),
            ..self.clone()
        };
        spawn_supervised_run(ctx, persisted.workflow, persisted.checkpoint, true);
        Ok(())
    }

    pub async fn restart_from(
        &self,
        run_id: &str,
        node_id: &str,
    ) -> Result<String, RunControlError> {
        let persisted = self
            .db
            .get_run(run_id)
            .await
            .map_err(RunControlError::Internal)?
            .ok_or(RunControlError::RunNotFound)?;
        if !persisted
            .workflow
            .nodes
            .iter()
            .any(|node| node.id == node_id)
        {
            return Err(RunControlError::NodeNotFound);
        }
        self.registry.abort_and_wait(run_id).await;
        let persisted = self
            .db
            .get_run(run_id)
            .await
            .map_err(RunControlError::Internal)?
            .ok_or(RunControlError::RunNotFound)?;

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
            var_map: checkpoint.var_map.clone(),
            call_stack: Vec::new(),
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
        let tmux_invocation = resolve_workflow_invocation(
            self.run_invocation.clone(),
            persisted.workflow.run_as.clone(),
            new_run_id.clone(),
        )
        .await
        .map_err(RunControlError::Internal)?
        .unwrap_or_default();
        if !self.registry.register(&new_run_id).await {
            return Err(RunControlError::AlreadyActive);
        }
        if let Err(error) = self
            .db
            .upsert_run(&PersistedRun {
                stream_token: new_stream_token(),
                tmux_invocation: Some(tmux_invocation.clone()),
                checkpoint: checkpoint.clone(),
                workflow: persisted.workflow.clone(),
            })
            .await
        {
            self.registry.clear(&new_run_id).await;
            return Err(RunControlError::Internal(error));
        }
        if let Err(error) = self
            .db
            .mark_run_status(
                run_id,
                RuntimeStatus::Restarted,
                Some("restarted".to_string()),
            )
            .await
        {
            self.registry.clear(&new_run_id).await;
            return Err(RunControlError::Internal(error));
        }

        let ctx = RuntimeContext {
            run_invocation: Some(tmux_invocation),
            ..self.clone()
        };
        spawn_supervised_run(ctx, persisted.workflow, checkpoint, true);
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

    pub async fn dismiss_run(&self, run_id: &str) -> anyhow::Result<()> {
        self.db
            .mark_run_status(run_id, RuntimeStatus::Aborted, Some("aborted".to_string()))
            .await?;
        reap_run_tmux_sessions(self, run_id).await;
        Ok(())
    }

    pub async fn respond_interaction(
        &self,
        run_id: &str,
        session_id: &str,
        response: String,
    ) -> Result<(), RunControlError> {
        self.registry
            .resolve_interaction(run_id, session_id, response)
            .await
    }
}

pub fn available_agents() -> Vec<AgentSpec> {
    driver::all_drivers()
        .into_iter()
        .map(|d| AgentSpec {
            name: d.name().to_string(),
            binary: driver::agent_binary(d.name()).unwrap_or_else(|| d.name().to_string()),
            capabilities: d.capabilities(),
            access_profiles: driver::agent_access_profile_names(d.name()),
        })
        .collect()
}

pub fn find_agent(name: &str) -> Option<AgentSpec> {
    let drv = driver::get_driver(name)?;
    Some(AgentSpec {
        name: name.to_string(),
        binary: driver::agent_binary(name).unwrap_or_else(|| name.to_string()),
        capabilities: drv.capabilities(),
        access_profiles: driver::agent_access_profile_names(name),
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
#[cfg(test)]
fn resolve_agent_executable(agent_name: &str, search_paths: &[PathBuf]) -> anyhow::Result<PathBuf> {
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
    invocation: TmuxInvocation,
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

    let agent = node
        .agent
        .clone()
        .unwrap_or_else(|| DEFAULT_AGENT.to_string());
    let config =
        crate::model::resolve_agent_config(&BTreeMap::new(), cwd, &agent, node, None, false, None);
    let resolved_prompt = wrap_prompt_for_json(node, resolved_prompt, &None);
    let agent_name = agent.clone();
    let prompt_clone = resolved_prompt.clone();
    let cwd_string = cwd.to_string();
    let config_clone = config.clone();
    let preview_timeout = timeout_for_node(node);
    let mut result = tokio::task::spawn_blocking(move || {
        crate::tmux_exec::run_tmux_oneshot(
            &agent_name,
            &prompt_clone,
            &cwd_string,
            Some(&config_clone),
            invocation,
            None,
            preview_timeout,
        )
    })
    .await
    .context("join error in node preview")??;
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

struct BatchItemTaskResult {
    item_index: usize,
    item_value: Value,
    task_result: CursorTaskResult,
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
    all_results: Arc<BTreeMap<String, NodeResult>>,
    var_map: BTreeMap<String, String>,
    /// Constant-per-run data shared via Arc to avoid cloning on each dispatch.
    shared: Arc<RunConstantData>,
}

fn new_cursor_id() -> String {
    format!("cursor_{}", Uuid::now_v7())
}

fn new_call_frame_id() -> String {
    format!("frame_{}", Uuid::now_v7())
}

fn new_split_family_id() -> String {
    format!("family_{}", Uuid::now_v7())
}

fn merge_key_for_edge(edge: &WorkflowEdge) -> String {
    edge.label.clone().unwrap_or_else(|| edge.from.clone())
}

fn collector_required_inputs(graph: &WorkflowGraph<'_>, collector_id: &str) -> BTreeSet<String> {
    let mut required_inputs = BTreeSet::new();
    for edge in graph.inbound_for(collector_id) {
        let merge_key = merge_key_for_edge(edge);
        if !required_inputs.insert(merge_key.clone()) {
            tracing::warn!(
                collector_id = %collector_id,
                edge_id = %edge.id,
                merge_key = %merge_key,
                "duplicate collector input key in barrier configuration"
            );
        }
    }
    required_inputs
}

fn new_collector_barrier_state(
    graph: &WorkflowGraph<'_>,
    collector_id: &str,
) -> CollectorBarrierState {
    CollectorBarrierState {
        required_inputs: collector_required_inputs(graph, collector_id),
        arrivals: BTreeMap::new(),
        waiting_cursor_ids: Vec::new(),
        representative_snapshot: None,
        released: false,
    }
}

fn reset_released_collector_barrier(
    barrier: &mut CollectorBarrierState,
    graph: &WorkflowGraph<'_>,
    collector_id: &str,
) {
    if !barrier.released {
        return;
    }
    barrier.required_inputs = collector_required_inputs(graph, collector_id);
    barrier.arrivals.clear();
    barrier.waiting_cursor_ids.clear();
    barrier.representative_snapshot = None;
    barrier.released = false;
}

fn evict_idle_split_families(checkpoint: &mut RuntimeCheckpoint, allow_force_failed: bool) {
    let referenced_family_ids = checkpoint
        .active_cursors
        .iter()
        .flat_map(|cursor| cursor.split_family_ids.iter().cloned())
        .collect::<HashSet<_>>();
    checkpoint.split_families.retain(|family_id, family| {
        if family.force_failed && !allow_force_failed {
            return true;
        }
        referenced_family_ids.contains(family_id)
    });
}

fn collector_barrier_is_idle(
    checkpoint: &RuntimeCheckpoint,
    barrier_key: &CollectorBarrierKey,
    barrier: &CollectorBarrierState,
) -> bool {
    if !barrier.released {
        return false;
    }
    !checkpoint.active_cursors.iter().any(|cursor| {
        cursor.state == CursorRuntimeState::WaitingCollector
            && cursor.node_id == barrier_key.collector_id
            && CollectorBarrierKey::from_cursor(
                cursor,
                &barrier_key.collector_id,
                barrier_key.execution_epoch,
            ) == *barrier_key
    })
}

fn evict_idle_collector_barriers(checkpoint: &mut RuntimeCheckpoint) {
    let idle_keys = checkpoint
        .collector_barriers
        .iter()
        .filter(|(key, barrier)| collector_barrier_is_idle(checkpoint, key, barrier))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in idle_keys {
        checkpoint.collector_barriers.remove(&key);
    }
}

fn evict_idle_runtime_maps(checkpoint: &mut RuntimeCheckpoint) {
    evict_idle_split_families(checkpoint, false);
    evict_idle_collector_barriers(checkpoint);
}

fn insert_collector_arrival(
    barrier: &mut CollectorBarrierState,
    collector_id: &str,
    merge_key: String,
    arrival: CollectorInputStatus,
) {
    use std::collections::btree_map::Entry;

    match barrier.arrivals.entry(merge_key) {
        Entry::Vacant(entry) => {
            entry.insert(arrival);
        }
        Entry::Occupied(entry) => {
            tracing::warn!(
                collector_id = %collector_id,
                merge_key = %entry.key(),
                existing_edge_id = %entry.get().edge_id,
                duplicate_edge_id = %arrival.edge_id,
                "duplicate collector input arrival for merge key; ignoring duplicate"
            );
        }
    }
}

fn collector_barrier_scope(cursor: &CursorState) -> &str {
    if let Some(frame) = cursor.call_stack.last() {
        if frame.frame_id.is_empty() {
            frame.call_node_id.as_str()
        } else {
            frame.frame_id.as_str()
        }
    } else {
        "root"
    }
}

/// Collect active-scope node IDs referenced by pending `continue_session_from` edges so
/// those source nodes keep their panes alive for later continuation.
fn build_session_persistence_set(
    graph: &WorkflowGraph<'_>,
    completed_results: &BTreeMap<String, NodeResult>,
) -> HashSet<String> {
    graph
        .node_map
        .values()
        .filter_map(|node| {
            if completed_results.contains_key(&node.id) {
                None
            } else {
                node.continue_session_from.clone()
            }
        })
        .collect()
}

fn scoped_workflow_cwd(active_workflow: &WorkflowV3, root_cwd: &str) -> String {
    if active_workflow.cwd.is_empty() {
        root_cwd.to_string()
    } else {
        active_workflow.cwd.clone()
    }
}

fn is_runner_node_kind(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Task { .. }
            | NodeKind::Decide { .. }
            | NodeKind::Spawn { .. }
            | NodeKind::Send { .. }
            | NodeKind::Wait { .. }
            | NodeKind::Capture { .. }
            | NodeKind::Kill { .. }
            | NodeKind::RunAgent { .. }
    )
}

fn prompt_template_for_node(node: &WorkflowNode) -> &str {
    match &node.kind {
        NodeKind::RunAgent {
            run_agent_config, ..
        } => run_agent_config.prompt.as_deref().unwrap_or(&node.prompt),
        NodeKind::Send { send_config } => {
            if send_config.text.is_empty() {
                &node.prompt
            } else {
                send_config.text.as_str()
            }
        }
        NodeKind::Decide { decide_config } => decide_config.prompt.as_str(),
        NodeKind::Task { .. }
        | NodeKind::Approval
        | NodeKind::Split
        | NodeKind::Collector
        | NodeKind::ParallelBatch { .. }
        | NodeKind::Subflow { .. }
        | NodeKind::Call { .. }
        | NodeKind::Spawn { .. }
        | NodeKind::Wait { .. }
        | NodeKind::Capture { .. }
        | NodeKind::Kill { .. } => &node.prompt,
    }
}

fn timeout_for_node(node: &WorkflowNode) -> Option<u64> {
    match &node.kind {
        NodeKind::RunAgent {
            run_agent_config, ..
        } => run_agent_config.timeout.or(node.timeout),
        NodeKind::Wait { wait_config } => wait_config.timeout.or(node.timeout),
        NodeKind::Task { .. }
        | NodeKind::Approval
        | NodeKind::Split
        | NodeKind::Collector
        | NodeKind::Decide { .. }
        | NodeKind::ParallelBatch { .. }
        | NodeKind::Subflow { .. }
        | NodeKind::Call { .. }
        | NodeKind::Spawn { .. }
        | NodeKind::Send { .. }
        | NodeKind::Capture { .. }
        | NodeKind::Kill { .. } => node.timeout,
    }
}

fn build_inbound_source_map(graph: &WorkflowGraph) -> HashMap<String, Vec<String>> {
    graph
        .inbound
        .iter()
        .map(|(node_id, edges)| {
            (
                (*node_id).to_string(),
                edges
                    .iter()
                    .map(|edge| edge.from.clone())
                    .collect::<Vec<_>>(),
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
        batch_item_results: BTreeMap::new(),
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
            var_map: var_map.clone(),
            call_stack: Vec::new(),
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

fn rehydrate_checkpoint_for_execution(
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
) -> Option<PendingApproval> {
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
                var_map: checkpoint.var_map.clone(),
                call_stack: Vec::new(),
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
    checkpoint
        .active_cursors
        .retain(|cursor| !cursor.cancel_requested);
    for cursor in &mut checkpoint.active_cursors {
        if cursor.var_map.is_empty() && cursor.call_stack.is_empty() {
            cursor.var_map = checkpoint.var_map.clone();
        }
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
        return checkpoint.pending_approval.take();
    }
    update_checkpoint_summary(workflow, checkpoint);
    None
}

async fn drop_inconsistent_pending_approval(
    ctx: &RuntimeContext,
    checkpoint: &mut RuntimeCheckpoint,
    pending: PendingApproval,
) -> anyhow::Result<()> {
    tracing::warn!(
        run_id = %checkpoint.run_id,
        node_id = %pending.node_id,
        cursor_id = %pending.cursor_id,
        "dropping pending_approval due to inconsistent persisted state"
    );
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("workflow_warn")
            .with(
                "message",
                format!(
                    "Dropped pending approval on node \"{}\" due to inconsistent persisted state",
                    pending.node_name
                ),
            )
            .with("nodeId", pending.node_id.clone())
            .with("cursorId", pending.cursor_id.clone()),
    )
    .await?;
    checkpoint.pending_approval = None;
    Ok(())
}

fn cursor_has_own_queued_approval(
    queued_approvals: &[QueuedApproval],
    cursor_id: &str,
) -> bool {
    queued_approvals
        .iter()
        .any(|queued| queued.approval.cursor_id == cursor_id)
}

fn waiting_approval_cursor_matches_pending_fallback(
    queued_approvals: &[QueuedApproval],
    pending: &PendingApproval,
    cursor: &CursorState,
) -> bool {
    cursor.state == CursorRuntimeState::WaitingApproval
        && !pending.cursor_id.is_empty()
        && pending.cursor_id != cursor.cursor_id
        && pending.node_id == cursor.node_id
        && !cursor_has_own_queued_approval(queued_approvals, &cursor.cursor_id)
}

async fn drop_inconsistent_waiting_approval_cursors(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
) -> anyhow::Result<()> {
    let pending = checkpoint.pending_approval.clone();
    let queued = checkpoint.queued_approvals.clone();
    let mut dropped = Vec::new();
    checkpoint.active_cursors.retain(|cursor| {
        if cursor.state != CursorRuntimeState::WaitingApproval {
            return true;
        }
        let matches_pending = pending.as_ref().is_some_and(|pending| {
            pending.node_id == cursor.node_id
                && (pending.cursor_id.is_empty() || pending.cursor_id == cursor.cursor_id)
        });
        let matches_queued = queued.iter().any(|queued| {
            queued.approval.node_id == cursor.node_id
                && (queued.approval.cursor_id.is_empty()
                    || queued.approval.cursor_id == cursor.cursor_id)
        });
        let matches_pending_fallback = pending.as_ref().is_some_and(|pending| {
            waiting_approval_cursor_matches_pending_fallback(&queued, pending, cursor)
        });
        if matches_pending || matches_queued || matches_pending_fallback {
            return true;
        }
        dropped.push(cursor.clone());
        false
    });
    for cursor in dropped {
        let node_name = workflow
            .nodes
            .iter()
            .find(|node| node.id == cursor.node_id)
            .map(|node| node.name.clone())
            .unwrap_or_else(|| cursor.node_id.clone());
        tracing::warn!(
            run_id = %checkpoint.run_id,
            node_id = %cursor.node_id,
            cursor_id = %cursor.cursor_id,
            "dropping WaitingApproval cursor with no pending or queued approval"
        );
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_warn")
                .with(
                    "message",
                    format!(
                        "Dropped approval wait on node \"{}\" due to inconsistent persisted state",
                        node_name
                    ),
                )
                .with("nodeId", cursor.node_id.clone())
                .with("cursorId", cursor.cursor_id.clone()),
        )
        .await?;
    }
    if checkpoint.active_cursors.is_empty()
        && checkpoint.execution_log.terminal_reason.is_none()
        && !checkpoint.all_results.values().any(|result| result.success)
    {
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_error").with(
                "message",
                "Run could not continue after inconsistent approval cursor state was removed.",
            ),
        )
        .await?;
        checkpoint.status = RuntimeStatus::Failed;
        checkpoint.execution_log.terminal_reason = Some("failed".to_string());
    }
    Ok(())
}

async fn fail_run_on_idle_unschedulable_cursors(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    run_id: &str,
) -> anyhow::Result<bool> {
    let Some(cursor) = checkpoint
        .active_cursors
        .iter()
        .find(|cursor| cursor.state != CursorRuntimeState::WaitingCollector)
    else {
        return Ok(false);
    };
    let node_name = workflow
        .nodes
        .iter()
        .find(|node| node.id == cursor.node_id)
        .map(|node| node.name.clone())
        .unwrap_or_else(|| cursor.node_id.clone());
    emit_event(
        ctx,
        run_id,
        RuntimeEvent::new("workflow_error")
            .with("cursorId", cursor.cursor_id.clone())
            .with("nodeId", cursor.node_id.clone())
            .with(
                "message",
                format!(
                    "Scheduler stalled on cursor \"{}\" at node \"{}\" in state {:?}",
                    cursor.cursor_id, node_name, cursor.state
                ),
            ),
    )
    .await?;
    checkpoint.status = RuntimeStatus::Failed;
    checkpoint.execution_log.terminal_reason = Some("failed".to_string());
    checkpoint.active_cursors.clear();
    Ok(true)
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
    checkpoint.current_node_name =
        workflow_for_cursor(workflow, cursor)
            .ok()
            .and_then(|active_workflow| {
                active_workflow
                    .nodes
                    .iter()
                    .find(|node| node.id == cursor.node_id)
                    .map(|node| node.name.clone())
            });
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

fn workflow_for_cursor<'a>(
    root_workflow: &'a WorkflowV3,
    cursor: &CursorState,
) -> anyhow::Result<&'a WorkflowV3> {
    let Some(frame) = cursor.call_stack.last() else {
        return Ok(root_workflow);
    };
    root_workflow
        .subflows
        .get(&frame.subflow_name)
        .map(|workflow| workflow.as_ref())
        .with_context(|| {
            format!(
                "Subflow \"{}\" is missing from the run-local workflow catalog",
                frame.subflow_name
            )
        })
}

fn graph_for_cursor<'a>(
    root_workflow: &'a WorkflowV3,
    cursor: &CursorState,
) -> anyhow::Result<WorkflowGraph<'a>> {
    Ok(workflow_for_cursor(root_workflow, cursor)?.graph())
}

fn results_for_cursor<'a>(
    checkpoint: &'a RuntimeCheckpoint,
    cursor: &'a CursorState,
) -> &'a BTreeMap<String, NodeResult> {
    cursor
        .call_stack
        .last()
        .map(|frame| &frame.subflow_results)
        .unwrap_or(&checkpoint.all_results)
}

fn all_results_for_cursor(
    checkpoint: &RuntimeCheckpoint,
    cursor: &CursorState,
) -> BTreeMap<String, NodeResult> {
    results_for_cursor(checkpoint, cursor).clone()
}

fn collect_skip_regexes(
    workflow: &WorkflowV3,
    cache: &mut BTreeMap<String, Regex>,
) -> anyhow::Result<()> {
    for node in &workflow.nodes {
        let Some(skip) = &node.skip_condition else {
            continue;
        };
        if skip.kind != "regex" {
            continue;
        }
        let regex = Regex::new(&skip.value).with_context(|| {
            format!(
                "invalid skip condition regex on node \"{}\" ({})",
                node.name, node.id
            )
        })?;
        cache.insert(node.id.clone(), regex);
    }
    Ok(())
}

fn compile_skip_regex_cache(
    workflow: &WorkflowV3,
) -> anyhow::Result<BTreeMap<Option<String>, BTreeMap<String, Regex>>> {
    let mut cache = BTreeMap::new();
    let mut root_map = BTreeMap::new();
    collect_skip_regexes(workflow, &mut root_map)?;
    cache.insert(None, root_map);
    for (subflow_name, subflow) in &workflow.subflows {
        let mut subflow_map = BTreeMap::new();
        collect_skip_regexes(subflow, &mut subflow_map)?;
        cache.insert(Some(subflow_name.clone()), subflow_map);
    }
    Ok(cache)
}

fn skip_regex_scope_key(cursor: &CursorState) -> Option<String> {
    cursor
        .call_stack
        .last()
        .map(|frame| frame.subflow_name.clone())
}

fn skip_regex_cache_for_cursor<'a>(
    cache: &'a BTreeMap<Option<String>, BTreeMap<String, Regex>>,
    cursor: &CursorState,
) -> anyhow::Result<&'a BTreeMap<String, Regex>> {
    let scope = skip_regex_scope_key(cursor);
    cache.get(&scope).with_context(|| {
        format!(
            "missing compiled skip regex cache for scope {:?}",
            scope
        )
    })
}

fn var_map_for_cursor(
    checkpoint: &RuntimeCheckpoint,
    cursor: &CursorState,
) -> BTreeMap<String, String> {
    if cursor.var_map.is_empty() && cursor.call_stack.is_empty() {
        checkpoint.var_map.clone()
    } else {
        cursor.var_map.clone()
    }
}

fn insert_result_for_cursor_index(
    checkpoint: &mut RuntimeCheckpoint,
    cursor_index: usize,
    node_id: String,
    result: NodeResult,
) -> bool {
    if let Some(frame) = checkpoint.active_cursors[cursor_index]
        .call_stack
        .last_mut()
    {
        frame.subflow_results.insert(node_id, result);
        false
    } else {
        checkpoint.all_results.insert(node_id, result);
        true
    }
}

fn scoped_output_hash_key(cursor: &CursorState, node_id: &str) -> String {
    if cursor.call_stack.is_empty() {
        return node_id.to_string();
    }
    let mut parts = cursor
        .call_stack
        .iter()
        .map(|frame| frame.call_node_id.as_str())
        .collect::<Vec<_>>();
    parts.push(node_id);
    parts.join("::")
}

fn record_output_hash(
    output_hashes: &mut BTreeMap<String, Vec<u32>>,
    key: String,
    output: &str,
) -> bool {
    let hashes = output_hashes.entry(key).or_default();
    hashes.push(djb2(output));
    if hashes.len() > STAGNATION_WINDOW {
        hashes.drain(0..hashes.len() - STAGNATION_WINDOW);
    }
    hashes.len() == STAGNATION_WINDOW && hashes.windows(2).all(|pair| pair[0] == pair[1])
}

fn parallel_batch_checkpoint_key(cursor: &CursorState, node_id: &str) -> String {
    let mut parts = cursor
        .call_stack
        .iter()
        .map(|frame| frame.call_node_id.as_str())
        .collect::<Vec<_>>();
    parts.push(cursor.cursor_id.as_str());
    parts.push(node_id);
    parts.join("::")
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
    skip_regex_cache: &BTreeMap<String, Regex>,
) -> anyhow::Result<bool> {
    let Some(skip) = &node.skip_condition else {
        return Ok(false);
    };
    let source_text = if skip.source == "previous_output" {
        cursor.last_output.as_str()
    } else {
        results_for_cursor(checkpoint, cursor)
            .get(&skip.source)
            .map(|result| result.output.as_str())
            .unwrap_or("")
    };
    Ok(match skip.kind.as_str() {
        "contains" => source_text.contains(&skip.value),
        "not_contains" => !source_text.contains(&skip.value),
        "regex" => skip_regex_cache
            .get(&node.id)
            .with_context(|| {
                format!(
                    "missing compiled skip regex for node \"{}\" ({})",
                    node.name, node.id
                )
            })?
            .is_match(source_text),
        _ => false,
    })
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
            if matches!(&target_node.kind, NodeKind::Collector) {
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
    let run_id = checkpoint.run_id.clone();
    let run_inv = resolve_workflow_invocation(
        ctx.run_invocation.clone(),
        workflow.run_as.clone(),
        run_id.clone(),
    )
    .await?;
    let ctx = RuntimeContext {
        run_invocation: run_inv,
        ..ctx
    };

    if let Some(pending) = rehydrate_checkpoint_for_execution(&workflow, &mut checkpoint) {
        drop_inconsistent_pending_approval(&ctx, &mut checkpoint, pending).await?;
    }
    drop_inconsistent_waiting_approval_cursors(&ctx, &workflow, &mut checkpoint).await?;
    let skip_regex_cache = compile_skip_regex_cache(&workflow)?;
    let start_instant = std::time::Instant::now();
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
            restore_pending_approval(&ctx, &workflow, &mut checkpoint, &mut active_approval)
                .await?;
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
            kill_active_run_panes(&ctx, &run_id).await;
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
            &mut checkpoint,
            &mut active_approval,
            &skip_regex_cache,
        )
        .await?
        {
            changed = true;
            if checkpoint.execution_log.terminal_reason.is_some() {
                break;
            }
        }

        if checkpoint.execution_log.terminal_reason.is_some() {
            if matches!(checkpoint.status, RuntimeStatus::Aborted) {
                kill_active_run_panes(&ctx, &run_id).await;
            }
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
            .filter(|cursor| {
                cursor.state == CursorRuntimeState::Runnable && !cursor.cancel_requested
            })
            .map(|cursor| cursor.cursor_id.clone())
            .collect::<Vec<_>>();

        for cursor_id in dispatchable {
            let Some(cursor) = cursor_snapshot(&checkpoint, &cursor_id) else {
                continue;
            };
            let active_graph = graph_for_cursor(&workflow, &cursor)?;
            let Some(node) = active_graph
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
            if !is_runner_node_kind(&node.kind) {
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
            let active_workflow = workflow_for_cursor(&workflow, &dispatch_cursor)?;
            let all_results = Arc::new(all_results_for_cursor(&checkpoint, &dispatch_cursor));
            let var_map = var_map_for_cursor(&checkpoint, &dispatch_cursor);
            let session_persistence_nodes =
                build_session_persistence_set(&active_graph, all_results.as_ref());
            let run_ctx = CursorTaskExecutionContext {
                run_id: run_id.clone(),
                workflow_goal: active_workflow.goal.clone(),
                workflow_nodes_len: active_workflow.nodes.len(),
                cwd: scoped_workflow_cwd(active_workflow, &checkpoint.cwd),
                use_orchestrator: active_workflow.use_orchestrator,
                total_executed: checkpoint.total_executed,
                all_results,
                var_map,
                shared: Arc::new(RunConstantData {
                    inbound_map: build_inbound_source_map(&active_graph),
                    agent_defaults: active_workflow.agent_defaults.clone(),
                    session_persistence_nodes,
                }),
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
                wait_for_approval(&ctx, &workflow, &mut checkpoint, &mut active_approval).await?;
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

            if fail_run_on_idle_unschedulable_cursors(&ctx, &workflow, &mut checkpoint, &run_id)
                .await?
            {
                break;
            }
        }

        if active_approval.is_some() {
            let wait = active_approval
                .as_mut()
                .expect("approval state should exist");
            let mut wait_changed = false;
            tokio::select! {
                task = running_tasks.join_next() => {
                    if let Some(task) = task {
                        apply_join_result(&ctx, &workflow, &mut checkpoint, task).await?;
                        wait_changed = true;
                    }
                }
                decision = &mut wait.receiver => {
                    let decision = decision.ok();
                    if let Some(wait) = active_approval.take() {
                        handle_approval_resolution(&ctx, &workflow, &mut checkpoint, wait.cursor_id, decision).await?;
                        wait_changed = true;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
            if wait_changed {
                update_checkpoint_summary(&workflow, &mut checkpoint);
                persist_checkpoint(&ctx, &workflow, &mut checkpoint).await?;
            }
        } else {
            let mut wait_changed = false;
            tokio::select! {
                task = running_tasks.join_next() => {
                    if let Some(task) = task {
                        apply_join_result(&ctx, &workflow, &mut checkpoint, task).await?;
                        wait_changed = true;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
            if wait_changed {
                update_checkpoint_summary(&workflow, &mut checkpoint);
                persist_checkpoint(&ctx, &workflow, &mut checkpoint).await?;
            }
        }
    }

    finalize_run(&ctx, &workflow, checkpoint, start_instant.elapsed()).await
}

async fn execute_workflow_to_terminal(
    ctx: RuntimeContext,
    workflow: WorkflowV3,
    checkpoint: RuntimeCheckpoint,
    resumed: bool,
) {
    let run_id = checkpoint.run_id.clone();
    let start_instant = std::time::Instant::now();
    let fallback_checkpoint = checkpoint.clone();
    if let Err(error) = execute_workflow(ctx.clone(), workflow.clone(), checkpoint, resumed).await {
        if let Err(backstop_error) = fail_workflow_after_error(
            &ctx,
            workflow,
            fallback_checkpoint,
            &run_id,
            error,
            start_instant.elapsed(),
        )
        .await
        {
            eprintln!(
                "failed to persist terminal workflow error for run {}: {:#}",
                run_id, backstop_error
            );
        }
    }
}

async fn fail_workflow_after_error(
    ctx: &RuntimeContext,
    workflow: WorkflowV3,
    fallback_checkpoint: RuntimeCheckpoint,
    run_id: &str,
    error: anyhow::Error,
    duration: Duration,
) -> anyhow::Result<()> {
    let message = error.to_string();
    let _ = emit_event(
        ctx,
        run_id,
        RuntimeEvent::new("workflow_error").with("message", message),
    )
    .await;

    let persisted = ctx.db.get_run(run_id).await.ok().flatten();
    let (mut checkpoint, workflow) = persisted
        .map(|persisted| (persisted.checkpoint, persisted.workflow))
        .unwrap_or((fallback_checkpoint, workflow));
    checkpoint.status = RuntimeStatus::Failed;
    checkpoint.execution_log.terminal_reason = Some("failed".to_string());
    checkpoint.active_cursors.clear();
    checkpoint.pending_approval = None;
    checkpoint.queued_approvals.clear();
    finalize_run(ctx, &workflow, checkpoint, duration).await
}

fn spawn_supervised_run(
    ctx: RuntimeContext,
    workflow: WorkflowV3,
    checkpoint: RuntimeCheckpoint,
    resumed: bool,
) {
    let registry = ctx.registry.clone();
    let run_id = checkpoint.run_id.clone();
    tokio::spawn(async move {
        let handle = tokio::spawn(async move {
            execute_workflow_to_terminal(ctx, workflow, checkpoint, resumed).await;
        });
        if let Err(join_error) = handle.await {
            tracing::warn!(
                run_id = %run_id,
                panic = join_error.is_panic(),
                cancelled = join_error.is_cancelled(),
                "run task exited before finalize_run; clearing registry entry (DB status may remain Running)"
            );
            registry.clear(&run_id).await;
        }
    });
}

async fn process_immediate_cursors(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    active_approval: &mut Option<ActiveApprovalWait>,
    skip_regex_cache: &BTreeMap<Option<String>, BTreeMap<String, Regex>>,
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
        let active_workflow = workflow_for_cursor(workflow, &cursor)?;
        let scope_skip_cache = skip_regex_cache_for_cursor(skip_regex_cache, &cursor)?;
        let active_graph = active_workflow.graph();
        let Some(node) = active_graph
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

        match &node.kind {
            NodeKind::Task { .. }
            | NodeKind::Decide { .. }
            | NodeKind::Spawn { .. }
            | NodeKind::Send { .. }
            | NodeKind::Wait { .. }
            | NodeKind::Capture { .. }
            | NodeKind::Kill { .. }
            | NodeKind::RunAgent { .. } => {
                if should_skip_cursor_node(&node, &cursor, checkpoint, scope_skip_cache)? {
                    let Some(_) =
                        prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                    else {
                        return Ok(true);
                    };
                    handle_skipped_task(ctx, workflow, &active_graph, checkpoint, cursor_id, node)
                        .await?;
                    return Ok(true);
                }
            }
            NodeKind::Approval => {
                let Some(_) =
                    prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                else {
                    return Ok(true);
                };
                queue_approval(ctx, checkpoint, cursor_id, node).await?;
                return Ok(true);
            }
            NodeKind::Split => {
                let Some(_) =
                    prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                else {
                    return Ok(true);
                };
                handle_split_node(ctx, &active_graph, checkpoint, cursor_id, node).await?;
                return Ok(true);
            }
            NodeKind::Collector => {
                let Some(_) =
                    prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                else {
                    return Ok(true);
                };
                handle_collector_entry(ctx, workflow, &active_graph, checkpoint, cursor_id, node)
                    .await?;
                return Ok(true);
            }
            NodeKind::ParallelBatch { batch_config } => {
                let config = batch_config.clone();
                let Some(_) =
                    prepare_cursor_visit(ctx, workflow, checkpoint, &cursor_id, &node).await?
                else {
                    return Ok(true);
                };
                handle_parallel_batch_node(
                    ctx,
                    workflow,
                    active_workflow,
                    &active_graph,
                    checkpoint,
                    cursor_id,
                    node,
                    config,
                )
                .await?;
                return Ok(true);
            }
            NodeKind::Subflow { subflow_config } | NodeKind::Call { subflow_config } => {
                let config = subflow_config.clone();
                let Some(_) =
                    prepare_cursor_visit(ctx, active_workflow, checkpoint, &cursor_id, &node)
                        .await?
                else {
                    return Ok(true);
                };
                handle_subflow_node(
                    ctx,
                    workflow,
                    &active_graph,
                    checkpoint,
                    cursor_id,
                    node,
                    config,
                )
                .await?;
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
    checkpoint.last_branch_origin_id = checkpoint.active_cursors[index]
        .last_branch_origin_id
        .clone();
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

async fn resolve_workflow_invocation(
    existing: Option<TmuxInvocation>,
    run_as: Option<model::RunAsConfig>,
    run_id: String,
) -> anyhow::Result<Option<TmuxInvocation>> {
    resolve_workflow_invocation_with(
        existing,
        run_as,
        run_id,
        crate::tmux_exec::build_tmux_invocation,
    )
    .await
}

pub(crate) async fn load_or_resolve_run_tmux_invocation(
    db: &Database,
    persisted: &PersistedRun,
    existing: Option<TmuxInvocation>,
) -> anyhow::Result<TmuxInvocation> {
    if let Some(invocation) = persisted.tmux_invocation.clone() {
        return Ok(invocation);
    }

    let invocation = if persisted.workflow.run_as.is_some() {
        resolve_workflow_invocation(
            existing,
            persisted.workflow.run_as.clone(),
            persisted.checkpoint.run_id.clone(),
        )
        .await?
        .unwrap_or_default()
    } else if let Some(invocation) = existing {
        invocation
    } else {
        // Legacy rows with NULL invocation ran on the default tmux server (no -L).
        TmuxInvocation::default()
    };
    db.store_tmux_invocation_if_missing(&persisted.checkpoint.run_id, &invocation)
        .await?;
    Ok(invocation)
}

async fn resolve_workflow_invocation_with<F>(
    existing: Option<TmuxInvocation>,
    run_as: Option<model::RunAsConfig>,
    run_id: String,
    build: F,
) -> anyhow::Result<Option<TmuxInvocation>>
where
    F: FnOnce(&model::RunAsConfig, &str) -> TmuxInvocation + Send + 'static,
{
    if existing.is_some() {
        return Ok(existing);
    }
    let Some(run_as) = run_as else {
        return Ok(Some(crate::tmux_exec::run_scoped_tmux_invocation(&run_id)));
    };

    tokio::task::spawn_blocking(move || build(&run_as, &run_id))
        .await
        .context("tmux invocation resolver task panicked")
        .map(Some)
}

async fn handle_subflow_node(
    ctx: &RuntimeContext,
    root_workflow: &WorkflowV3,
    parent_graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
    config: model::SubflowConfig,
) -> anyhow::Result<()> {
    let Some(index) = find_cursor_index(checkpoint, &cursor_id) else {
        return Ok(());
    };
    let parent_cursor = checkpoint.active_cursors[index].clone();
    let subflow_name = config.workflow_name.trim().to_string();
    let subflow = root_workflow
        .subflows
        .get(&subflow_name)
        .map(|workflow| workflow.as_ref())
        .with_context(|| format!("subflow \"{}\" not found", subflow_name))?;
    let max_depth = config.max_depth.max(1) as usize;
    if parent_cursor.call_stack.len() + 1 > max_depth {
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_error")
                .with("cursorId", cursor_id.clone())
                .with("nodeId", node.id.clone())
                .with(
                    "message",
                    format!(
                        "Subflow \"{}\" exceeded maxDepth {} at call node \"{}\".",
                        subflow_name, max_depth, node.name
                    ),
                ),
        )
        .await?;
        checkpoint.status = RuntimeStatus::Failed;
        checkpoint.execution_log.terminal_reason = Some("failed".to_string());
        checkpoint.active_cursors.remove(index);
        return Ok(());
    }

    let exit_node_id = resolve_subflow_exit_node(&config, subflow)?;
    let parent_results = all_results_for_cursor(checkpoint, &parent_cursor);
    let parent_vars = var_map_for_cursor(checkpoint, &parent_cursor);
    let parent_inbound = build_inbound_source_map(parent_graph);
    let binding_context = TemplateRuntimeContext {
        current_node_id: &node.id,
        current_node: &node,
        all_results: &parent_results,
        last_output: &parent_cursor.last_output,
        var_map: &parent_vars,
        inbound_map: &parent_inbound,
        last_branch_origin_id: parent_cursor.last_branch_origin_id.as_deref(),
        last_branch_choice: parent_cursor.last_branch_choice.as_deref(),
    };

    let mut subflow_vars = BTreeMap::new();
    for variable in &subflow.variables {
        subflow_vars.insert(variable.name.clone(), variable.default.clone());
    }
    for input in &config.inputs {
        let value =
            resolve_input_binding_source(&input.source, &binding_context).with_context(|| {
                format!(
                    "subflow input \"{}\" references missing source \"{}\"",
                    input.name, input.source
                )
            })?;
        subflow_vars.insert(input.name.clone(), value);
    }

    let frame = CallFrameState {
        frame_id: new_call_frame_id(),
        call_node_id: node.id.clone(),
        call_node_name: node.name.clone(),
        subflow_name: subflow_name.clone(),
        exit_node_id: exit_node_id.clone(),
        parent_last_output: parent_cursor.last_output.clone(),
        parent_loop_counters: parent_cursor.loop_counters.clone(),
        parent_visit_counters: parent_cursor.visit_counters.clone(),
        parent_last_branch_origin_id: parent_cursor.last_branch_origin_id.clone(),
        parent_last_branch_choice: parent_cursor.last_branch_choice.clone(),
        parent_var_map: parent_vars,
        subflow_results: BTreeMap::new(),
    };

    let cursor = &mut checkpoint.active_cursors[index];
    cursor.call_stack.push(frame);
    cursor.node_id = subflow.entry_node_id.clone();
    cursor.incoming_edge_id = None;
    cursor.incoming_node_id = Some(node.id.clone());
    cursor.last_output.clear();
    cursor.loop_counters = BTreeMap::new();
    cursor.visit_counters = BTreeMap::new();
    cursor.var_map = subflow_vars;
    cursor.last_branch_origin_id = None;
    cursor.last_branch_choice = None;
    cursor.state = CursorRuntimeState::Runnable;

    record_transition_for_cursor(
        ctx,
        checkpoint,
        Some(cursor_id.clone()),
        node.id.clone(),
        Some(subflow.entry_node_id.clone()),
        "subflow",
        &subflow_name,
    )
    .await?;
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("subflow_start")
            .with("cursorId", cursor_id)
            .with("nodeId", node.id)
            .with("subflowName", subflow_name)
            .with("entryNodeId", subflow.entry_node_id.clone())
            .with("exitNodeId", exit_node_id),
    )
    .await?;
    Ok(())
}

fn resolve_subflow_exit_node(
    config: &model::SubflowConfig,
    subflow: &WorkflowV3,
) -> anyhow::Result<String> {
    if let Some(exit_node_id) = config
        .exit_node_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(exit_node_id.to_string());
    }

    let graph = subflow.graph();
    let terminal_nodes = subflow
        .nodes
        .iter()
        .filter(|node| graph.outgoing_for(&node.id).is_empty())
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    match terminal_nodes.as_slice() {
        [exit_node_id] => Ok(exit_node_id.clone()),
        _ => anyhow::bail!(
            "subflow \"{}\" must have exactly one terminal node when exitNodeId is omitted",
            config.workflow_name
        ),
    }
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

fn resolve_restore_approval_cursor<'a>(
    checkpoint: &'a RuntimeCheckpoint,
    pending: &PendingApproval,
) -> Result<Option<&'a CursorState>, Vec<String>> {
    if let Some(cursor) = checkpoint.active_cursors.iter().find(|cursor| {
        cursor.state == CursorRuntimeState::WaitingApproval
            && (pending.cursor_id.is_empty() || cursor.cursor_id == pending.cursor_id)
    }) {
        return Ok(Some(cursor));
    }
    if pending.cursor_id.is_empty() {
        return Ok(None);
    }
    let candidates: Vec<_> = checkpoint
        .active_cursors
        .iter()
        .filter(|cursor| {
            waiting_approval_cursor_matches_pending_fallback(
                &checkpoint.queued_approvals,
                pending,
                cursor,
            )
        })
        .collect();
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(Some(candidates[0])),
        _ => Err(candidates
            .iter()
            .map(|cursor| cursor.cursor_id.clone())
            .collect()),
    }
}

async fn restore_pending_approval(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    active_approval: &mut Option<ActiveApprovalWait>,
) -> anyhow::Result<()> {
    let Some(pending) = checkpoint.pending_approval.clone() else {
        return Ok(());
    };
    let bound_cursor = match resolve_restore_approval_cursor(checkpoint, &pending) {
        Ok(Some(cursor)) => Some(cursor.clone()),
        Ok(None) => None,
        Err(candidate_ids) => {
            tracing::warn!(
                run_id = %checkpoint.run_id,
                node_id = %pending.node_id,
                cursor_id = %pending.cursor_id,
                ?candidate_ids,
                "ambiguous pending_approval restore: multiple waiting cursors match node_id"
            );
            None
        }
    };
    let Some(cursor) = bound_cursor else {
        drop_inconsistent_pending_approval(ctx, checkpoint, pending).await?;
        return Ok(());
    };
    let node = workflow
        .nodes
        .iter()
        .find(|node| node.id == cursor.node_id);
    let node_id = cursor.node_id.clone();
    let node_name = node
        .map(|node| node.name.clone())
        .unwrap_or_else(|| pending.node_name.clone());
    let prompt = node
        .map(|node| node.prompt.clone())
        .unwrap_or_else(|| pending.prompt.clone());
    let (sender, receiver) = oneshot::channel();
    ctx.registry
        .set_pending_approval(&checkpoint.run_id, sender)
        .await?;
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("approval_required")
            .with("runId", checkpoint.run_id.clone())
            .with("cursorId", cursor.cursor_id.clone())
            .with("nodeId", node_id)
            .with("nodeName", node_name)
            .with("prompt", prompt)
            .with("lastOutput", cursor.last_output.clone()),
    )
    .await?;
    *active_approval = Some(ActiveApprovalWait {
        cursor_id: cursor.cursor_id,
        receiver,
    });
    Ok(())
}

async fn wait_for_approval(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    active_approval: &mut Option<ActiveApprovalWait>,
) -> anyhow::Result<()> {
    if let Some(wait) = active_approval.as_mut() {
        tokio::select! {
            decision = &mut wait.receiver => {
                let cursor_id = wait.cursor_id.clone();
                *active_approval = None;
                handle_approval_resolution(ctx, workflow, checkpoint, cursor_id, decision.ok()).await?;
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
    if let NodeKind::Decide { decide_config } = &node.kind {
        return run_decide_node(
            ctx,
            run_ctx,
            cursor,
            node.clone(),
            decide_config.clone(),
            iteration,
        )
        .await;
    }

    let prompt_template = prompt_template_for_node(&node);
    let mut resolved_prompt = resolve_template_vars(
        prompt_template,
        &TemplateRuntimeContext {
            current_node_id: &node.id,
            current_node: &node,
            all_results: run_ctx.all_results.as_ref(),
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
        let orchestrator_result = run_orchestrator_refinement(
            &ctx,
            &run_ctx.run_id,
            &run_ctx.workflow_goal,
            &node,
            &resolved_prompt,
            &cursor.last_output,
            run_ctx.total_executed.saturating_sub(1),
            run_ctx.workflow_nodes_len,
            &run_ctx.cwd,
        )
        .await;
        match orchestrator_result {
            Ok(orchestrator) if orchestrator.success && !orchestrator.output.is_empty() => {
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
            }
            Ok(orchestrator) => {
                emit_orchestrator_warn(
                    &ctx,
                    &run_ctx.run_id,
                    &cursor.cursor_id,
                    &node.id,
                    orchestrator.stderr,
                )
                .await?;
            }
            Err(error) => {
                emit_orchestrator_warn(
                    &ctx,
                    &run_ctx.run_id,
                    &cursor.cursor_id,
                    &node.id,
                    error.to_string(),
                )
                .await?;
            }
        }
    }

    let agent_name = agent_name_for_node(&node);
    let node_timeout = timeout_for_node(&node);

    // Session reuse: resolve resume_session_id from referenced node's result
    let resume_session_id = node.continue_session_from.as_ref().and_then(|source_id| {
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
    let max_attempts = max_node_retry_attempts(node.retry_count);
    let mut result = loop {
        attempts += 1;
        let step_result = ctx
            .runner
            .run_node_with_interaction(
                node.clone(),
                agent_name.clone(),
                resolved_prompt.clone(),
                run_ctx.cwd.clone(),
                node_timeout,
                Some(agent_config.clone()),
                cursor.last_output.clone(),
                ctx.clone(),
                run_ctx.run_id.clone(),
                cursor.cursor_id.clone(),
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

async fn run_decide_node(
    ctx: RuntimeContext,
    run_ctx: CursorTaskExecutionContext,
    cursor: CursorState,
    node: WorkflowNode,
    config: model::DecideConfig,
    iteration: u32,
) -> anyhow::Result<CursorTaskResult> {
    let template_context = TemplateRuntimeContext {
        current_node_id: &node.id,
        current_node: &node,
        all_results: run_ctx.all_results.as_ref(),
        last_output: &cursor.last_output,
        var_map: &run_ctx.var_map,
        inbound_map: &run_ctx.shared.inbound_map,
        last_branch_origin_id: cursor.last_branch_origin_id.as_deref(),
        last_branch_choice: cursor.last_branch_choice.as_deref(),
    };
    let bindings = resolve_decide_input_bindings(&config, &template_context)?;
    let resolved_prompt = render_decide_prompt(
        &config.prompt,
        &template_context,
        &bindings,
        &config.outcomes,
    );
    emit_event(
        &ctx,
        &run_ctx.run_id,
        RuntimeEvent::new("node_start")
            .with("cursorId", cursor.cursor_id.clone())
            .with("nodeId", node.id.clone())
            .with("nodeName", node.name.clone())
            .with("agent", "llm")
            .with("resolvedPrompt", resolved_prompt.clone())
            .with("iteration", iteration),
    )
    .await?;

    let start = std::time::Instant::now();
    let cwd = run_ctx.cwd.clone();
    let agent = DEFAULT_AGENT.to_string();
    let agent_config = AgentConfig {
        model: config
            .model
            .clone()
            .or_else(|| Some(model::default_decide_model())),
        ..AgentConfig::default()
    };
    let prompt_for_task = resolved_prompt.clone();
    let inv = ctx
        .run_invocation
        .clone()
        .context("tmux invocation missing for decide node")?;
    let agent_config_clone = agent_config.clone();
    let decide_timeout = timeout_for_node(&node);
    let abort_token = ctx.registry.abort_signal(&run_ctx.run_id).await;
    let interaction = abort_token.as_ref().map(|_| {
        crate::tmux_exec::InteractionEscalation::new(
            ctx.clone(),
            run_ctx.run_id.clone(),
            abort_token.clone(),
        )
    });
    let interaction_for_blocking = interaction.clone();
    let agent_join = tokio::task::spawn_blocking(move || {
        crate::tmux_exec::run_tmux_oneshot(
            &agent,
            &prompt_for_task,
            &cwd,
            Some(&agent_config_clone),
            inv,
            interaction_for_blocking.as_ref(),
            decide_timeout,
        )
    });
    let result = if let Some(abort_token) = abort_token {
        tokio::select! {
            joined = agent_join => joined.context("join error in decide node")??,
            _ = abort_token.cancelled() => NodeResult {
                success: false,
                output: String::new(),
                stderr: "Agent run aborted".to_string(),
                exit_code: -15,
                duration: format!("{:.1}", start.elapsed().as_secs_f64()),
                agent: "llm".to_string(),
                prompt: config.prompt.clone(),
                metadata: AgentExecutionMetadata {
                    error_type: Some("aborted".to_owned()),
                    ..Default::default()
                },
                resolved_prompt: Some(resolved_prompt.clone()),
                ..Default::default()
            },
        }
    } else {
        agent_join
            .await
            .context("join error in decide node")??
    };
    let duration = format!("{:.1}", start.elapsed().as_secs_f64());
    let result = apply_decide_outcome_to_agent_result(
        result,
        &node,
        &config,
        &bindings,
        &resolved_prompt,
        duration,
    );

    Ok(CursorTaskResult {
        cursor_id: cursor.cursor_id,
        node,
        result,
        resolved_prompt,
        refined_prompt: None,
        iteration,
        attempts: 1,
    })
}

fn resolve_decide_input_bindings(
    config: &model::DecideConfig,
    context: &TemplateRuntimeContext<'_>,
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut bindings = BTreeMap::new();
    for input in &config.inputs {
        let value = resolve_input_binding_source(&input.source, context).with_context(|| {
            format!(
                "decide input \"{}\" references missing source \"{}\"",
                input.name, input.source
            )
        })?;
        bindings.insert(input.name.clone(), value);
    }
    Ok(bindings)
}

fn resolve_input_binding_source(
    source: &str,
    context: &TemplateRuntimeContext<'_>,
) -> Option<String> {
    resolve_input_binding_value(source, context).map(|value| value_to_template_string(&value))
}

fn resolve_input_binding_value(
    source: &str,
    context: &TemplateRuntimeContext<'_>,
) -> Option<Value> {
    let source = source.trim();
    match source {
        "previous_output" => return Some(Value::String(context.last_output.to_string())),
        "branch_origin" => {
            return Some(Value::String(
                context
                    .last_branch_origin_id
                    .unwrap_or_default()
                    .to_string(),
            ));
        }
        "branch_choice" => {
            return Some(Value::String(
                context.last_branch_choice.unwrap_or_default().to_string(),
            ));
        }
        _ => {}
    }

    if let Some(var_name) = source.strip_prefix("var:") {
        return context.var_map.get(var_name).cloned().map(Value::String);
    }

    if let Some(rest) = source.strip_prefix("node:") {
        if let Some((node_id, field_path)) = rest.split_once(".parsedOutput.") {
            return context
                .all_results
                .get(node_id)
                .and_then(|result| result.parsed_output.as_ref())
                .and_then(|parsed| get_nested_field(parsed, field_path))
                .cloned();
        }
        if let Some(node_id) = rest.strip_suffix(".parsedOutput") {
            return context
                .all_results
                .get(node_id)
                .and_then(|result| result.parsed_output.clone());
        }
        if let Some((node_id, field_path)) = rest.split_once(".output.") {
            return context.all_results.get(node_id).and_then(|result| {
                serde_json::from_str::<Value>(&result.output)
                    .ok()
                    .and_then(|output| get_nested_field(&output, field_path).cloned())
            });
        }
        let node_id = rest.strip_suffix(".output").unwrap_or(rest);
        return context
            .all_results
            .get(node_id)
            .map(|result| Value::String(result.output.clone()));
    }

    context
        .var_map
        .get(source)
        .cloned()
        .map(Value::String)
        .or_else(|| {
            context
                .all_results
                .get(source)
                .map(|result| Value::String(result.output.clone()))
        })
}

fn render_decide_prompt(
    prompt: &str,
    context: &TemplateRuntimeContext<'_>,
    bindings: &BTreeMap<String, String>,
    outcomes: &[String],
) -> String {
    let mut var_map = context.var_map.clone();
    for (name, value) in bindings {
        var_map.insert(name.clone(), value.clone());
    }
    let binding_context = TemplateRuntimeContext {
        current_node_id: context.current_node_id,
        current_node: context.current_node,
        all_results: context.all_results,
        last_output: context.last_output,
        var_map: &var_map,
        inbound_map: context.inbound_map,
        last_branch_origin_id: context.last_branch_origin_id,
        last_branch_choice: context.last_branch_choice,
    };
    let mut rendered = resolve_template_vars(prompt, &binding_context);
    for (name, value) in bindings {
        rendered = rendered.replace(&format!("{{{{{}}}}}", name), value);
    }
    rendered.push_str(&format!(
        "\n\nChoose exactly one outcome. Emit a single JSON object in this exact shape: {{\"outcome\":\"<label>\"}}. The outcome value must exactly match one of: {}.",
        format_decide_outcomes_for_prompt(outcomes)
    ));

    let mut json_node = context.current_node.clone();
    json_node.response_format = Some(ResponseFormat::Json);
    json_node.output_schema = Some(decide_output_schema(outcomes));
    wrap_prompt_for_json(&json_node, rendered, &None)
}

fn format_decide_outcomes_for_prompt(outcomes: &[String]) -> String {
    outcomes
        .iter()
        .map(|outcome| serde_json::to_string(outcome).unwrap_or_else(|_| format!("\"{outcome}\"")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn decide_output_schema(outcomes: &[String]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "outcome": {
                "type": "string",
                "description": format!(
                    "The selected outcome. Must exactly match one of: {}",
                    format_decide_outcomes_for_prompt(outcomes)
                ),
            }
        },
        "required": ["outcome"],
        "additionalProperties": false,
    })
}

fn parse_decide_structured_response(node: &WorkflowNode, response: &str) -> Option<Value> {
    let mut json_node = node.clone();
    json_node.response_format = Some(ResponseFormat::Json);

    let mut result = NodeResult {
        output: response.to_string(),
        raw_output: Some(response.to_string()),
        ..Default::default()
    };
    parse_structured_output(&json_node, &mut result);
    result.parsed_output
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DecideOutcomeSelection {
    Matched(String),
    Failed(DecideOutcomeFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DecideOutcomeFailure {
    Unmatched,
    StructuredLabelMismatch,
    StructuredNonScalar { json_type: &'static str },
}

fn coerce_decide_outcome_scalar(outcome_value: &Value) -> Result<String, &'static str> {
    match outcome_value {
        Value::String(label) => Ok(label.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(_) => Err("boolean"),
        Value::Array(_) => Err("array"),
        Value::Object(_) => Err("object"),
        Value::Null => Err("null"),
    }
}

fn decide_outcome_failure_stderr(failure: DecideOutcomeFailure, outcomes: &[String]) -> String {
    let labels = outcomes.join(", ");
    match failure {
        DecideOutcomeFailure::Unmatched | DecideOutcomeFailure::StructuredLabelMismatch => {
            format!("LLM response did not match any decide outcome: {labels}")
        }
        DecideOutcomeFailure::StructuredNonScalar { json_type } => format!(
            "decide agent emitted a non-string outcome (got {json_type}); expected one of: {labels}"
        ),
    }
}

fn apply_decide_outcome_to_agent_result(
    mut result: NodeResult,
    node: &WorkflowNode,
    config: &model::DecideConfig,
    bindings: &BTreeMap<String, String>,
    resolved_prompt: &str,
    duration: String,
) -> NodeResult {
    result.duration = duration;
    result.resolved_prompt = Some(resolved_prompt.to_string());

    if !result.success {
        let response = result.output.clone();
        result.parsed_output = Some(json!({
            "outcome": None::<String>,
            "response": response,
            "inputs": bindings,
            "agent": DEFAULT_AGENT,
        }));
        return result;
    }

    let response = result.output.clone();
    let parsed_response = parse_decide_structured_response(node, &response);
    let selection =
        select_decide_outcome(&config.outcomes, &response, parsed_response.as_ref());
    match selection {
        DecideOutcomeSelection::Matched(selected) => {
            result.success = true;
            result.output = selected.clone();
            result.stderr.clear();
            result.exit_code = 0;
            result.parsed_output = Some(json!({
                "outcome": Some(selected),
                "response": response,
                "inputs": bindings,
                "agent": DEFAULT_AGENT,
            }));
        }
        DecideOutcomeSelection::Failed(failure) => {
            result.success = false;
            result.stderr = decide_outcome_failure_stderr(failure, &config.outcomes);
            result.exit_code = 1;
            result.parsed_output = Some(json!({
                "outcome": None::<String>,
                "response": response,
                "inputs": bindings,
                "agent": DEFAULT_AGENT,
            }));
        }
    }
    if result.raw_output.is_none() {
        result.raw_output = Some(response.clone());
    }
    result
}

fn select_decide_outcome(
    outcomes: &[String],
    response: &str,
    structured_output: Option<&Value>,
) -> DecideOutcomeSelection {
    if let Some(parsed) = structured_output {
        if let Some(outcome_value) = parsed.get("outcome") {
            return match coerce_decide_outcome_scalar(outcome_value) {
                Ok(outcome_label) => outcomes
                    .iter()
                    .find(|outcome| outcome.as_str() == outcome_label)
                    .cloned()
                    .map(DecideOutcomeSelection::Matched)
                    .unwrap_or(DecideOutcomeSelection::Failed(
                        DecideOutcomeFailure::StructuredLabelMismatch,
                    )),
                Err(json_type) => DecideOutcomeSelection::Failed(
                    DecideOutcomeFailure::StructuredNonScalar { json_type },
                ),
            };
        }
    }

    let trimmed = response
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .trim();
    if let Some(outcome) = outcomes.iter().find(|outcome| outcome.as_str() == trimmed) {
        return DecideOutcomeSelection::Matched(outcome.clone());
    }

    let mut matches = Vec::new();
    for outcome in outcomes {
        let pattern = format!(r"\b{}\b", regex::escape(outcome));
        let Ok(re) = Regex::new(&pattern) else {
            continue;
        };
        for mat in re.find_iter(response) {
            matches.push((mat.start(), outcome.len(), outcome.clone()));
        }
    }

    if matches.is_empty() {
        return DecideOutcomeSelection::Failed(DecideOutcomeFailure::Unmatched);
    }

    matches.sort_by(|(offset_a, len_a, label_a), (offset_b, len_b, label_b)| {
        offset_a
            .cmp(offset_b)
            .then_with(|| len_b.cmp(len_a))
            .then_with(|| label_a.cmp(label_b))
    });

    let (best_offset, best_len, best_label) = &matches[0];
    let tied_at_best = matches
        .iter()
        .filter(|(offset, len, _)| *offset == *best_offset && *len == *best_len)
        .collect::<Vec<_>>();
    if tied_at_best.len() > 1 {
        return DecideOutcomeSelection::Failed(DecideOutcomeFailure::Unmatched);
    }

    DecideOutcomeSelection::Matched(best_label.clone())
}

async fn apply_join_result(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
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
    let active_workflow = workflow_for_cursor(workflow, &checkpoint.active_cursors[index])?;
    let graph = active_workflow.graph();
    let output_hash_key =
        scoped_output_hash_key(&checkpoint.active_cursors[index], &task_result.node.id);
    checkpoint.active_cursors[index].state = CursorRuntimeState::Runnable;
    checkpoint.active_cursors[index].last_output = task_result.result.output.clone();
    if checkpoint.active_cursors[index].call_stack.is_empty() {
        checkpoint.last_output = task_result.result.output.clone();
    }
    insert_result_for_cursor_index(
        checkpoint,
        index,
        task_result.node.id.clone(),
        task_result.result.clone(),
    );

    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("node_done")
            .with("cursorId", task_result.cursor_id.clone())
            .with("nodeId", task_result.node.id.clone())
            .with("nodeName", task_result.node.name.clone())
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
                    if let (Some(base), Some(meta)) =
                        (event_result.as_object_mut(), meta_val.as_object())
                    {
                        base.extend(meta.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                }
                event_result
            }),
    )
    .await?;

    checkpoint
        .execution_log
        .node_executions
        .push(NodeExecutionLog {
            cursor_id: Some(task_result.cursor_id.clone()),
            node_id: task_result.node.id.clone(),
            node_name: task_result.node.name.clone(),
            node_type: task_result.node.node_type().as_str().to_string(),
            agent: agent_name_for_node(&task_result.node),
            original_prompt: prompt_template_for_node(&task_result.node).to_string(),
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

    if complete_subflow_if_at_exit(
        ctx,
        workflow,
        checkpoint,
        &task_result.cursor_id,
        &task_result.node,
        &task_result.result,
    )
    .await?
    {
        return Ok(());
    }

    if !task_result.result.success {
        if ctx.registry.is_aborted(&checkpoint.run_id).await
            || task_result
                .result
                .metadata
                .error_type
                .as_deref()
                .is_some_and(|error_type| error_type == "aborted")
        {
            checkpoint.status = RuntimeStatus::Aborted;
            checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
            checkpoint.active_cursors.clear();
            return Ok(());
        }
        let status = if task_result.result.exit_code == -2
            || task_result
                .result
                .metadata
                .error_type
                .as_deref()
                .is_some_and(|error_type| error_type == "timeout")
        {
            CursorTerminalStatus::Timeout
        } else {
            CursorTerminalStatus::Failure
        };
        handle_terminal_cursor_status(
            ctx,
            workflow,
            &graph,
            checkpoint,
            task_result.cursor_id,
            task_result.node,
            task_result.result,
            status,
        )
        .await?;
        return Ok(());
    }

    if record_output_hash(
        &mut checkpoint.output_hashes,
        output_hash_key,
        &task_result.result.output,
    ) {
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_error")
                .with("cursorId", task_result.cursor_id.clone())
                .with("nodeId", task_result.node.id.clone())
                .with(
                    "message",
                    format!(
                        "Node \"{}\" produced identical output {} times consecutively — aborting (stagnation detected).",
                        task_result.node.name, STAGNATION_WINDOW
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
        active_workflow,
        &graph,
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
            finish_cursor(
                ctx,
                workflow,
                checkpoint,
                &task_result.cursor_id,
                &task_result.node,
                &task_result.result,
            )
            .await?;
        }
    }

    Ok(())
}

async fn complete_subflow_if_at_exit(
    ctx: &RuntimeContext,
    root_workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: &str,
    node: &WorkflowNode,
    exit_result: &NodeResult,
) -> anyhow::Result<bool> {
    let Some(index) = find_cursor_index(checkpoint, cursor_id) else {
        return Ok(false);
    };
    let should_complete = checkpoint.active_cursors[index]
        .call_stack
        .last()
        .is_some_and(|frame| frame.exit_node_id == node.id);
    if !should_complete {
        return Ok(false);
    }

    let mut frame = checkpoint.active_cursors[index]
        .call_stack
        .pop()
        .expect("checked call frame exists");
    frame
        .subflow_results
        .entry(node.id.clone())
        .or_insert_with(|| exit_result.clone());

    let call_result = NodeResult {
        success: exit_result.success,
        output: exit_result.output.clone(),
        stderr: exit_result.stderr.clone(),
        exit_code: exit_result.exit_code,
        duration: exit_result.duration.clone(),
        agent: "subflow".to_string(),
        prompt: frame.subflow_name.clone(),
        raw_output: exit_result.raw_output.clone(),
        parsed_output: exit_result.parsed_output.clone(),
        parse_error: exit_result.parse_error.clone(),
        resolved_prompt: Some(format!("subflow:{}", frame.subflow_name)),
        stale: exit_result.stale,
        preserved_from_run_id: exit_result.preserved_from_run_id.clone(),
        metadata: exit_result.metadata.clone(),
    };

    {
        let cursor = &mut checkpoint.active_cursors[index];
        cursor.last_output = call_result.output.clone();
        cursor.loop_counters = frame.parent_loop_counters.clone();
        cursor.visit_counters = frame.parent_visit_counters.clone();
        cursor.var_map = frame.parent_var_map.clone();
        cursor.last_branch_origin_id = frame.parent_last_branch_origin_id.clone();
        cursor.last_branch_choice = frame.parent_last_branch_choice.clone();
        cursor.state = CursorRuntimeState::Runnable;
    }

    let parent_workflow = workflow_for_cursor(root_workflow, &checkpoint.active_cursors[index])?;
    let parent_graph = parent_workflow.graph();
    let call_node = parent_graph
        .node_map
        .get(frame.call_node_id.as_str())
        .map(|node| (*node).clone())
        .with_context(|| {
            format!(
                "call node \"{}\" missing while completing subflow \"{}\"",
                frame.call_node_id, frame.subflow_name
            )
        })?;

    insert_result_for_cursor_index(
        checkpoint,
        index,
        frame.call_node_id.clone(),
        call_result.clone(),
    );
    if checkpoint.active_cursors[index].call_stack.is_empty() {
        checkpoint.last_output = call_result.output.clone();
    }

    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("subflow_done")
            .with("cursorId", cursor_id.to_string())
            .with("nodeId", frame.call_node_id.clone())
            .with("subflowName", frame.subflow_name.clone())
            .with("exitNodeId", node.id.clone())
            .with("output", call_result.output.clone()),
    )
    .await?;
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("node_done")
            .with("cursorId", cursor_id.to_string())
            .with("nodeId", frame.call_node_id.clone())
            .with("nodeName", frame.call_node_name.clone())
            .with(
                "result",
                json!({
                    "success": call_result.success,
                    "output": call_result.output.clone(),
                    "stderr": call_result.stderr.clone(),
                    "exitCode": call_result.exit_code,
                    "duration": call_result.duration.clone(),
                    "nodeName": frame.call_node_name.clone(),
                    "resolvedPrompt": format!("subflow:{}", frame.subflow_name),
                    "parsedOutput": call_result.parsed_output.clone(),
                    "parseError": call_result.parse_error.clone(),
                }),
            ),
    )
    .await?;

    let iteration = frame
        .parent_loop_counters
        .get(&frame.call_node_id)
        .copied()
        .unwrap_or(1);
    checkpoint
        .execution_log
        .node_executions
        .push(NodeExecutionLog {
            cursor_id: Some(cursor_id.to_string()),
            node_id: frame.call_node_id.clone(),
            node_name: frame.call_node_name.clone(),
            node_type: call_node.node_type().as_str().to_string(),
            agent: "subflow".to_string(),
            original_prompt: call_node.prompt.clone(),
            resolved_prompt: format!("subflow:{}", frame.subflow_name),
            refined_prompt: None,
            output: call_result.output.clone(),
            stderr: call_result.stderr.clone(),
            exit_code: call_result.exit_code,
            success: call_result.success,
            duration: call_result.duration.clone(),
            iteration,
            attempts: 1,
            timestamp: now_iso(),
            metadata: call_result.metadata.clone(),
        });

    if !call_result.success {
        let status = if call_result.exit_code == -2 {
            CursorTerminalStatus::Timeout
        } else {
            CursorTerminalStatus::Failure
        };
        if Box::pin(complete_subflow_if_at_exit(
            ctx,
            root_workflow,
            checkpoint,
            cursor_id,
            &call_node,
            &call_result,
        ))
        .await?
        {
            return Ok(true);
        }
        handle_terminal_cursor_status(
            ctx,
            root_workflow,
            &parent_graph,
            checkpoint,
            cursor_id.to_string(),
            call_node,
            call_result,
            status,
        )
        .await?;
        return Ok(true);
    }

    let decision = select_next_decision(
        ctx,
        parent_workflow,
        &parent_graph,
        checkpoint,
        cursor_id,
        &call_node,
        &call_result,
        iteration,
    )
    .await?;
    let next_node_id = decision.next_edge.as_ref().map(|edge| edge.to.clone());
    record_transition_for_cursor(
        ctx,
        checkpoint,
        Some(cursor_id.to_string()),
        frame.call_node_id.clone(),
        next_node_id,
        &decision.control_type,
        &decision.reason,
    )
    .await?;

    if let Some(index) = find_cursor_index(checkpoint, cursor_id) {
        if let Some(edge) = decision.next_edge {
            let cursor = &mut checkpoint.active_cursors[index];
            cursor.node_id = edge.to.clone();
            cursor.incoming_edge_id = Some(edge.id.clone());
            cursor.incoming_node_id = Some(frame.call_node_id.clone());
            if decision.control_type == "branch" {
                cursor.last_branch_origin_id = Some(frame.call_node_id);
                cursor.last_branch_choice = edge.label.clone();
            }
            cursor.state = CursorRuntimeState::Runnable;
        } else {
            if Box::pin(complete_subflow_if_at_exit(
                ctx,
                root_workflow,
                checkpoint,
                cursor_id,
                &call_node,
                &call_result,
            ))
            .await?
            {
                return Ok(true);
            }
            checkpoint.active_cursors.remove(index);
        }
    }

    Ok(true)
}

async fn finish_cursor(
    ctx: &RuntimeContext,
    root_workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: &str,
    node: &WorkflowNode,
    result: &NodeResult,
) -> anyhow::Result<()> {
    if complete_subflow_if_at_exit(ctx, root_workflow, checkpoint, cursor_id, node, result).await? {
        return Ok(());
    }
    if let Some(index) = find_cursor_index(checkpoint, cursor_id) {
        checkpoint.active_cursors.remove(index);
    }
    evict_idle_runtime_maps(checkpoint);
    Ok(())
}

async fn handle_approval_resolution(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    decision: Option<ApprovalDecision>,
) -> anyhow::Result<()> {
    checkpoint.pending_approval = None;
    let Some(index) = find_cursor_index(checkpoint, &cursor_id) else {
        return Ok(());
    };
    let graph = graph_for_cursor(workflow, &checkpoint.active_cursors[index])?;
    let node_id = checkpoint.active_cursors[index].node_id.clone();
    let Some(node) = graph.node_map.get(node_id.as_str()).map(|n| (*n).clone()) else {
        return Ok(());
    };
    let approved = decision
        .as_ref()
        .map(|value| value.approved)
        .unwrap_or(false);
    let user_input = decision.map(|value| value.user_input).unwrap_or_default();
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
    insert_result_for_cursor_index(checkpoint, index, node.id.clone(), result.clone());
    checkpoint
        .execution_log
        .node_executions
        .push(NodeExecutionLog {
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
            .with("nodeName", node.name.clone())
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

    if complete_subflow_if_at_exit(ctx, workflow, checkpoint, &cursor_id, &node, &result).await? {
        return Ok(());
    }

    if approved {
        let next_edge = select_success_edge(&graph, &node.id);
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
            finish_cursor(ctx, workflow, checkpoint, &cursor_id, &node, &result).await?;
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
            workflow,
            &graph,
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

async fn handle_parallel_batch_node(
    ctx: &RuntimeContext,
    root_workflow: &WorkflowV3,
    workflow: &WorkflowV3,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
    config: model::BatchConfig,
) -> anyhow::Result<()> {
    let Some(parent_index) = find_cursor_index(checkpoint, &cursor_id) else {
        return Ok(());
    };
    let parent_cursor = checkpoint.active_cursors[parent_index].clone();
    let base_all_results = Arc::new(all_results_for_cursor(checkpoint, &parent_cursor));
    let base_var_map = var_map_for_cursor(checkpoint, &parent_cursor);
    let inbound_map = build_inbound_source_map(graph);
    let items = {
        let binding_context = TemplateRuntimeContext {
            current_node_id: &node.id,
            current_node: &node,
            all_results: base_all_results.as_ref(),
            last_output: &parent_cursor.last_output,
            var_map: &base_var_map,
            inbound_map: &inbound_map,
            last_branch_origin_id: parent_cursor.last_branch_origin_id.as_deref(),
            last_branch_choice: parent_cursor.last_branch_choice.as_deref(),
        };
        read_batch_items(&binding_context, &config.items_binding)?
    };
    let max_concurrent = config
        .max_concurrent
        .clamp(1, MAX_PARALLEL_BATCH_CONCURRENT) as usize;
    let body_node = graph
        .node_map
        .get(config.body_entry.as_str())
        .map(|node| (*node).clone())
        .with_context(|| {
            format!(
                "parallel_batch bodyEntry \"{}\" does not reference a known node",
                config.body_entry
            )
        })?;
    anyhow::ensure!(
        is_runner_node_kind(&body_node.kind),
        "parallel_batch bodyEntry \"{}\" is not an executable node",
        config.body_entry
    );
    let iteration = *parent_cursor.loop_counters.get(&node.id).unwrap_or(&1);
    let total_items = items.len();
    let batch_key = parallel_batch_checkpoint_key(&parent_cursor, &node.id);
    let mut item_results_by_index = checkpoint
        .batch_item_results
        .get(&batch_key)
        .cloned()
        .unwrap_or_default();
    item_results_by_index.retain(|item_index, result| {
        let Some(item_value) = items.get(*item_index) else {
            return false;
        };
        result.get("item") == Some(item_value)
    });
    if item_results_by_index.is_empty() {
        checkpoint.batch_item_results.remove(&batch_key);
    } else {
        checkpoint
            .batch_item_results
            .insert(batch_key.clone(), item_results_by_index.clone());
    }

    let session_persistence_nodes = build_session_persistence_set(graph, base_all_results.as_ref());
    let scoped_cwd = scoped_workflow_cwd(workflow, &checkpoint.cwd);
    let scoped_use_orchestrator = workflow.use_orchestrator;
    let shared = Arc::new(RunConstantData {
        inbound_map,
        agent_defaults: workflow.agent_defaults.clone(),
        session_persistence_nodes,
    });
    let mut pending_items = items
        .iter()
        .cloned()
        .enumerate()
        .filter(|(item_index, _)| !item_results_by_index.contains_key(item_index))
        .collect::<VecDeque<_>>();
    let mut running = JoinSet::new();
    let run_id = checkpoint.run_id.clone();
    let mut batch_aborted = ctx.registry.is_aborted(&run_id).await;
    let abort_token = ctx.registry.abort_signal(&run_id).await;

    while !batch_aborted && running.len() < max_concurrent {
        let Some((item_index, item_value)) = pending_items.pop_front() else {
            break;
        };
        spawn_batch_item_task(
            ctx,
            &mut running,
            workflow,
            checkpoint,
            shared.clone(),
            scoped_cwd.clone(),
            scoped_use_orchestrator,
            base_all_results.clone(),
            base_var_map.clone(),
            parent_cursor.clone(),
            node.clone(),
            body_node.clone(),
            config.item_var.clone(),
            item_index,
            item_value,
        )
        .await?;
    }

    while !batch_aborted {
        let joined = if let Some(ref abort_token) = abort_token {
            tokio::select! {
                joined = running.join_next() => joined,
                _ = abort_token.cancelled() => {
                    batch_aborted = true;
                    None
                }
            }
        } else {
            running.join_next().await
        };
        let Some(joined) = joined else {
            break;
        };
        let item_result = match joined {
            Ok(item_result) => item_result,
            Err(error) if error.is_cancelled() => continue,
            Err(error) => return Err(error.into()),
        };
        let item_result = record_batch_item_result(ctx, checkpoint, item_result).await?;
        let item_index = item_result
            .get("index")
            .and_then(Value::as_u64)
            .context("batch item result is missing index")? as usize;
        item_results_by_index.insert(item_index, item_result.clone());
        checkpoint
            .batch_item_results
            .entry(batch_key.clone())
            .or_default()
            .insert(item_index, item_result);
        persist_checkpoint(ctx, root_workflow, checkpoint).await?;

        if ctx.registry.is_aborted(&run_id).await {
            batch_aborted = true;
            break;
        }

        while running.len() < max_concurrent {
            if ctx.registry.is_aborted(&run_id).await {
                batch_aborted = true;
                break;
            }
            let Some((item_index, item_value)) = pending_items.pop_front() else {
                break;
            };
            spawn_batch_item_task(
                ctx,
                &mut running,
                workflow,
                checkpoint,
                shared.clone(),
                scoped_cwd.clone(),
                scoped_use_orchestrator,
                base_all_results.clone(),
                base_var_map.clone(),
                parent_cursor.clone(),
                node.clone(),
                body_node.clone(),
                config.item_var.clone(),
                item_index,
                item_value,
            )
            .await?;
        }
    }

    if !batch_aborted && ctx.registry.is_aborted(&run_id).await {
        batch_aborted = true;
    }
    if batch_aborted {
        running.abort_all();
        while running.join_next().await.is_some() {}
        for (item_index, item_value) in items.iter().cloned().enumerate() {
            item_results_by_index
                .entry(item_index)
                .or_insert_with(|| cancelled_batch_item_result(item_index, item_value));
        }
    }

    checkpoint.batch_item_results.remove(&batch_key);

    let item_results = item_results_by_index.values().cloned().collect::<Vec<_>>();
    let succeeded = item_results
        .iter()
        .filter(|result| result.get("success").and_then(Value::as_bool) == Some(true))
        .count();
    let cancelled = item_results
        .iter()
        .filter(|result| result.get("cancelled").and_then(Value::as_bool) == Some(true))
        .count();
    let parsed_output = json!({
        "items": item_results,
        "summary": {
            "total": total_items,
            "succeeded": succeeded,
            "failed": total_items.saturating_sub(succeeded),
            "cancelled": cancelled,
            "maxConcurrent": max_concurrent,
        }
    });
    if let Some(collector_var) = config
        .collector_var
        .as_deref()
        .filter(|collector_var| !collector_var.trim().is_empty())
    {
        let collected_items = serde_json::to_string(&parsed_output["items"])?;
        let write_global = checkpoint.active_cursors[parent_index]
            .call_stack
            .is_empty();
        checkpoint.active_cursors[parent_index]
            .var_map
            .insert(collector_var.to_string(), collected_items.clone());
        if write_global {
            checkpoint
                .var_map
                .insert(collector_var.to_string(), collected_items);
        }
    }
    let output =
        serde_json::to_string_pretty(&parsed_output).unwrap_or_else(|_| parsed_output.to_string());
    let success = !batch_aborted && succeeded == total_items;
    let result = NodeResult {
        success,
        output: output.clone(),
        stderr: if batch_aborted {
            "Workflow aborted by user.".to_string()
        } else if success {
            String::new()
        } else {
            format!(
                "{} of {} batch items failed",
                total_items - succeeded,
                total_items
            )
        },
        exit_code: if success { 0 } else { 1 },
        duration: "0".to_string(),
        agent: "system".to_string(),
        prompt: node.prompt.clone(),
        parsed_output: Some(parsed_output.clone()),
        resolved_prompt: Some(node.prompt.clone()),
        ..Default::default()
    };
    insert_result_for_cursor_index(checkpoint, parent_index, node.id.clone(), result.clone());
    if checkpoint.active_cursors[parent_index]
        .call_stack
        .is_empty()
    {
        checkpoint.last_output = output.clone();
    }
    checkpoint
        .execution_log
        .node_executions
        .push(NodeExecutionLog {
            cursor_id: Some(cursor_id.clone()),
            node_id: node.id.clone(),
            node_name: node.name.clone(),
            node_type: node.node_type().as_str().to_string(),
            agent: "system".to_string(),
            original_prompt: node.prompt.clone(),
            resolved_prompt: node.prompt.clone(),
            refined_prompt: None,
            output: output.clone(),
            stderr: result.stderr.clone(),
            exit_code: result.exit_code,
            success: result.success,
            duration: result.duration.clone(),
            iteration,
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
            .with("nodeName", node.name.clone())
            .with(
                "result",
                json!({
                    "success": result.success,
                    "output": result.output.clone(),
                    "stderr": result.stderr.clone(),
                    "exitCode": result.exit_code,
                    "duration": result.duration.clone(),
                    "nodeName": node.name.clone(),
                    "resolvedPrompt": node.prompt.clone(),
                    "parsedOutput": parsed_output,
                }),
            ),
    )
    .await?;

    if batch_aborted {
        emit_event(
            ctx,
            &run_id,
            RuntimeEvent::new("workflow_error").with("message", "Workflow aborted by user."),
        )
        .await?;
        checkpoint.status = RuntimeStatus::Aborted;
        checkpoint.execution_log.terminal_reason = Some("aborted".to_string());
        persist_checkpoint(ctx, root_workflow, checkpoint).await?;
        return Ok(());
    }

    if complete_subflow_if_at_exit(ctx, root_workflow, checkpoint, &cursor_id, &node, &result)
        .await?
    {
        return Ok(());
    }

    if !success {
        handle_terminal_cursor_status(
            ctx,
            root_workflow,
            graph,
            checkpoint,
            cursor_id,
            node,
            result,
            CursorTerminalStatus::Failure,
        )
        .await?;
        return Ok(());
    }

    let next_edge = select_success_edge(graph, &node.id);
    let next_node_id = next_edge.as_ref().map(|edge| edge.to.clone());
    record_transition_for_cursor(
        ctx,
        checkpoint,
        Some(cursor_id.clone()),
        node.id.clone(),
        next_node_id.clone(),
        "success",
        "parallel_batch complete",
    )
    .await?;
    if let Some(index) = find_cursor_index(checkpoint, &cursor_id) {
        if let Some(edge) = next_edge {
            checkpoint.active_cursors[index].node_id = edge.to.clone();
            checkpoint.active_cursors[index].incoming_edge_id = Some(edge.id.clone());
            checkpoint.active_cursors[index].incoming_node_id = Some(node.id);
            checkpoint.active_cursors[index].last_output = output;
            checkpoint.active_cursors[index].state = CursorRuntimeState::Runnable;
        } else {
            finish_cursor(ctx, root_workflow, checkpoint, &cursor_id, &node, &result).await?;
        }
    }
    Ok(())
}

async fn spawn_batch_item_task(
    ctx: &RuntimeContext,
    running: &mut JoinSet<BatchItemTaskResult>,
    workflow: &WorkflowV3,
    checkpoint: &RuntimeCheckpoint,
    shared: Arc<RunConstantData>,
    cwd: String,
    use_orchestrator: bool,
    base_all_results: Arc<BTreeMap<String, NodeResult>>,
    mut base_var_map: BTreeMap<String, String>,
    parent_cursor: CursorState,
    batch_node: WorkflowNode,
    body_node: WorkflowNode,
    item_var: String,
    item_index: usize,
    item_value: Value,
) -> anyhow::Result<()> {
    let child_cursor_id = new_cursor_id();
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("cursor_spawned")
            .with("cursorId", child_cursor_id.clone())
            .with("parentCursorId", parent_cursor.cursor_id.clone())
            .with("fromNodeId", batch_node.id.clone())
            .with("toNodeId", body_node.id.clone())
            .with("itemIndex", item_index)
            .with("item", item_value.clone()),
    )
    .await?;
    base_var_map.insert(item_var, value_to_template_string(&item_value));
    let fallback_resolved_prompt = wrap_prompt_for_json(
        &body_node,
        resolve_template_vars(
            prompt_template_for_node(&body_node),
            &TemplateRuntimeContext {
                current_node_id: &body_node.id,
                current_node: &body_node,
                all_results: base_all_results.as_ref(),
                last_output: &parent_cursor.last_output,
                var_map: &base_var_map,
                inbound_map: &shared.inbound_map,
                last_branch_origin_id: parent_cursor.last_branch_origin_id.as_deref(),
                last_branch_choice: parent_cursor.last_branch_choice.as_deref(),
            },
        ),
        &None,
    );
    let run_ctx = CursorTaskExecutionContext {
        run_id: checkpoint.run_id.clone(),
        workflow_goal: workflow.goal.clone(),
        workflow_nodes_len: workflow.nodes.len(),
        cwd,
        use_orchestrator,
        total_executed: checkpoint.total_executed,
        all_results: base_all_results,
        var_map: base_var_map.clone(),
        shared,
    };
    let child_cursor = CursorState {
        cursor_id: child_cursor_id,
        node_id: body_node.id.clone(),
        execution_epoch: checkpoint.execution_epoch,
        parent_cursor_id: Some(parent_cursor.cursor_id.clone()),
        incoming_edge_id: None,
        incoming_node_id: Some(batch_node.id),
        split_family_ids: parent_cursor.split_family_ids.clone(),
        last_output: parent_cursor.last_output,
        loop_counters: parent_cursor.loop_counters,
        visit_counters: parent_cursor.visit_counters,
        var_map: base_var_map,
        call_stack: parent_cursor.call_stack,
        last_branch_origin_id: parent_cursor.last_branch_origin_id,
        last_branch_choice: parent_cursor.last_branch_choice,
        cancel_requested: false,
        state: CursorRuntimeState::Running,
    };
    let task_ctx: RuntimeContext = (*ctx).clone();
    running.spawn(async move {
        let fallback_cursor_id = child_cursor.cursor_id.clone();
        let fallback_node = body_node.clone();
        let task_result = match std::panic::AssertUnwindSafe(run_cursor_task(
            task_ctx,
            run_ctx,
            child_cursor,
            body_node,
            1,
        ))
        .catch_unwind()
        .await
        {
            Ok(Ok(task_result)) => task_result,
            Ok(Err(error)) => failed_batch_item_task_result(
                fallback_cursor_id,
                fallback_node,
                fallback_resolved_prompt,
                error.to_string(),
            ),
            Err(panic) => failed_batch_item_task_result(
                fallback_cursor_id,
                fallback_node,
                fallback_resolved_prompt,
                format!(
                    "batch item task panicked: {}",
                    panic_payload_to_string(&*panic)
                ),
            ),
        };
        BatchItemTaskResult {
            item_index,
            item_value,
            task_result,
        }
    });
    Ok(())
}

fn failed_batch_item_task_result(
    cursor_id: String,
    node: WorkflowNode,
    resolved_prompt: String,
    stderr: String,
) -> CursorTaskResult {
    let prompt = prompt_template_for_node(&node).to_string();
    CursorTaskResult {
        cursor_id,
        node: node.clone(),
        result: NodeResult {
            success: false,
            output: String::new(),
            stderr,
            exit_code: 1,
            duration: "0".to_string(),
            agent: agent_name_for_node(&node),
            prompt,
            resolved_prompt: Some(resolved_prompt.clone()),
            ..Default::default()
        },
        resolved_prompt,
        refined_prompt: None,
        iteration: 1,
        attempts: 1,
    }
}

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

async fn record_batch_item_result(
    ctx: &RuntimeContext,
    checkpoint: &mut RuntimeCheckpoint,
    item_result: BatchItemTaskResult,
) -> anyhow::Result<Value> {
    let task_result = item_result.task_result;
    emit_event(
        ctx,
        &checkpoint.run_id,
        RuntimeEvent::new("node_done")
            .with("cursorId", task_result.cursor_id.clone())
            .with("nodeId", task_result.node.id.clone())
            .with("nodeName", task_result.node.name.clone())
            .with("itemIndex", item_result.item_index)
            .with(
                "result",
                json!({
                    "success": task_result.result.success,
                    "output": task_result.result.output.clone(),
                    "stderr": task_result.result.stderr.clone(),
                    "exitCode": task_result.result.exit_code,
                    "duration": task_result.result.duration.clone(),
                    "nodeName": task_result.node.name.clone(),
                    "resolvedPrompt": task_result.resolved_prompt.clone(),
                    "parsedOutput": task_result.result.parsed_output.clone(),
                    "parseError": task_result.result.parse_error.clone(),
                }),
            ),
    )
    .await?;
    checkpoint
        .execution_log
        .node_executions
        .push(NodeExecutionLog {
            cursor_id: Some(task_result.cursor_id.clone()),
            node_id: task_result.node.id.clone(),
            node_name: task_result.node.name.clone(),
            node_type: task_result.node.node_type().as_str().to_string(),
            agent: agent_name_for_node(&task_result.node),
            original_prompt: prompt_template_for_node(&task_result.node).to_string(),
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
    Ok(json!({
        "index": item_result.item_index,
        "item": item_result.item_value,
        "cursorId": task_result.cursor_id,
        "nodeId": task_result.node.id,
        "success": task_result.result.success,
        "output": task_result.result.output,
        "stderr": task_result.result.stderr,
        "exitCode": task_result.result.exit_code,
        "parsedOutput": task_result.result.parsed_output,
    }))
}

fn cancelled_batch_item_result(item_index: usize, item_value: Value) -> Value {
    json!({
        "index": item_index,
        "item": item_value,
        "cursorId": Value::Null,
        "nodeId": Value::Null,
        "success": false,
        "output": "",
        "stderr": "cancelled",
        "exitCode": -1,
        "parsedOutput": Value::Null,
        "cancelled": true,
    })
}

fn read_batch_items(
    context: &TemplateRuntimeContext<'_>,
    items_binding: &str,
) -> anyhow::Result<Vec<Value>> {
    let binding = items_binding.trim();
    if let Some(items) = context
        .all_results
        .get(binding)
        .and_then(|result| result.parsed_output.as_ref())
        .and_then(Value::as_array)
    {
        return Ok(items.clone());
    }

    let value = resolve_input_binding_value(binding, context)
        .with_context(|| format!("parallel_batch itemsBinding \"{}\" is not set", binding))?;
    match value {
        Value::Array(items) => Ok(items),
        Value::String(raw) => {
            let parsed = serde_json::from_str::<Value>(&raw).with_context(|| {
                format!(
                    "parallel_batch itemsBinding \"{}\" must contain a JSON array",
                    binding
                )
            })?;
            let Value::Array(items) = parsed else {
                anyhow::bail!(
                    "parallel_batch itemsBinding \"{}\" must contain a JSON array",
                    binding
                );
            };
            Ok(items)
        }
        _ => {
            anyhow::bail!(
                "parallel_batch itemsBinding \"{}\" must contain a JSON array",
                binding
            );
        }
    }
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
    let mut cursor = checkpoint.active_cursors.remove(index);
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
            force_failed: false,
        },
    );
    let split_result = NodeResult {
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
    };
    if cursor.call_stack.is_empty() {
        checkpoint.all_results.insert(node.id.clone(), split_result);
    } else if let Some(frame) = cursor.call_stack.last_mut() {
        frame.subflow_results.insert(node.id.clone(), split_result);
    }
    checkpoint
        .execution_log
        .node_executions
        .push(NodeExecutionLog {
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
            .with("nodeName", node.name.clone())
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
            edge.label
                .clone()
                .unwrap_or_else(|| edge.id.clone())
                .as_str(),
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
            var_map: cursor.var_map.clone(),
            call_stack: cursor.call_stack.clone(),
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
    root_workflow: &WorkflowV3,
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
    let merge_key = merge_key_for_edge(&incoming_edge);
    let cursor_results = all_results_for_cursor(checkpoint, &checkpoint.active_cursors[index]);
    let source_result = cursor_results
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
    let split_family_ids = checkpoint.active_cursors[index].split_family_ids.clone();
    let barrier_key = CollectorBarrierKey::from_cursor(
        &checkpoint.active_cursors[index],
        &node.id,
        checkpoint.execution_epoch,
    );
    let barrier = checkpoint
        .collector_barriers
        .entry(barrier_key)
        .or_insert_with(|| new_collector_barrier_state(graph, &node.id));
    reset_released_collector_barrier(barrier, graph, &node.id);
    insert_collector_arrival(
        barrier,
        &node.id,
        merge_key.clone(),
        CollectorInputStatus {
            source_node_id: incoming_edge.from.clone(),
            edge_id: incoming_edge.id.clone(),
            edge_label: incoming_edge.label.clone(),
            split_family_ids,
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
    release_collectors_if_ready(ctx, root_workflow, graph, checkpoint).await
}

async fn release_collectors_if_ready(
    ctx: &RuntimeContext,
    root_workflow: &WorkflowV3,
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
        let representative_snapshot = barrier.representative_snapshot.clone();
        let required_len = barrier.required_inputs.len();
        let collector_id = barrier_key.collector_id.clone();
        let Some(node) = graph
            .node_map
            .get(collector_id.as_str())
            .map(|n| (*n).clone())
        else {
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
        let collector_result = NodeResult {
            success: true,
            output: output.clone(),
            duration: "0".to_string(),
            agent: "system".to_string(),
            prompt: node.prompt.clone(),
            parsed_output: Some(parsed_output.clone()),
            resolved_prompt: Some(node.prompt.clone()),
            ..Default::default()
        };
        let mut representative_state = waiting_cursor_ids
            .iter()
            .find_map(|waiting_cursor_id| {
                checkpoint
                    .active_cursors
                    .iter()
                    .find(|cursor| cursor.cursor_id == *waiting_cursor_id)
                    .cloned()
            })
            .or(representative_snapshot);
        let representative_cursor_id = waiting_cursor_ids
            .iter()
            .find(|waiting_cursor_id| find_cursor_index(checkpoint, waiting_cursor_id).is_some())
            .cloned()
            .unwrap_or_else(new_cursor_id);
        if find_cursor_index(checkpoint, &representative_cursor_id).is_none() {
            if let Some(mut released_cursor) = representative_state.clone() {
                released_cursor.cursor_id = representative_cursor_id.clone();
                released_cursor.node_id = collector_id.clone();
                released_cursor.state = CursorRuntimeState::WaitingCollector;
                checkpoint.active_cursors.push(released_cursor);
            } else {
                tracing::warn!(
                    collector_id = %collector_id,
                    cursor_id = %representative_cursor_id,
                    "collector released without a live cursor or representative snapshot"
                );
            }
        }
        if let Some(representative_index) = find_cursor_index(checkpoint, &representative_cursor_id)
        {
            insert_result_for_cursor_index(
                checkpoint,
                representative_index,
                collector_id.clone(),
                collector_result.clone(),
            );
            representative_state = Some(checkpoint.active_cursors[representative_index].clone());
        } else {
            checkpoint
                .all_results
                .insert(collector_id.clone(), collector_result.clone());
        }
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

        for waiting_cursor_id in waiting_cursor_ids
            .iter()
            .filter(|waiting_cursor_id| *waiting_cursor_id != &representative_cursor_id)
        {
            if let Some(index) = find_cursor_index(checkpoint, waiting_cursor_id) {
                checkpoint.active_cursors.remove(index);
            }
        }

        checkpoint
            .execution_log
            .node_executions
            .push(NodeExecutionLog {
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

        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("node_done")
                .with("cursorId", representative_cursor_id.clone())
                .with("nodeId", collector_id.clone())
                .with("nodeName", node.name.clone())
                .with(
                    "result",
                    json!({
                        "success": true,
                        "output": output.clone(),
                        "stderr": "",
                        "exitCode": 0,
                        "duration": "0",
                        "nodeName": node.name.clone(),
                        "resolvedPrompt": node.prompt.clone(),
                        "parsedOutput": parsed_output.clone(),
                    }),
                ),
        )
        .await?;

        if Box::pin(complete_subflow_if_at_exit(
            ctx,
            root_workflow,
            checkpoint,
            &representative_cursor_id,
            &node,
            &collector_result,
        ))
        .await?
        {
            continue;
        }

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

        if let Some(edge) = next_edge {
            if let Some(index) = find_cursor_index(checkpoint, &representative_cursor_id) {
                checkpoint.active_cursors.remove(index);
            }
            let representative_state = representative_state.unwrap_or_else(|| CursorState {
                cursor_id: representative_cursor_id.clone(),
                node_id: collector_id.clone(),
                execution_epoch: checkpoint.execution_epoch,
                parent_cursor_id: None,
                incoming_edge_id: None,
                incoming_node_id: None,
                split_family_ids: Vec::new(),
                last_output: String::new(),
                loop_counters: BTreeMap::new(),
                visit_counters: BTreeMap::new(),
                var_map: checkpoint.var_map.clone(),
                call_stack: Vec::new(),
                last_branch_origin_id: None,
                last_branch_choice: None,
                cancel_requested: false,
                state: CursorRuntimeState::Runnable,
            });
            checkpoint.active_cursors.push(CursorState {
                cursor_id: representative_cursor_id,
                node_id: edge.to.clone(),
                execution_epoch: checkpoint.execution_epoch,
                parent_cursor_id: representative_state.parent_cursor_id,
                incoming_edge_id: Some(edge.id.clone()),
                incoming_node_id: Some(collector_id),
                split_family_ids: representative_state.split_family_ids,
                last_output: output,
                loop_counters: representative_state.loop_counters,
                visit_counters: representative_state.visit_counters,
                var_map: representative_state.var_map,
                call_stack: representative_state.call_stack,
                last_branch_origin_id: representative_state.last_branch_origin_id,
                last_branch_choice: representative_state.last_branch_choice,
                cancel_requested: false,
                state: CursorRuntimeState::Runnable,
            });
        } else {
            Box::pin(finish_cursor(
                ctx,
                root_workflow,
                checkpoint,
                &representative_cursor_id,
                &node,
                &collector_result,
            ))
            .await?;
        }
    }
    evict_idle_runtime_maps(checkpoint);
    Ok(())
}

async fn handle_terminal_cursor_status(
    ctx: &RuntimeContext,
    root_workflow: &WorkflowV3,
    graph: &WorkflowGraph<'_>,
    checkpoint: &mut RuntimeCheckpoint,
    cursor_id: String,
    node: WorkflowNode,
    result: NodeResult,
    status: CursorTerminalStatus,
) -> anyhow::Result<()> {
    if Box::pin(complete_subflow_if_at_exit(
        ctx,
        root_workflow,
        checkpoint,
        &cursor_id,
        &node,
        &result,
    ))
    .await?
    {
        return Ok(());
    }

    if let Some(index) = find_cursor_index(checkpoint, &cursor_id) {
        let cursor = checkpoint.active_cursors[index].clone();
        for target in nearest_collectors_for_node(graph, &node.id) {
            let barrier_key = CollectorBarrierKey::from_cursor(
                &cursor,
                &target.collector_id,
                checkpoint.execution_epoch,
            );
            let barrier = checkpoint
                .collector_barriers
                .entry(barrier_key)
                .or_insert_with(|| new_collector_barrier_state(graph, &target.collector_id));
            reset_released_collector_barrier(barrier, graph, &target.collector_id);
            if barrier.representative_snapshot.is_none() {
                barrier.representative_snapshot = Some(cursor.clone());
            }
            insert_collector_arrival(
                barrier,
                &target.collector_id,
                merge_key_for_edge(&target.inbound_edge),
                CollectorInputStatus {
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
                },
            );
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
            } else {
                tracing::warn!(
                    family_id = %family_id,
                    cursor_id = %cursor_id,
                    "cursor references missing split family"
                );
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
        release_collectors_if_ready(ctx, root_workflow, graph, checkpoint).await?;
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

    if matches!(&node.kind, NodeKind::Decide { .. }) {
        let chosen_label = result.output.trim();
        let chosen = branch_edges
            .iter()
            .find(|edge| edge.label.as_deref() == Some(chosen_label))
            .map(|edge| (*edge).clone());
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
                    .with("chosenLabel", chosen_label.to_string()),
            )
            .await?;
            checkpoint.execution_log.decisions.push(DecisionLog {
                kind: "decide".to_string(),
                node_id: node.id.clone(),
                chosen_branch: Some(edge.branch_id.clone().unwrap_or_else(|| edge.id.clone())),
                chosen_label: Some(chosen_label.to_string()),
                verdict: None,
                duration: Some(result.duration.clone()),
                deterministic: false,
                raw_request: result.resolved_prompt.clone(),
                raw_response: result.raw_output.clone(),
                timestamp: now_iso(),
            });
            return Ok(NextDecision {
                next_edge: Some(edge.clone()),
                control_type: "branch".to_string(),
                reason: chosen_label.to_string(),
            });
        }
        emit_event(
            ctx,
            &checkpoint.run_id,
            RuntimeEvent::new("workflow_error")
                .with("cursorId", cursor_id.to_string())
                .with("nodeId", node.id.clone())
                .with(
                    "message",
                    format!(
                        "decide node \"{}\" selected outcome \"{}\" with no matching branch edge",
                        node.name, chosen_label
                    ),
                ),
        )
        .await?;
        return Ok(NextDecision {
            next_edge: default_success,
            control_type: "success".to_string(),
            reason: format!(
                "decide outcome \"{}\" has no matching branch edge",
                chosen_label
            ),
        });
    }

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
            if loop_exit.is_none() {
                emit_event(
                    ctx,
                    &checkpoint.run_id,
                    RuntimeEvent::new("workflow_error")
                        .with("cursorId", cursor_id.to_string())
                        .with("nodeId", node.id.clone())
                        .with(
                            "message",
                            format!(
                                "Loop node \"{}\" reached max iterations ({}) with no loop_exit edge.",
                                node.name, max_iterations
                            ),
                        ),
                )
                .await?;
                checkpoint.status = RuntimeStatus::Failed;
                checkpoint.execution_log.terminal_reason = Some("failed".to_string());
            }
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
                next_edge: if matched {
                    Some(loop_continue)
                } else {
                    loop_exit
                },
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
        if chosen.is_none() && workflow.use_orchestrator {
            let invocation = ctx
                .run_invocation
                .clone()
                .context("tmux invocation missing for orchestrator branch")?;
            let orchestration = run_orchestrator_branch(
                &workflow.goal,
                node,
                &result.output,
                &branch_edges,
                &scoped_workflow_cwd(workflow, &checkpoint.cwd),
                invocation,
            )
            .await?;
            let chosen_id = orchestration
                .output
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
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
    evict_idle_split_families(&mut checkpoint, true);
    evict_idle_collector_barriers(&mut checkpoint);
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

    let log_id = build_log_id(&checkpoint.execution_log.workflow_name, &run_id);
    let completed_results = checkpoint.all_results.clone();
    let persistence_result: anyhow::Result<()> = async {
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
        if !ctx.db.update_run_checkpoint(&checkpoint).await? {
            ctx.db
                .upsert_run(&PersistedRun {
                    stream_token: new_stream_token(),
                    tmux_invocation: ctx.run_invocation.clone(),
                    checkpoint: checkpoint.clone(),
                    workflow: workflow.clone(),
                })
                .await?;
        }
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
        Ok(())
    }
    .await;

    cleanup_terminal_active_panes(ctx, &run_id, workflow, &completed_results).await;
    ctx.registry.clear(&run_id).await;
    persistence_result
}

async fn run_pane_cleanup(
    run_invocation: Option<TmuxInvocation>,
    targets: Vec<crate::tmux_exec::PaneCleanupTarget>,
) -> Result<Vec<crate::tmux_exec::PaneCleanupVerdict>, tokio::task::JoinError> {
    let inv = run_invocation;
    tokio::task::spawn_blocking(move || {
        if let Some(inv) = inv {
            tmux_tools_core::with_invocation(inv, || crate::tmux_exec::cleanup_panes(&targets))
        } else {
            crate::tmux_exec::cleanup_panes(&targets)
        }
    })
    .await
}

async fn cleanup_terminal_active_panes(
    ctx: &RuntimeContext,
    run_id: &str,
    workflow: &WorkflowV3,
    completed_results: &BTreeMap<String, NodeResult>,
) {
    let pending_continuation_keys =
        build_session_persistence_set(&workflow.graph(), completed_results);
    if !pending_continuation_keys.is_empty() {
        tracing::warn!(
            run_id = %run_id,
            pending_continuation_keys = ?pending_continuation_keys,
            "terminal cleanup encountered unconsumed continuation panes; cleaning up unconditionally"
        );
    }
    let targets = ctx
        .registry
        .active_pane_cleanup_targets_except_keys(run_id, &HashSet::new())
        .await;
    if targets.is_empty() {
        return;
    }

    let cleanup_verdicts = run_pane_cleanup(ctx.run_invocation.clone(), targets).await;
    let cleanup_verdicts = match cleanup_verdicts {
        Ok(verdicts) => verdicts,
        Err(error) => {
            tracing::warn!(
                run_id = %run_id,
                error = %error,
                "Terminal tmux cleanup task failed; retaining session registrations"
            );
            return;
        }
    };

    for verdict in cleanup_verdicts.iter().filter(|verdict| !verdict.absent) {
        if let Some(session_name) = verdict.target.session_name.as_deref() {
            tracing::warn!(
                run_id = %run_id,
                session_name = %session_name,
                "Terminal tmux cleanup could not confirm session absence; retaining registration"
            );
        } else {
            tracing::warn!(
                run_id = %run_id,
                pane_id = %verdict.target.pane_id,
                "Terminal tmux cleanup could not confirm pane absence"
            );
        }
    }

    let killed_sessions = cleanup_verdicts
        .into_iter()
        .filter(|verdict| verdict.absent)
        .filter_map(|verdict| verdict.target.session_name)
        .collect::<BTreeSet<_>>();

    if killed_sessions.is_empty() {
        return;
    }

    if let Err(error) = ctx.db.remove_tmux_sessions(run_id, &killed_sessions).await {
        tracing::warn!(
            run_id = %run_id,
            session_names = ?killed_sessions,
            error = %error,
            "Terminal tmux sessions are absent but their registrations remain"
        );
    }
}

pub(crate) async fn register_tmux_session(
    db: &Database,
    run_id: &str,
    session_name: &str,
) -> anyhow::Result<bool> {
    db.register_tmux_session(run_id, session_name).await
}

fn tmux_invocation_key(invocation: &TmuxInvocation) -> (Vec<String>, Option<String>, String) {
    (
        invocation.prefix.clone(),
        invocation.socket.clone(),
        invocation.tmux_bin.clone(),
    )
}

fn legacy_reaper_default_invocation(ctx: &RuntimeContext) -> TmuxInvocation {
    match ctx.legacy_reaper_tmux_bin.as_deref() {
        Some(tmux_bin) => TmuxInvocation {
            tmux_bin: tmux_bin.to_string(),
            ..TmuxInvocation::default()
        },
        None => TmuxInvocation::default(),
    }
}

pub async fn reap_stale_tmux_sessions(ctx: &RuntimeContext) {
    reap_tmux_sessions(ctx, None).await;
}

async fn reap_run_tmux_sessions(ctx: &RuntimeContext, run_id: &str) {
    reap_tmux_sessions(ctx, Some(run_id)).await;
}

async fn reap_tmux_sessions(ctx: &RuntimeContext, run_id: Option<&str>) {
    let reapable_sessions = match run_id {
        Some(run_id) => ctx.db.list_reapable_tmux_sessions_for_run(run_id).await,
        None => ctx.db.list_reapable_tmux_sessions().await,
    };
    let reapable_sessions = match reapable_sessions {
        Ok(sessions) => sessions,
        Err(error) => {
            tracing::warn!(error = %error, "Skipping stale tmux cleanup after storage error");
            return;
        }
    };
    let active_run_ids = ctx.registry.active_run_ids().await;
    let mut terminal_sessions_by_invocation = HashMap::<
        (Vec<String>, Option<String>, String),
        (TmuxInvocation, HashMap<String, BTreeSet<String>>),
    >::new();
    let mut reconstructed_invocations = HashMap::<String, TmuxInvocation>::new();
    for reapable in reapable_sessions {
        if active_run_ids.contains(&reapable.run_id) {
            continue;
        }
        let invocation = match reapable.tmux_invocation {
            Some(invocation) => invocation,
            None => {
                if let Some(invocation) = reconstructed_invocations.get(&reapable.run_id) {
                    invocation.clone()
                } else {
                    let persisted = match ctx.db.get_run(&reapable.run_id).await {
                        Ok(Some(persisted)) => persisted,
                        Ok(None) => {
                            tracing::warn!(
                                run_id = %reapable.run_id,
                                session_name = %reapable.session_name,
                                "Skipping stale tmux session whose run no longer exists"
                            );
                            continue;
                        }
                        Err(error) => {
                            tracing::warn!(
                                run_id = %reapable.run_id,
                                session_name = %reapable.session_name,
                                error = %error,
                                "Skipping stale tmux session after run load failure"
                            );
                            continue;
                        }
                    };
                    let invocation = persisted
                        .workflow
                        .run_as
                        .as_ref()
                        .map(|run_as| {
                            crate::tmux_exec::build_tmux_invocation_without_resolving(
                                run_as,
                                &reapable.run_id,
                            )
                        })
                        .unwrap_or_else(|| legacy_reaper_default_invocation(ctx));
                    if persisted.tmux_invocation.is_none() {
                        if let Err(error) = ctx
                            .db
                            .store_tmux_invocation_if_missing(&reapable.run_id, &invocation)
                            .await
                        {
                            tracing::warn!(
                                run_id = %reapable.run_id,
                                error = %error,
                                "Could not persist reconstructed legacy tmux invocation"
                            );
                        }
                    }
                    reconstructed_invocations.insert(reapable.run_id.clone(), invocation.clone());
                    invocation
                }
            }
        };
        let key = tmux_invocation_key(&invocation);
        let entry = terminal_sessions_by_invocation
            .entry(key)
            .or_insert_with(|| (invocation, HashMap::new()));
        entry
            .1
            .entry(reapable.session_name)
            .or_default()
            .insert(reapable.run_id);
    }

    for (_, (invocation, terminal_session_runs)) in terminal_sessions_by_invocation {
        let live_sessions = match tokio::task::spawn_blocking({
            let invocation = invocation.clone();
            move || {
                tmux_tools_core::with_invocation(invocation, || {
                    crate::tmux_exec::list_silverbond_tmux_sessions()
                })
            }
        })
        .await
        {
            Ok(Ok(sessions)) => sessions,
            Ok(Err(error)) => {
                tracing::warn!(
                    error = %error,
                    invocation = ?invocation,
                    "Skipping stale tmux sessions after list failure"
                );
                continue;
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    invocation = ?invocation,
                    "Skipping stale tmux sessions after list task failure"
                );
                continue;
            }
        };
        let live_sessions = live_sessions.into_iter().collect::<HashSet<_>>();

        for (session, session_run_ids) in terminal_session_runs {
            if !live_sessions.contains(&session) {
                for run_id in session_run_ids {
                    if let Err(error) = ctx
                        .db
                        .remove_tmux_sessions(&run_id, &BTreeSet::from([session.clone()]))
                        .await
                    {
                        tracing::warn!(
                            run_id = %run_id,
                            session_name = %session,
                            error = %error,
                            "Absent tmux session registration could not be reconciled"
                        );
                    }
                }
                continue;
            }
            let invocation = invocation.clone();
            let session_to_kill = session.clone();
            let killed = tokio::task::spawn_blocking(move || {
                tmux_tools_core::with_invocation(invocation, || {
                    crate::tmux_exec::kill_tmux_session(&session_to_kill)
                })
            })
            .await;
            match killed {
                Ok(true) => {
                    for run_id in session_run_ids {
                        if let Err(error) = ctx
                            .db
                            .remove_tmux_sessions(&run_id, &BTreeSet::from([session.clone()]))
                            .await
                        {
                            tracing::warn!(
                                run_id = %run_id,
                                session_name = %session,
                                error = %error,
                                "Stale tmux session was killed but its registration remains"
                            );
                        }
                    }
                }
                Ok(false) => {
                    for run_id in session_run_ids {
                        tracing::warn!(
                            run_id = %run_id,
                            session_name = %session,
                            "Failed to kill stale tmux session; retaining registration"
                        );
                    }
                }
                Err(error) => {
                    for run_id in session_run_ids {
                        tracing::warn!(
                            run_id = %run_id,
                            session_name = %session,
                            error = %error,
                            "Stale tmux kill task failed; retaining registration"
                        );
                    }
                }
            }
        }
    }
}

async fn kill_active_run_panes(ctx: &RuntimeContext, run_id: &str) {
    let targets = ctx.registry.active_pane_cleanup_targets(run_id).await;
    if targets.is_empty() {
        return;
    }

    let cleanup_verdicts = run_pane_cleanup(ctx.run_invocation.clone(), targets).await;
    match cleanup_verdicts {
        Ok(verdicts) => {
            for verdict in verdicts.into_iter().filter(|verdict| !verdict.absent) {
                tracing::warn!(
                    run_id = %run_id,
                    pane_id = %verdict.target.pane_id,
                    session_name = ?verdict.target.session_name,
                    "Abort cleanup could not confirm tmux target absence; registration will be reconciled by the reaper"
                );
            }
        }
        Err(error) => tracing::warn!(
            run_id = %run_id,
            error = %error,
            "Abort tmux cleanup task failed; registrations will be reconciled by the reaper"
        ),
    }
}

async fn emit_event(ctx: &RuntimeContext, run_id: &str, mut event: RuntimeEvent) -> anyhow::Result<()> {
    let seq = ctx.db.append_event(run_id, &event).await?;
    event.seq = Some(seq);
    ctx.registry.send_event(run_id, event).await;
    Ok(())
}

fn checkpoint_content_hash(checkpoint: &RuntimeCheckpoint) -> u32 {
    let mut snapshot = checkpoint.clone();
    snapshot.updated_at.clear();
    djb2(
        &serde_json::to_string(&snapshot).expect("RuntimeCheckpoint should serialize for hashing"),
    )
}

async fn persist_checkpoint(
    ctx: &RuntimeContext,
    workflow: &WorkflowV3,
    checkpoint: &mut RuntimeCheckpoint,
) -> anyhow::Result<()> {
    let content_hash = checkpoint_content_hash(checkpoint);
    if ctx
        .registry
        .last_persist_hash(&checkpoint.run_id)
        .await
        == Some(content_hash)
    {
        return Ok(());
    }
    checkpoint.updated_at = now_iso();
    if ctx.db.update_run_checkpoint(checkpoint).await? {
        ctx.registry
            .record_persist_hash(&checkpoint.run_id, content_hash)
            .await;
        return Ok(());
    }
    ctx.db
        .upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: ctx.run_invocation.clone(),
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await?;
    ctx.registry
        .record_persist_hash(&checkpoint.run_id, content_hash)
        .await;
    Ok(())
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

    static OUTPUT_FIELD_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let output_re = OUTPUT_FIELD_RE
        .get_or_init(|| Regex::new(r"\{\{node:([^.}]+)\.output\.([^}]+)\}\}").unwrap());
    resolved = output_re
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
                .and_then(|result| serde_json::from_str::<Value>(&result.output).ok())
                .and_then(|output| get_nested_field(&output, field_path).cloned())
                .map(|value| value_to_template_string(&value))
                .unwrap_or_default()
        })
        .into_owned();

    static PARSED_OUTPUT_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let parsed_re = PARSED_OUTPUT_RE
        .get_or_init(|| Regex::new(r"\{\{node:([^.}]+)\.parsedOutput\.([^}]+)\}\}").unwrap());
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

fn build_log_id(workflow_name: &str, run_id: &str) -> String {
    let slug = slugify_filename(workflow_name);
    format!("{}_{}", slug, run_id.trim_start_matches("run_"))
}

/// Escalate an agent interaction through the runtime event/channel path.
pub(crate) async fn escalate_agent_interaction(
    ctx: &RuntimeContext,
    run_id: &str,
    session_id: &str,
    interaction_type: &str,
    description: &str,
    output_so_far: &str,
) -> anyhow::Result<String> {
    let abort_signal = ctx.registry.abort_signal(run_id).await;
    let (sender, receiver) = oneshot::channel();
    ctx.registry
        .set_pending_interaction(run_id, session_id, sender)
        .await?;
    let mut pending = PendingInteractionGuard::new(&ctx.registry, run_id, session_id);

    if let Err(err) = emit_event(
        ctx,
        run_id,
        RuntimeEvent::new("agent_interaction_required")
            .with("sessionId", session_id)
            .with("interactionType", interaction_type)
            .with("description", description)
            .with("outputSoFar", output_so_far),
    )
    .await
    {
        pending.clear_now().await;
        return Err(err);
    }

    let response = if let Some(abort_token) = abort_signal {
        tokio::select! {
            response = receiver => {
                response.map_err(|_| anyhow::anyhow!("Interaction channel closed"))?
            }
            _ = abort_token.cancelled() => {
                pending.clear_now().await;
                anyhow::bail!("Interaction aborted");
            }
        }
    } else {
        receiver
            .await
            .map_err(|_| anyhow::anyhow!("Interaction channel closed"))?
    };
    pending.disarm();

    // Emit resolved event
    emit_event(
        ctx,
        run_id,
        RuntimeEvent::new("agent_interaction_resolved")
            .with("sessionId", session_id)
            .with("description", description)
            .with("response", &response),
    )
    .await?;

    Ok(response)
}

struct PendingInteractionGuard {
    registry: RunRegistry,
    run_id: String,
    session_id: String,
    armed: bool,
}

impl PendingInteractionGuard {
    fn new(registry: &RunRegistry, run_id: &str, session_id: &str) -> Self {
        Self {
            registry: registry.clone(),
            run_id: run_id.to_string(),
            session_id: session_id.to_string(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    async fn clear_now(&mut self) {
        if self.armed {
            self.registry
                .clear_pending_interaction(&self.run_id, &self.session_id)
                .await;
            self.armed = false;
        }
    }
}

impl Drop for PendingInteractionGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let registry = self.registry.clone();
        let run_id = std::mem::take(&mut self.run_id);
        let session_id = std::mem::take(&mut self.session_id);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let _ = handle.spawn(async move {
                registry
                    .clear_pending_interaction(&run_id, &session_id)
                    .await;
            });
        }
    }
}

async fn emit_orchestrator_warn(
    ctx: &RuntimeContext,
    run_id: &str,
    cursor_id: &str,
    node_id: &str,
    error: String,
) -> anyhow::Result<()> {
    emit_event(
        ctx,
        run_id,
        RuntimeEvent::new("orchestrator_warn")
            .with("cursorId", cursor_id)
            .with("nodeId", node_id)
            .with(
                "message",
                "Orchestrator refinement failed — using original prompt",
            )
            .with("error", error),
    )
    .await
}

async fn run_orchestrator_refinement(
    ctx: &RuntimeContext,
    _run_id: &str,
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
    let agent = DEFAULT_AGENT.to_string();
    let owned_prompt = prompt.clone();
    let owned_cwd = cwd.to_string();
    let inv = ctx
        .run_invocation
        .clone()
        .context("tmux invocation missing for orchestrator refinement")?;
    tokio::task::spawn_blocking(move || {
        crate::tmux_exec::run_tmux_oneshot(&agent, &owned_prompt, &owned_cwd, None, inv, None, None)
    })
    .await
    .context("join error in orchestrator")?
}

async fn run_orchestrator_branch(
    goal: &str,
    node: &WorkflowNode,
    output: &str,
    branches: &[&WorkflowEdge],
    cwd: &str,
    inv: TmuxInvocation,
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
    let agent = DEFAULT_AGENT.to_string();
    let owned_prompt = prompt.clone();
    let owned_cwd = cwd.to_string();
    tokio::task::spawn_blocking(move || {
        crate::tmux_exec::run_tmux_oneshot(&agent, &owned_prompt, &owned_cwd, None, inv, None, None)
    })
    .await
    .context("join error in orchestrator")?
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        collections::{HashMap, VecDeque},
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex as StdMutex},
        time::{Duration, Instant},
    };

    use serde_json::{Value, json};
    use tempfile::TempDir;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::{
        driver::{AccessMode, AgentConfig, ReasoningLevel, get_driver},
        model::{
            AgentDefaults, AgentNodeConfig, BatchConfig, InputBinding, NodeKind,
            SplitFailurePolicy, StructuredCondition, SubflowConfig, SkipCondition,
            WorkflowEdge, WorkflowEdgeOutcome, WorkflowLimits, WorkflowNode, WorkflowNodeType,
            WorkflowV3, WorkflowVariable, normalize_workflow_value, resolve_agent_config,
        },
        storage::Database,
    };

    use super::*;

    const TMUX_CLEANUP_TEST_TIMEOUT: Duration = crate::test_support::test_budget(
        tmux_tools_core::tmux::DEFAULT_TMUX_COMMAND_TIMEOUT,
    );

    thread_local! {
        static ASYNC_WORKER_MARKER: Cell<bool> = const { Cell::new(false) };
    }

    fn decide_matched(label: &str) -> DecideOutcomeSelection {
        DecideOutcomeSelection::Matched(label.to_string())
    }

    fn decide_unmatched() -> DecideOutcomeSelection {
        DecideOutcomeSelection::Failed(DecideOutcomeFailure::Unmatched)
    }

    fn decide_structured_label_mismatch() -> DecideOutcomeSelection {
        DecideOutcomeSelection::Failed(DecideOutcomeFailure::StructuredLabelMismatch)
    }

    #[derive(Clone, Default)]
    struct WarningCapture {
        warnings: Arc<StdMutex<Vec<String>>>,
    }

    #[derive(Default)]
    struct WarningVisitor {
        fields: Vec<String>,
    }

    impl tracing::field::Visit for WarningVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }

    impl<S> tracing_subscriber::Layer<S> for WarningCapture
    where
        S: tracing::Subscriber,
    {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() != tracing::Level::WARN {
                return;
            }
            let mut visitor = WarningVisitor::default();
            event.record(&mut visitor);
            self.warnings.lock().unwrap().push(visitor.fields.join(" "));
        }
    }

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

        fn with_parsed_output(mut self, parsed_output: Value) -> Self {
            self.parsed_output = Some(parsed_output);
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
                        return Err(anyhow::anyhow!(
                            "No scripted step for prompt \"{}\"",
                            prompt
                        ));
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

    #[derive(Clone)]
    struct PaneTrackingRunner {
        registrations: Arc<Mutex<Vec<(String, String, String)>>>,
        registered: Arc<tokio::sync::Barrier>,
        release: Arc<tokio::sync::Barrier>,
    }

    impl PaneTrackingRunner {
        fn new(expected_tasks: usize) -> Self {
            Self {
                registrations: Arc::new(Mutex::new(Vec::new())),
                registered: Arc::new(tokio::sync::Barrier::new(expected_tasks + 1)),
                release: Arc::new(tokio::sync::Barrier::new(expected_tasks + 1)),
            }
        }
    }

    impl NodeRunner for PaneTrackingRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            Box::pin(async move { Err(anyhow::anyhow!("unexpected basic run for {prompt}")) })
        }

        fn run_node_with_interaction(
            &self,
            node: WorkflowNode,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
            _previous_output: String,
            ctx: RuntimeContext,
            run_id: String,
            cursor_id: String,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            let registrations = self.registrations.clone();
            let registered = self.registered.clone();
            let release = self.release.clone();
            Box::pin(async move {
                let key = active_pane_key(&cursor_id, &node.id);
                let pane = format!("pane-{cursor_id}");
                ctx.registry.set_active_pane(&run_id, &key, &pane).await;
                registrations
                    .lock()
                    .await
                    .push((cursor_id.clone(), key.clone(), pane.clone()));
                registered.wait().await;
                release.wait().await;
                ctx.registry.clear_active_pane(&run_id, &key).await;
                Ok(NodeResult {
                    success: true,
                    output: pane,
                    exit_code: 0,
                    duration: "0".to_string(),
                    agent: "mock".to_string(),
                    prompt: prompt.clone(),
                    resolved_prompt: Some(prompt),
                    ..Default::default()
                })
            })
        }
    }

    #[derive(Clone)]
    struct PersistentPaneRunner {
        pane_id: String,
    }

    impl PersistentPaneRunner {
        fn new(pane_id: &str) -> Self {
            Self {
                pane_id: pane_id.to_string(),
            }
        }
    }

    impl NodeRunner for PersistentPaneRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            Box::pin(async move { Err(anyhow::anyhow!("unexpected basic run for {prompt}")) })
        }

        fn run_node_with_interaction(
            &self,
            node: WorkflowNode,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
            _previous_output: String,
            ctx: RuntimeContext,
            run_id: String,
            cursor_id: String,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            let pane_id = self.pane_id.clone();
            Box::pin(async move {
                let key = active_pane_key(&cursor_id, &node.id);
                ctx.registry.set_active_pane(&run_id, &key, &pane_id).await;
                Ok(NodeResult {
                    success: true,
                    output: pane_id,
                    exit_code: 0,
                    duration: "0".to_string(),
                    agent: "mock".to_string(),
                    prompt: prompt.clone(),
                    resolved_prompt: Some(prompt),
                    ..Default::default()
                })
            })
        }
    }

    #[derive(Debug, Clone)]
    struct CapturedRun {
        node_id: String,
        cwd: String,
        config: Option<AgentConfig>,
    }

    #[derive(Clone)]
    struct CapturingRunner {
        captures: Arc<Mutex<Vec<CapturedRun>>>,
    }

    impl CapturingRunner {
        fn new() -> Self {
            Self {
                captures: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn captures(&self) -> Vec<CapturedRun> {
            self.captures.lock().await.clone()
        }
    }

    impl NodeRunner for CapturingRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            Box::pin(async move { Err(anyhow::anyhow!("unexpected basic run for {prompt}")) })
        }

        fn run_node_with_interaction(
            &self,
            node: WorkflowNode,
            _agent: String,
            prompt: String,
            cwd: String,
            _timeout_secs: Option<u64>,
            config: Option<AgentConfig>,
            _previous_output: String,
            _ctx: RuntimeContext,
            _run_id: String,
            _cursor_id: String,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            let captures = self.captures.clone();
            Box::pin(async move {
                captures.lock().await.push(CapturedRun {
                    node_id: node.id.clone(),
                    cwd: cwd.clone(),
                    config: config.clone(),
                });
                Ok(NodeResult {
                    success: true,
                    output: format!("{} done", node.id),
                    exit_code: 0,
                    duration: "0".to_string(),
                    agent: "mock".to_string(),
                    prompt: prompt.clone(),
                    resolved_prompt: Some(prompt),
                    ..Default::default()
                })
            })
        }
    }

    #[derive(Clone)]
    struct ContinuationPaneRunner {
        captures: Arc<Mutex<Vec<CapturedRun>>>,
    }

    impl ContinuationPaneRunner {
        fn new() -> Self {
            Self {
                captures: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn captures(&self) -> Vec<CapturedRun> {
            self.captures.lock().await.clone()
        }
    }

    impl NodeRunner for ContinuationPaneRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            Box::pin(async move { Err(anyhow::anyhow!("unexpected basic run for {prompt}")) })
        }

        fn run_node_with_interaction(
            &self,
            node: WorkflowNode,
            _agent: String,
            prompt: String,
            cwd: String,
            _timeout_secs: Option<u64>,
            config: Option<AgentConfig>,
            _previous_output: String,
            ctx: RuntimeContext,
            run_id: String,
            cursor_id: String,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            let captures = self.captures.clone();
            Box::pin(async move {
                captures.lock().await.push(CapturedRun {
                    node_id: node.id.clone(),
                    cwd,
                    config: config.clone(),
                });

                let key = active_pane_key(&cursor_id, &node.id);
                let pane = if let Some(source_id) = node.continue_session_from.as_deref() {
                    let source_key = active_pane_key(&cursor_id, source_id);
                    ctx.registry
                        .resolve_active_pane(&run_id, &source_key)
                        .await
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "continueSessionFrom source node \"{}\" does not have an active tmux pane",
                                source_id
                            )
                        })?
                } else {
                    format!("pane-{}", node.id)
                };

                ctx.registry.set_active_pane(&run_id, &key, &pane).await;
                if config
                    .as_ref()
                    .map(|config| config.ephemeral_session)
                    .unwrap_or(true)
                {
                    ctx.registry.clear_active_pane(&run_id, &key).await;
                }

                Ok(NodeResult {
                    success: true,
                    output: pane.clone(),
                    exit_code: 0,
                    duration: "0".to_string(),
                    agent: "mock".to_string(),
                    prompt: prompt.clone(),
                    resolved_prompt: Some(prompt),
                    metadata: AgentExecutionMetadata {
                        agent_session_id: Some(pane),
                        ..Default::default()
                    },
                    ..Default::default()
                })
            })
        }
    }

    #[cfg(unix)]
    #[derive(Clone)]
    struct AbortBlockingRunner {
        pane_id: String,
        kill_marker: PathBuf,
        registered: Arc<tokio::sync::Barrier>,
    }

    #[cfg(unix)]
    impl AbortBlockingRunner {
        fn new(pane_id: &str, kill_marker: PathBuf) -> Self {
            Self {
                pane_id: pane_id.to_string(),
                kill_marker,
                registered: Arc::new(tokio::sync::Barrier::new(2)),
            }
        }
    }

    #[cfg(unix)]
    impl NodeRunner for AbortBlockingRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            Box::pin(async move { Err(anyhow::anyhow!("unexpected basic run for {prompt}")) })
        }

        fn run_node_with_interaction(
            &self,
            node: WorkflowNode,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
            _previous_output: String,
            ctx: RuntimeContext,
            run_id: String,
            cursor_id: String,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            let pane_id = self.pane_id.clone();
            let kill_marker = self.kill_marker.clone();
            let registered = self.registered.clone();
            Box::pin(async move {
                let key = active_pane_key(&cursor_id, &node.id);
                ctx.registry.set_active_pane(&run_id, &key, &pane_id).await;
                registered.wait().await;
                let pane_for_result = pane_id.clone();
                tokio::task::spawn_blocking(move || -> anyhow::Result<NodeResult> {
                    let deadline = Instant::now()
                        + TMUX_CLEANUP_TEST_TIMEOUT.saturating_add(
                            tmux_tools_core::tmux::DEFAULT_TMUX_COMMAND_TIMEOUT,
                        );
                    while !kill_marker.exists() && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    anyhow::ensure!(
                        kill_marker.exists(),
                        "timed out waiting for abort cleanup to kill active pane"
                    );
                    Ok(NodeResult {
                        success: false,
                        output: String::new(),
                        stderr: "aborted".to_string(),
                        exit_code: -1,
                        duration: "0".to_string(),
                        agent: "mock".to_string(),
                        prompt: prompt.clone(),
                        resolved_prompt: Some(prompt),
                        metadata: AgentExecutionMetadata {
                            agent_session_id: Some(pane_for_result),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                })
                .await
                .context("abort blocking runner task panicked")?
            })
        }
    }

    #[derive(Clone)]
    struct EchoRunner;

    impl NodeRunner for EchoRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            Box::pin(async move {
                Ok(NodeResult {
                    success: true,
                    output: prompt.clone(),
                    exit_code: 0,
                    duration: "0".to_string(),
                    agent: "mock".to_string(),
                    prompt: prompt.clone(),
                    resolved_prompt: Some(prompt),
                    ..Default::default()
                })
            })
        }
    }

    #[derive(Clone)]
    struct FlagshipTemplateRunner;

    impl NodeRunner for FlagshipTemplateRunner {
        fn run(
            &self,
            _agent: String,
            prompt: String,
            _cwd: String,
            _timeout_secs: Option<u64>,
            _config: Option<AgentConfig>,
        ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
            Box::pin(async move {
                let output = if prompt.starts_with("Read the plan document") {
                    json!({"tasks": ["task-a", "task-b"]}).to_string()
                } else if prompt.starts_with("Implement the following task") {
                    "implementation result".to_string()
                } else if prompt.starts_with("Run the test suite") {
                    "test result".to_string()
                } else if prompt.starts_with("Review batch results") {
                    "review result".to_string()
                } else if prompt.starts_with("Apply fixes for test failures") {
                    "fix result".to_string()
                } else if prompt.starts_with("Consolidate the multi-agent implementation run") {
                    "consolidated".to_string()
                } else {
                    prompt.clone()
                };
                Ok(NodeResult {
                    success: true,
                    output,
                    exit_code: 0,
                    duration: "0".to_string(),
                    agent: "mock".to_string(),
                    prompt: prompt.clone(),
                    resolved_prompt: Some(prompt),
                    ..Default::default()
                })
            })
        }
    }

    async fn next_interaction_required(
        events: &mut tokio::sync::broadcast::Receiver<RuntimeEvent>,
    ) -> RuntimeEvent {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("timed out waiting for interaction event")
                .expect("event stream closed");
            if event.kind == "agent_interaction_required" {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn abort_signal_is_latched_before_the_waiter_is_polled() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db);
        let run_id = "run_latched_abort_signal";
        ctx.registry.register(run_id).await;
        let abort_signal = ctx.registry.abort_signal(run_id).await.unwrap();

        ctx.abort_run(run_id).await.unwrap();

        tokio::time::timeout(Duration::from_secs(1), abort_signal.cancelled())
            .await
            .expect("an abort must remain observable before the waiter is first polled");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abort_and_wait_returns_when_the_run_drains_concurrently() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let registry = RunRegistry::default();
        let run_id = "run_concurrent_drain";
        registry.register(run_id).await;
        let abort_signal = registry.abort_signal(run_id).await.unwrap();
        let waiting_registry = registry.clone();
        let waiter = tokio::spawn(async move {
            waiting_registry.abort_and_wait(run_id).await;
        });

        tokio::time::timeout(Duration::from_secs(1), abort_signal.cancelled())
            .await
            .expect("abort_and_wait did not request cancellation");
        registry.clear(run_id).await;

        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("abort_and_wait hung after the run drained")
            .expect("abort_and_wait task panicked");
    }

    #[tokio::test]
    async fn abort_and_wait_force_clears_when_drain_never_arrives() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let registry = RunRegistry::default();
        let run_id = "run_drain_timeout";
        registry.register(run_id).await;

        let started = Instant::now();
        registry.abort_and_wait(run_id).await;
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "abort_and_wait should return within the configured drain timeout, took {:?}",
            elapsed
        );
        assert!(
            !registry.active_run_ids().await.contains(run_id),
            "registry entry should be force-cleared after drain timeout"
        );
    }

    #[tokio::test]
    async fn run_task_supervisor_clears_registry_on_panic() {
        let registry = RunRegistry::default();
        let run_id = "run_supervisor_panic";
        registry.register(run_id).await;

        let registry_supervisor = registry.clone();
        let run_id_supervisor = run_id.to_string();
        let supervisor = tokio::spawn(async move {
            let handle = tokio::spawn(async move {
                panic!("simulated run task panic");
            });
            let Err(join_error) = handle.await else {
                return;
            };
            tracing::warn!(
                run_id = %run_id_supervisor,
                panic = join_error.is_panic(),
                "run task exited before finalize_run; clearing registry entry (DB status may remain Running)"
            );
            registry_supervisor.clear(&run_id_supervisor).await;
        });
        supervisor.await.unwrap();

        assert!(
            !registry.active_run_ids().await.contains(run_id),
            "panic should clear the registry so resume is not blocked by AlreadyActive"
        );

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "go")], vec![]);
        let checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();
        runtime.resume_run(run_id).await.expect("resume after panic clear");
        runtime.registry.clear(run_id).await;
    }

    #[tokio::test]
    async fn persist_checkpoint_skips_redundant_writes_when_unchanged() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db.clone());
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "go")], vec![]);
        let run_id = "run_persist_dedup";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        persist_checkpoint(&ctx, &workflow, &mut checkpoint)
            .await
            .unwrap();
        let first_updated = checkpoint.updated_at.clone();
        let db_after_first = db.get_run(run_id).await.unwrap().unwrap();
        let db_updated_after_first = db_after_first.checkpoint.updated_at.clone();

        for _ in 0..5 {
            persist_checkpoint(&ctx, &workflow, &mut checkpoint)
                .await
                .unwrap();
        }

        assert_eq!(
            checkpoint.updated_at, first_updated,
            "unchanged checkpoint should not advance updated_at in memory"
        );
        let db_after_idle = db.get_run(run_id).await.unwrap().unwrap();
        assert_eq!(
            db_after_idle.checkpoint.updated_at, db_updated_after_first,
            "unchanged checkpoint should not produce additional DB writes"
        );
    }

    #[tokio::test]
    async fn checkpoint_persist_hash_retired_when_run_registry_cleared() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db.clone());
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "go")], vec![]);
        let run_id = "run_persist_hash_retire";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await
        .unwrap();
        ctx.registry.register(run_id).await;

        persist_checkpoint(&ctx, &workflow, &mut checkpoint)
            .await
            .unwrap();
        assert!(
            ctx.registry.last_persist_hash(run_id).await.is_some(),
            "persist should record dedupe hash"
        );

        let mut checkpoint = checkpoint.clone();
        checkpoint.status = RuntimeStatus::Completed;
        finalize_run(&ctx, &workflow, checkpoint, Duration::from_millis(1))
            .await
            .unwrap();

        assert!(
            ctx.registry.last_persist_hash(run_id).await.is_none(),
            "finalize_run should clear checkpoint dedupe hash via registry.clear"
        );
    }

    #[tokio::test]
    async fn orphan_waiting_approval_cursor_is_reconciled_on_resume() {
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
            "approve",
            vec![
                approval_node("gate", "Gate", "approve?"),
                task_node("after", "After", "after"),
            ],
            vec![success_edge("gate_after", "gate", "after", None)],
        );
        let run_id = "run_orphan_waiting_approval";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Paused;
        checkpoint.active_cursors = vec![CursorState {
            cursor_id: "cursor-orphan".to_string(),
            node_id: "gate".to_string(),
            execution_epoch: checkpoint.execution_epoch,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: String::new(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            var_map: BTreeMap::new(),
            call_stack: Vec::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::WaitingApproval,
        }];
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();
        let events = wait_for_event(&db, run_id, "workflow_warn").await;
        let warn = events
            .iter()
            .find(|event| event.kind == "workflow_warn")
            .expect("expected workflow_warn for orphan WaitingApproval cursor");
        assert_eq!(warn.data.get("nodeId").and_then(Value::as_str), Some("gate"));
        assert_eq!(
            warn.data.get("cursorId").and_then(Value::as_str),
            Some("cursor-orphan")
        );
    }

    #[tokio::test]
    async fn idle_unschedulable_cursors_fail_instead_of_spinning() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db.clone());
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "go")], vec![]);
        let run_id = "run_idle_stuck";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.active_cursors = vec![CursorState {
            cursor_id: "cursor-stuck".to_string(),
            node_id: "work".to_string(),
            execution_epoch: checkpoint.execution_epoch,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: String::new(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            var_map: BTreeMap::new(),
            call_stack: Vec::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::WaitingApproval,
        }];

        let failed =
            fail_run_on_idle_unschedulable_cursors(&ctx, &workflow, &mut checkpoint, run_id)
                .await
                .unwrap();
        assert!(failed);
        assert_eq!(checkpoint.status, RuntimeStatus::Failed);
        assert!(checkpoint.active_cursors.is_empty());
        let events = wait_for_event(&db, run_id, "workflow_error").await;
        assert!(
            events.iter().any(|event| event.kind == "workflow_error"),
            "stuck scheduler path should emit workflow_error"
        );
    }

    #[tokio::test]
    async fn concurrent_interactions_resolve_by_session_id() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db);
        let run_id = "run_concurrent_interactions";
        ctx.registry.register(run_id).await;
        let mut events = ctx.registry.subscribe(run_id).await.unwrap();

        let task_a = {
            let ctx = ctx.clone();
            let run_id = run_id.to_string();
            tokio::spawn(async move {
                escalate_agent_interaction(
                    &ctx,
                    &run_id,
                    "session-a",
                    "question",
                    "question a",
                    "output a",
                )
                .await
            })
        };
        let task_b = {
            let ctx = ctx.clone();
            let run_id = run_id.to_string();
            tokio::spawn(async move {
                escalate_agent_interaction(
                    &ctx,
                    &run_id,
                    "session-b",
                    "question",
                    "question b",
                    "output b",
                )
                .await
            })
        };

        let first = next_interaction_required(&mut events).await;
        let second = next_interaction_required(&mut events).await;
        let mut sessions = vec![
            first
                .data
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap()
                .to_string(),
            second
                .data
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap()
                .to_string(),
        ];
        sessions.sort();
        assert_eq!(sessions, vec!["session-a", "session-b"]);

        ctx.respond_interaction(run_id, "session-b", "response b".to_string())
            .await
            .unwrap();
        ctx.respond_interaction(run_id, "session-a", "response a".to_string())
            .await
            .unwrap();

        let response_a = tokio::time::timeout(Duration::from_secs(1), task_a)
            .await
            .expect("timed out waiting for session-a")
            .unwrap()
            .unwrap();
        let response_b = tokio::time::timeout(Duration::from_secs(1), task_b)
            .await
            .expect("timed out waiting for session-b")
            .unwrap()
            .unwrap();
        assert_eq!(response_a, "response a");
        assert_eq!(response_b, "response b");
    }

    #[tokio::test]
    async fn interaction_can_be_resolved_immediately_after_required_event() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db);
        let run_id = "run_interaction_race";
        ctx.registry.register(run_id).await;
        let mut events = ctx.registry.subscribe(run_id).await.unwrap();

        let task = {
            let ctx = ctx.clone();
            let run_id = run_id.to_string();
            tokio::spawn(async move {
                escalate_agent_interaction(
                    &ctx,
                    &run_id,
                    "race-session",
                    "question",
                    "respond now",
                    "",
                )
                .await
            })
        };

        let event = next_interaction_required(&mut events).await;
        let session_id = event.data.get("sessionId").and_then(Value::as_str).unwrap();
        assert_eq!(session_id, "race-session");

        ctx.respond_interaction(run_id, session_id, "immediate response".to_string())
            .await
            .unwrap();

        let response = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("timed out waiting for interaction response")
            .unwrap()
            .unwrap();
        assert_eq!(response, "immediate response");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abort_delivered_as_agent_enters_interaction_wait_terminates_the_node() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db);
        let run_id = "run_interaction_abort";
        ctx.registry.register(run_id).await;
        let mut events = ctx.registry.subscribe(run_id).await.unwrap();

        let task = {
            let ctx = ctx.clone();
            let run_id = run_id.to_string();
            tokio::spawn(async move {
                escalate_agent_interaction(
                    &ctx,
                    &run_id,
                    "abort-session",
                    "question",
                    "waiting for abort",
                    "",
                )
                .await
            })
        };

        let event = next_interaction_required(&mut events).await;
        assert_eq!(
            event.data.get("sessionId").and_then(Value::as_str),
            Some("abort-session")
        );

        let abort_started = Instant::now();
        ctx.abort_run(run_id).await.unwrap();
        let err = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("timed out waiting for aborted interaction")
            .unwrap()
            .unwrap_err();
        assert_eq!(err.to_string(), "Interaction aborted");
        assert!(
            abort_started.elapsed() < Duration::from_millis(500),
            "abort did not promptly unblock the interaction"
        );
        assert!(
            ctx.respond_interaction(run_id, "abort-session", "late".to_string())
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn run_as_invocation_resolution_uses_blocking_pool() {
        ASYNC_WORKER_MARKER.with(|marker| marker.set(true));
        let run_as = model::RunAsConfig {
            command: Some(vec!["sandbox-prefix".to_string()]),
            user: None,
        };

        let resolved = resolve_workflow_invocation_with(
            None,
            Some(run_as),
            "run_blocking".to_string(),
            |run_as, run_id| {
                let ran_inline = ASYNC_WORKER_MARKER.with(|marker| marker.get());
                assert!(
                    !ran_inline,
                    "runAs tmux invocation resolution ran inline on the async worker"
                );
                TmuxInvocation {
                    prefix: run_as.command.clone().unwrap_or_default(),
                    socket: Some(format!("silverbond-{run_id}")),
                    tmux_bin: "tmux-from-builder".to_string(),
                }
            },
        )
        .await
        .unwrap()
        .unwrap();

        ASYNC_WORKER_MARKER.with(|marker| marker.set(false));
        assert_eq!(resolved.prefix, vec!["sandbox-prefix"]);
        assert_eq!(resolved.socket.as_deref(), Some("silverbond-run_blocking"));
        assert_eq!(resolved.tmux_bin, "tmux-from-builder");
    }

    #[tokio::test]
    async fn no_run_as_invocation_resolution_returns_run_scoped_socket() {
        let run_id = "run_no_run_as_socket";
        let resolved = resolve_workflow_invocation(None, None, run_id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved,
            crate::tmux_exec::run_scoped_tmux_invocation(run_id)
        );
    }

    #[tokio::test]
    async fn start_run_without_run_as_persists_run_scoped_socket() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let workflow = workflow_from_parts(
            "approve",
            vec![approval_node("approve", "Approve", "continue?")],
            vec![],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();

        let persisted = db.get_run(&run_id).await.unwrap().unwrap();
        assert_eq!(
            persisted.tmux_invocation,
            Some(crate::tmux_exec::run_scoped_tmux_invocation(&run_id))
        );

        runtime.abort_run(&run_id).await.unwrap();
    }

    #[tokio::test]
    async fn resume_run_without_run_as_persists_default_invocation_for_legacy_rows() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let run_id = "run_resume_no_run_as";
        let workflow = workflow_from_parts(
            "approve",
            vec![approval_node("approve", "Approve", "continue?")],
            vec![],
        );
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint: build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None),
            workflow,
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();

        let persisted = db.get_run(run_id).await.unwrap().unwrap();
        assert_eq!(persisted.tmux_invocation, Some(TmuxInvocation::default()));

        runtime.abort_run(run_id).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn stale_tmux_reaper_reconstructs_no_run_as_invocation_without_resolving() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();

        let run_id = "run_no_run_as_stale_reaper";
        let session = "silverbond-No-RunAs-Stale";
        let log = temp.path().join("no-run-as-reaper-args.log");
        let fake_tmux = temp.path().join("tmux");
        fs::write(
            &fake_tmux,
            format!(
                "#!/bin/sh\nlog=\"{}\"\nprintf '%s\\n' \"$@\" >> \"$log\"\nif [ \"$1\" = \"-L\" ]; then shift 2; fi\nif [ \"$1\" = \"list-sessions\" ]; then printf '%s\\n' \"{session}\"; fi\nif [ \"$1\" = \"has-session\" ]; then exit 1; fi\nexit 0\n",
                log.to_string_lossy()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&fake_tmux).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_tmux, permissions).unwrap();

        let ctx = RuntimeContext {
            legacy_reaper_tmux_bin: Some(fake_tmux.to_string_lossy().into_owned()),
            ..RuntimeContext::new(db.clone())
        };

        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Failed;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow,
        })
        .await
        .unwrap();
        register_tmux_session(&db, run_id, session).await.unwrap();

        reap_stale_tmux_sessions(&ctx).await;

        let recorded = fs::read_to_string(&log).expect("fake tmux should record reaper args");
        let args = recorded.lines().collect::<Vec<_>>();
        assert!(
            args.windows(3).any(|window| window == ["list-sessions", "-F", "#{session_name}"]),
            "legacy no-runAs reaper should list sessions on the default tmux server; args={args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == &"-L"),
            "legacy no-runAs reaper must not target a run-scoped socket; args={args:?}"
        );
        assert!(
            args.windows(3).any(|window| window == ["kill-session", "-t", session]),
            "reaper should kill the stale session on the default tmux server; args={args:?}"
        );
        assert!(db.list_reapable_tmux_sessions().await.unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_run_persists_resolved_tmux_invocation_before_returning() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let script = temp.path().join("resolve-tmux.sh");
        let lookup_log = temp.path().join("tmux-lookups.log");
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nshift\nif [ \"$1\" = \"zsh\" ]; then printf 'lookup\\n' >> \"$log\"; printf 'SBTMUX:/custom/bin/tmux\\n'; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let mut workflow = workflow_from_parts(
            "approve",
            vec![approval_node("approve", "Approve", "continue?")],
            vec![],
        );
        workflow.run_as = Some(model::RunAsConfig {
            command: Some(vec![
                script.to_string_lossy().into_owned(),
                lookup_log.to_string_lossy().into_owned(),
            ]),
            user: None,
        });

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();

        let persisted = db.get_run(&run_id).await.unwrap().unwrap();
        assert_eq!(
            persisted.tmux_invocation,
            Some(TmuxInvocation {
                prefix: vec![
                    script.to_string_lossy().into_owned(),
                    lookup_log.to_string_lossy().into_owned(),
                ],
                socket: Some(format!("silverbond-{run_id}")),
                tmux_bin: "/custom/bin/tmux".to_string(),
            })
        );
        assert_eq!(fs::read_to_string(&lookup_log).unwrap(), "lookup\n");

        runtime.abort_run(&run_id).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn resume_run_persists_missing_tmux_invocation_before_returning() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let script = temp.path().join("resolve-legacy-tmux.sh");
        let lookup_log = temp.path().join("legacy-tmux-lookups.log");
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nshift\nif [ \"$1\" = \"zsh\" ]; then printf 'lookup\\n' >> \"$log\"; printf 'SBTMUX:/legacy/bin/tmux\\n'; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let run_id = "run_pre_m5";
        let mut workflow = workflow_from_parts(
            "approve",
            vec![approval_node("approve", "Approve", "continue?")],
            vec![],
        );
        workflow.run_as = Some(model::RunAsConfig {
            command: Some(vec![
                script.to_string_lossy().into_owned(),
                lookup_log.to_string_lossy().into_owned(),
            ]),
            user: None,
        });
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint: build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None),
            workflow,
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();

        let persisted = db.get_run(run_id).await.unwrap().unwrap();
        assert_eq!(
            persisted.tmux_invocation,
            Some(TmuxInvocation {
                prefix: vec![
                    script.to_string_lossy().into_owned(),
                    lookup_log.to_string_lossy().into_owned(),
                ],
                socket: Some(format!("silverbond-{run_id}")),
                tmux_bin: "/legacy/bin/tmux".to_string(),
            })
        );
        assert_eq!(fs::read_to_string(&lookup_log).unwrap(), "lookup\n");

        runtime.abort_run(run_id).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn restart_run_persists_fresh_tmux_invocation_before_returning() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let script = temp.path().join("resolve-restart-tmux.sh");
        let lookup_log = temp.path().join("restart-tmux-lookups.log");
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nshift\nif [ \"$1\" = \"zsh\" ]; then printf 'lookup\\n' >> \"$log\"; printf 'SBTMUX:/restart/bin/tmux\\n'; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let old_run_id = "run_before_restart";
        let mut workflow = workflow_from_parts(
            "approve",
            vec![approval_node("approve", "Approve", "continue?")],
            vec![],
        );
        workflow.run_as = Some(model::RunAsConfig {
            command: Some(vec![
                script.to_string_lossy().into_owned(),
                lookup_log.to_string_lossy().into_owned(),
            ]),
            user: None,
        });
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: Some(TmuxInvocation {
                prefix: vec!["old-prefix".to_string()],
                socket: Some("old-socket".to_string()),
                tmux_bin: "old-tmux".to_string(),
            }),
            checkpoint: build_initial_checkpoint(&workflow, old_run_id, BTreeMap::new(), None),
            workflow,
        })
        .await
        .unwrap();

        let new_run_id = runtime.restart_from(old_run_id, "approve").await.unwrap();

        let persisted = db.get_run(&new_run_id).await.unwrap().unwrap();
        assert_eq!(
            persisted.tmux_invocation,
            Some(TmuxInvocation {
                prefix: vec![
                    script.to_string_lossy().into_owned(),
                    lookup_log.to_string_lossy().into_owned(),
                ],
                socket: Some(format!("silverbond-{new_run_id}")),
                tmux_bin: "/restart/bin/tmux".to_string(),
            })
        );
        assert_eq!(fs::read_to_string(&lookup_log).unwrap(), "lookup\n");

        runtime.abort_run(&new_run_id).await.unwrap();
    }

    #[cfg(unix)]
    fn fake_run_as_config(temp: &TempDir, stem: &str) -> (model::RunAsConfig, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join(format!("{stem}-prefix.sh"));
        let log = temp.path().join(format!("{stem}-args.log"));
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nshift\nprintf '%s\\n' \"$@\" > \"$log\"\nexit 87\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        (
            model::RunAsConfig {
                command: Some(vec![
                    script.to_string_lossy().into_owned(),
                    log.to_string_lossy().into_owned(),
                    "sandbox-prefix".to_string(),
                ]),
                user: None,
            },
            log,
        )
    }

    #[cfg(unix)]
    fn fake_run_as_invocation(temp: &TempDir, stem: &str) -> (TmuxInvocation, PathBuf) {
        let (run_as, log) = fake_run_as_config(temp, stem);
        let inv = crate::tmux_exec::build_tmux_invocation(&run_as, stem);
        let _ = fs::remove_file(&log);

        (inv, log)
    }

    #[cfg(unix)]
    fn fake_tmux_kill_invocation(temp: &TempDir) -> (TmuxInvocation, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("tmux-kill-prefix.sh");
        let log = temp.path().join("tmux-kill-args.log");
        let marker = temp.path().join("tmux-pane-killed");
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nmarker=\"$2\"\nshift 2\nprintf '%s\\n' \"$@\" >> \"$log\"\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"-L\" ]; then shift 2; fi\nif [ \"$1\" = \"kill-pane\" ]; then touch \"$marker\"; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        (
            TmuxInvocation {
                prefix: vec![
                    script.to_string_lossy().into_owned(),
                    log.to_string_lossy().into_owned(),
                    marker.to_string_lossy().into_owned(),
                ],
                socket: Some("abort-kill-socket".to_string()),
                tmux_bin: "tmux".to_string(),
            },
            log,
            marker,
        )
    }

    #[cfg(unix)]
    async fn wait_for_path(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if path.exists() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for path {}",
                path.display()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_registry_empty(registry: &RunRegistry, run_id: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if registry.active_pane_targets(run_id).await.is_empty() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for run {} active pane registry to clear",
                run_id
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(unix)]
    fn assert_recorded_run_as_tmux_args(log: &Path, socket: &str) {
        let recorded = fs::read_to_string(log).expect("fake tmux prefix should record arguments");
        let args = recorded.lines().collect::<Vec<_>>();
        assert_eq!(args.first().copied(), Some("sandbox-prefix"));
        assert_eq!(args.get(1).copied(), Some("tmux"));
        assert_eq!(args.get(2).copied(), Some("-L"));
        assert_eq!(args.get(3).copied(), Some(socket));
        assert_eq!(args.get(4).copied(), Some("new-session"));
    }

    fn task_node(id: &str, name: &str, prompt: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: name.to_string(),
            kind: WorkflowNodeType::Task.into(),
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
            cwd: None,
            continue_session_from: None,
        }
    }

    fn approval_node(id: &str, name: &str, prompt: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: name.to_string(),
            kind: WorkflowNodeType::Approval.into(),
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
            cwd: None,
            continue_session_from: None,
        }
    }

    fn split_node(id: &str, policy: SplitFailurePolicy) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: format!("Split {}", id),
            kind: WorkflowNodeType::Split.into(),
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
            cwd: None,
            continue_session_from: None,
        }
    }

    fn collector_node(id: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: format!("Collector {}", id),
            kind: WorkflowNodeType::Collector.into(),
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
            cwd: None,
            continue_session_from: None,
        }
    }

    fn parallel_batch_node(
        id: &str,
        items_binding: &str,
        max_concurrent: u32,
        item_var: &str,
        body_entry: &str,
        collector_var: Option<&str>,
    ) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: format!("Batch {}", id),
            kind: NodeKind::ParallelBatch {
                batch_config: BatchConfig {
                    items_binding: items_binding.to_string(),
                    max_concurrent,
                    item_var: item_var.to_string(),
                    body_entry: body_entry.to_string(),
                    collector_var: collector_var.map(str::to_string),
                },
            },
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
            cwd: None,
            continue_session_from: None,
        }
    }

    fn call_node(
        id: &str,
        workflow_name: &str,
        exit_node_id: &str,
        inputs: Vec<InputBinding>,
    ) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: format!("Call {}", workflow_name),
            kind: NodeKind::Call {
                subflow_config: SubflowConfig {
                    workflow_name: workflow_name.to_string(),
                    exit_node_id: Some(exit_node_id.to_string()),
                    inputs,
                    max_depth: 5,
                },
            },
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
            version: 4,
            name: Some("test".to_string()),
            goal: "goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: entry_node_id.to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 20,
                max_visits_per_node: 10,
            },
            nodes,
            edges,
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        }
    }

    fn task_node_with_regex_skip(id: &str, name: &str, regex: &str) -> WorkflowNode {
        let mut node = task_node(id, name, "");
        node.skip_condition = Some(SkipCondition {
            source: "previous_output".to_string(),
            kind: "regex".to_string(),
            value: regex.to_string(),
        });
        node
    }

    #[test]
    fn compile_skip_regex_cache_honors_scope_for_duplicate_node_ids() {
        let root_step1 = task_node_with_regex_skip("step1", "Root Step 1", "^root-skip$");
        let subflow_step1 = task_node_with_regex_skip("step1", "Subflow Step 1", "^subflow-skip$");
        let subflow = WorkflowV3 {
            version: 4,
            name: Some("child".to_string()),
            goal: "child goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: "step1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 20,
                max_visits_per_node: 10,
            },
            nodes: vec![subflow_step1.clone()],
            edges: Vec::new(),
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        };
        let mut workflow = workflow_from_parts("step1", vec![root_step1.clone()], Vec::new());
        workflow
            .subflows
            .insert("child".to_string(), Box::new(subflow));

        let cache = compile_skip_regex_cache(&workflow).unwrap();
        let checkpoint = build_initial_checkpoint(&workflow, "run_scope_skip", BTreeMap::new(), None);

        let root_cursor = CursorState {
            cursor_id: "root".to_string(),
            node_id: "step1".to_string(),
            execution_epoch: 1,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: "root-skip".to_string(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            var_map: BTreeMap::new(),
            call_stack: Vec::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::Runnable,
        };
        let subflow_cursor = CursorState {
            cursor_id: "child".to_string(),
            node_id: "step1".to_string(),
            execution_epoch: 1,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: "subflow-skip".to_string(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            var_map: BTreeMap::new(),
            call_stack: vec![CallFrameState {
                frame_id: String::new(),
                call_node_id: "call_child".to_string(),
                call_node_name: "Call Child".to_string(),
                subflow_name: "child".to_string(),
                exit_node_id: "step1".to_string(),
                parent_last_output: String::new(),
                parent_loop_counters: BTreeMap::new(),
                parent_visit_counters: BTreeMap::new(),
                parent_last_branch_origin_id: None,
                parent_last_branch_choice: None,
                parent_var_map: BTreeMap::new(),
                subflow_results: BTreeMap::new(),
            }],
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::Runnable,
        };

        let root_cache = skip_regex_cache_for_cursor(&cache, &root_cursor).unwrap();
        assert!(should_skip_cursor_node(
            &root_step1,
            &root_cursor,
            &checkpoint,
            root_cache
        )
        .unwrap());

        let mut root_cursor_other_scope = root_cursor.clone();
        root_cursor_other_scope.last_output = "subflow-skip".to_string();
        assert!(!should_skip_cursor_node(
            &root_step1,
            &root_cursor_other_scope,
            &checkpoint,
            root_cache
        )
        .unwrap());

        let subflow_cache = skip_regex_cache_for_cursor(&cache, &subflow_cursor).unwrap();
        assert!(should_skip_cursor_node(
            &subflow_step1,
            &subflow_cursor,
            &checkpoint,
            subflow_cache
        )
        .unwrap());

        let mut subflow_cursor_other_scope = subflow_cursor.clone();
        subflow_cursor_other_scope.last_output = "root-skip".to_string();
        assert!(!should_skip_cursor_node(
            &subflow_step1,
            &subflow_cursor_other_scope,
            &checkpoint,
            subflow_cache
        )
        .unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_tmux_reaper_does_not_kill_name_registered_to_nonterminal_run() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let session = "silverbond-shared-live-name";
        let kill_marker = temp.path().join("shared-name-killed");
        let script = temp.path().join("tmux-shared-live-name.sh");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nmarker=\"$1\"\nshift\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"list-sessions\" ]; then printf '%s\\n' '{}'; exit 0; fi\nif [ \"$1\" = \"kill-session\" ]; then touch \"$marker\"; exit 0; fi\nexit 0\n",
                session
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let invocation = TmuxInvocation {
            prefix: vec![
                script.to_string_lossy().into_owned(),
                kill_marker.to_string_lossy().into_owned(),
            ],
            socket: None,
            tmux_bin: "tmux".to_string(),
        };
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        for (run_id, status) in [
            ("terminal-run", RuntimeStatus::Completed),
            ("nonterminal-run", RuntimeStatus::Running),
        ] {
            let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
            checkpoint.status = status;
            db.upsert_run(&PersistedRun {
                stream_token: new_stream_token(),
                tmux_invocation: Some(invocation.clone()),
                checkpoint,
                workflow: workflow.clone(),
            })
            .await
            .unwrap();
            db.register_tmux_session(run_id, session).await.unwrap();
        }

        reap_stale_tmux_sessions(&RuntimeContext::new(db.clone())).await;

        assert!(
            !kill_marker.exists(),
            "a terminal row must not kill a name also registered to a nonterminal run"
        );
        assert_eq!(
            db.list_run_tmux_session_names("terminal-run")
                .await
                .unwrap(),
            vec![session.to_string()]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_tmux_reaper_retains_registration_when_listing_fails() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let script = temp.path().join("tmux-failed-reaper-list.sh");
        fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let invocation = TmuxInvocation {
            prefix: vec![script.to_string_lossy().into_owned()],
            socket: None,
            tmux_bin: "tmux".to_string(),
        };
        let ctx = RuntimeContext::new(db.clone());
        let run_id = "run_failed_stale_listing";
        let session = "silverbond-unknown-after-list-failure";
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Failed;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: Some(invocation),
            checkpoint,
            workflow,
        })
        .await
        .unwrap();
        db.register_tmux_session(run_id, session).await.unwrap();

        reap_stale_tmux_sessions(&ctx).await;

        assert_eq!(
            db.list_run_tmux_session_names(run_id).await.unwrap(),
            vec![session.to_string()],
            "a failed listing must not reconcile registrations against unknown tmux state"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_tmux_reaper_retains_registration_when_kill_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let session = "silverbond-live-after-rejected-reap";
        let script = temp.path().join("tmux-rejected-reaper-kill.sh");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"list-sessions\" ]; then printf '%s\\n' '{}'; exit 0; fi\nif [ \"$1\" = \"kill-session\" ]; then exit 1; fi\nif [ \"$1\" = \"has-session\" ]; then exit 0; fi\nexit 2\n",
                session
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let invocation = TmuxInvocation {
            prefix: vec![script.to_string_lossy().into_owned()],
            socket: None,
            tmux_bin: "tmux".to_string(),
        };
        let ctx = RuntimeContext::new(db.clone());
        let run_id = "run_rejected_stale_kill";
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Aborted;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: Some(invocation),
            checkpoint,
            workflow,
        })
        .await
        .unwrap();
        db.register_tmux_session(run_id, session).await.unwrap();

        reap_stale_tmux_sessions(&ctx).await;

        assert_eq!(
            db.list_run_tmux_session_names(run_id).await.unwrap(),
            vec![session.to_string()],
            "a rejected kill must retain the row while has-session confirms the session is live"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_tmux_reaper_removes_registration_for_absent_session() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let script = temp.path().join("tmux-empty-list.sh");
        fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"list-sessions\" ]; then exit 0; fi\nexit 2\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let invocation = TmuxInvocation {
            prefix: vec![script.to_string_lossy().into_owned()],
            socket: None,
            tmux_bin: "tmux".to_string(),
        };
        let ctx = RuntimeContext::new(db.clone());
        let run_id = "run_absent_stale_session";
        let session = "silverbond-already-absent";
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Completed;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: Some(invocation),
            checkpoint,
            workflow,
        })
        .await
        .unwrap();
        db.register_tmux_session(run_id, session).await.unwrap();

        reap_stale_tmux_sessions(&ctx).await;

        assert!(
            db.list_run_tmux_session_names(run_id)
                .await
                .unwrap()
                .is_empty(),
            "a successful empty listing should reconcile an already-absent session registration"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_cleanup_retains_registration_when_session_remains_live() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let script = temp.path().join("tmux-terminal-rejected-kill.sh");
        fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"kill-session\" ]; then exit 1; fi\nif [ \"$1\" = \"has-session\" ]; then exit 0; fi\nexit 2\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let invocation = TmuxInvocation {
            prefix: vec![script.to_string_lossy().into_owned()],
            socket: None,
            tmux_bin: "tmux".to_string(),
        };
        let ctx = RuntimeContext {
            run_invocation: Some(invocation.clone()),
            ..RuntimeContext::new(db.clone())
        };
        let run_id = "run_terminal_rejected_kill";
        let session = "silverbond-terminal-still-live";
        let workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        let checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: Some(invocation),
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();
        assert!(db.register_tmux_session(run_id, session).await.unwrap());
        ctx.registry.register(run_id).await;
        ctx.registry
            .set_active_pane_with_session(
                run_id,
                &active_pane_key("cursor", "work"),
                "%terminal-pane",
                Some(session.to_string()),
            )
            .await;

        cleanup_terminal_active_panes(&ctx, run_id, &workflow, &BTreeMap::new()).await;

        assert_eq!(
            db.list_run_tmux_session_names(run_id).await.unwrap(),
            vec![session.to_string()],
            "terminal cleanup must retain a registration while the session is still present"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_tmux_reaper_batches_by_persisted_invocation_without_resolving() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db.clone());

        let socket = "stale-run-socket";
        let first_session = "silverbond-Stale-First";
        let second_session = "silverbond-Stale-Second";
        let script = temp.path().join("tmux-reaper-prefix.sh");
        let log = temp.path().join("tmux-reaper-args.log");
        let resolver_marker = temp.path().join("tmux-reaper-resolver-ran");
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nfirst=\"$2\"\nsecond=\"$3\"\nresolver_marker=\"$4\"\nshift 4\nif [ \"$1\" = \"zsh\" ]; then touch \"$resolver_marker\"; printf 'SBTMUX:tmux\\n'; exit 0; fi\nprintf '%s\\n' \"$@\" >> \"$log\"\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"-L\" ]; then shift 2; fi\nif [ \"$1\" = \"list-sessions\" ]; then printf '%s\\n%s\\n' \"$first\" \"$second\"; fi\nif [ \"$1\" = \"has-session\" ]; then exit 1; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let mut workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        workflow.run_as = Some(model::RunAsConfig {
            command: Some(vec![
                script.to_string_lossy().into_owned(),
                log.to_string_lossy().into_owned(),
                first_session.to_string(),
                second_session.to_string(),
                resolver_marker.to_string_lossy().into_owned(),
            ]),
            user: None,
        });
        let invocation = TmuxInvocation {
            prefix: workflow.run_as.as_ref().unwrap().command.clone().unwrap(),
            socket: Some(socket.to_string()),
            tmux_bin: "tmux".to_string(),
        };
        for (run_id, session) in [
            ("run_stale_reaper_first", first_session),
            ("run_stale_reaper_second", second_session),
        ] {
            let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
            checkpoint.status = RuntimeStatus::Completed;
            db.upsert_run(&PersistedRun {
                stream_token: new_stream_token(),
                tmux_invocation: Some(invocation.clone()),
                checkpoint,
                workflow: workflow.clone(),
            })
            .await
            .unwrap();
            register_tmux_session(&db, run_id, session).await.unwrap();
        }

        reap_stale_tmux_sessions(&ctx).await;

        assert!(
            !resolver_marker.exists(),
            "startup reaper must not resolve persisted tmux invocations"
        );
        let recorded = fs::read_to_string(log).expect("fake tmux prefix should record reaper args");
        let args = recorded.lines().collect::<Vec<_>>();
        assert_eq!(
            args.iter()
                .filter(|argument| **argument == "list-sessions")
                .count(),
            1,
            "shared invocation should be listed once; args={args:?}"
        );
        assert!(
            args.windows(6).any(|window| window
                == [
                    "tmux",
                    "-L",
                    socket,
                    "list-sessions",
                    "-F",
                    "#{session_name}"
                ]),
            "reaper should list sessions under the persisted invocation; args={args:?}"
        );
        assert!(
            args.windows(6)
                .any(|window| window
                    == ["tmux", "-L", socket, "kill-session", "-t", first_session]),
            "reaper should kill first stale session under the persisted invocation; args={args:?}"
        );
        assert!(
            args.windows(6).any(
                |window| window == ["tmux", "-L", socket, "kill-session", "-t", second_session]
            ),
            "reaper should kill second stale session under the persisted invocation; args={args:?}"
        );
        assert!(db.list_reapable_tmux_sessions().await.unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_tmux_reaper_reconstructs_legacy_invocation_without_resolving() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("silverbond.db");
        let db = Database::new(path.clone());
        db.init().await.unwrap();

        let run_id = "run_legacy_stale_reaper";
        let legacy_socket = "legacy-stale-run-socket";
        let run_socket = format!("silverbond-{run_id}");
        let session = "silverbond-Legacy-Stale";
        let script = temp.path().join("legacy-tmux-reaper-prefix.sh");
        let log = temp.path().join("legacy-tmux-reaper-args.log");
        let resolver_marker = temp.path().join("legacy-tmux-reaper-resolver-ran");
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nsession=\"$2\"\nresolver_marker=\"$3\"\nshift 3\nif [ \"$1\" = \"zsh\" ]; then touch \"$resolver_marker\"; printf 'SBTMUX:tmux\\n'; exit 0; fi\nprintf '%s\\n' \"$@\" >> \"$log\"\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"-L\" ]; then shift 2; fi\nif [ \"$1\" = \"list-sessions\" ]; then printf '%s\\n' \"$session\"; fi\nif [ \"$1\" = \"has-session\" ]; then exit 1; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let mut workflow = workflow_from_parts("work", vec![task_node("work", "Work", "")], vec![]);
        workflow.run_as = Some(model::RunAsConfig {
            command: Some(vec![
                script.to_string_lossy().into_owned(),
                log.to_string_lossy().into_owned(),
                session.to_string(),
                resolver_marker.to_string_lossy().into_owned(),
            ]),
            user: None,
        });
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Failed;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint: checkpoint.clone(),
            workflow,
        })
        .await
        .unwrap();

        let mut legacy_state = serde_json::to_value(&checkpoint).unwrap();
        legacy_state["tmuxSessions"] = json!([session]);
        let connection = rusqlite::Connection::open(db.path()).unwrap();
        let workflow_json: String = connection
            .query_row(
                "SELECT workflow_json FROM runs WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .unwrap();
        let mut legacy_workflow: Value = serde_json::from_str(&workflow_json).unwrap();
        legacy_workflow["runAs"]["socket"] = json!(legacy_socket);
        connection
            .execute("DROP TABLE run_tmux_sessions", [])
            .unwrap();
        connection
            .execute(
                "UPDATE runs SET state_json = ?2, workflow_json = ?3 WHERE run_id = ?1",
                rusqlite::params![
                    run_id,
                    serde_json::to_string(&legacy_state).unwrap(),
                    serde_json::to_string(&legacy_workflow).unwrap()
                ],
            )
            .unwrap();
        drop(connection);

        let migrated = Database::new(path);
        migrated.init().await.unwrap();
        assert_eq!(
            migrated
                .list_reapable_tmux_sessions()
                .await
                .unwrap()
                .first()
                .and_then(|reapable| reapable.tmux_invocation.as_ref()),
            None,
            "legacy-backfilled sessions should exercise the missing-invocation path"
        );

        reap_stale_tmux_sessions(&RuntimeContext::new(migrated.clone())).await;

        assert!(
            !resolver_marker.exists(),
            "legacy invocation reconstruction must not resolve tmux"
        );
        let recorded = fs::read_to_string(log).expect("reconstructed invocation should run tmux");
        let args = recorded.lines().collect::<Vec<_>>();
        assert!(
            args.windows(6).any(|window| window
                == [
                    "tmux",
                    "-L",
                    run_socket.as_str(),
                    "list-sessions",
                    "-F",
                    "#{session_name}"
                ]),
            "reaper should list sessions under the reconstructed invocation; args={args:?}"
        );
        assert!(
            args.windows(6).any(|window| window
                == [
                    "tmux",
                    "-L",
                    run_socket.as_str(),
                    "kill-session",
                    "-t",
                    session
                ]),
            "reaper should kill the legacy session under the reconstructed invocation; args={args:?}"
        );
        assert!(
            migrated
                .list_reapable_tmux_sessions()
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn stale_tmux_reaper_is_best_effort_when_storage_is_unavailable() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db.clone());
        let connection = rusqlite::Connection::open(db.path()).unwrap();
        connection
            .execute("DROP TABLE run_tmux_sessions", [])
            .unwrap();

        let outcome: () = reap_stale_tmux_sessions(&ctx).await;

        assert_eq!(outcome, ());
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

    async fn wait_for_event(db: &Database, run_id: &str, kind: &str) -> Vec<RuntimeEvent> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let events = db.list_events(run_id).await.unwrap();
            if events.iter().any(|event| event.kind == kind) {
                return events;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for event {} on run {}",
                kind,
                run_id
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn basic_workflow() -> WorkflowV3 {
        WorkflowV3 {
            version: 4,
            name: Some("test".to_string()),
            goal: "goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: None,
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
                    kind: WorkflowNodeType::Task.into(),
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
                    cwd: None,
                    continue_session_from: None,
                },
                WorkflowNode {
                    id: "n2".to_string(),
                    name: "Step 2".to_string(),
                    kind: WorkflowNodeType::Task.into(),
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
            subflows: BTreeMap::new(),
            ui: None,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_as_decide_oneshot_uses_invocation_prefix_and_socket() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();

        let socket = "silverbond-run_decide";
        let (run_as, log) = fake_run_as_config(&temp, "decide");

        let mut node = task_node("decide", "Decide", "");
        node.kind = NodeKind::Decide {
            decide_config: model::DecideConfig {
                inputs: Vec::new(),
                prompt: "Choose an outcome".to_string(),
                model: None,
                outcomes: vec!["approve".to_string()],
            },
        };
        node.agent = None;

        let mut workflow = workflow_from_parts("decide", vec![node], vec![]);
        workflow.cwd = temp.path().to_string_lossy().into_owned();
        workflow.run_as = Some(run_as);
        let checkpoint = build_initial_checkpoint(&workflow, "run_decide", BTreeMap::new(), None);
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await
        .unwrap();
        let ctx = RuntimeContext::new(db);

        let _ = execute_workflow(ctx, workflow, checkpoint, false).await;
        assert_recorded_run_as_tmux_args(&log, socket);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_as_orchestrator_oneshots_use_invocation_prefix_and_socket() {
        let temp = TempDir::new().unwrap();

        let refinement_socket = "silverbond-run_orchestrator_refinement";
        let (refinement_run_as, refinement_log) =
            fake_run_as_config(&temp, "orchestrator-refinement");
        let refinement_db = Database::new(temp.path().join("orchestrator-refinement.db"));
        refinement_db.init().await.unwrap();
        let mut refinement_workflow = workflow_from_parts(
            "first",
            vec![
                task_node("first", "First", "first prompt"),
                task_node("second", "Second", "second prompt"),
            ],
            vec![success_edge("first_second", "first", "second", None)],
        );
        refinement_workflow.cwd = temp.path().to_string_lossy().into_owned();
        refinement_workflow.use_orchestrator = true;
        refinement_workflow.run_as = Some(refinement_run_as);
        let refinement_checkpoint = build_initial_checkpoint(
            &refinement_workflow,
            "run_orchestrator_refinement",
            BTreeMap::new(),
            None,
        );
        refinement_db
            .upsert_run(&PersistedRun {
                stream_token: new_stream_token(),
                tmux_invocation: None,
                checkpoint: refinement_checkpoint.clone(),
                workflow: refinement_workflow.clone(),
            })
            .await
            .unwrap();
        let refinement_ctx = RuntimeContext::with_runner(
            refinement_db,
            Arc::new(ScriptedRunner::new([(
                "first prompt".to_string(),
                vec![ScriptedStep::success("previous output")],
            )])),
        );
        let _ = execute_workflow(
            refinement_ctx,
            refinement_workflow,
            refinement_checkpoint,
            false,
        )
        .await;
        assert_recorded_run_as_tmux_args(&refinement_log, refinement_socket);

        let branch_socket = "silverbond-orchestrator-branch";
        let (branch_inv, branch_log) = fake_run_as_invocation(&temp, "orchestrator-branch");
        let node = task_node("task", "Task", "prompt");
        let branch_edge = WorkflowEdge {
            id: "edge_branch".to_string(),
            from: "task".to_string(),
            to: "next".to_string(),
            outcome: WorkflowEdgeOutcome::Branch,
            label: Some("Branch A".to_string()),
            branch_id: Some("branch_a".to_string()),
            condition: None,
        };
        let branch_refs = [&branch_edge];
        let branch = run_orchestrator_branch(
            "goal",
            &node,
            "task output",
            &branch_refs,
            &temp.path().to_string_lossy(),
            branch_inv,
        )
        .await;
        assert!(branch.is_err());
        assert_recorded_run_as_tmux_args(&branch_log, branch_socket);
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
    fn pre_snapshot_checkpoint_deserializes_with_legacy_split_members() {
        let workflow = workflow_from_parts(
            "work",
            vec![task_node("work", "Work", "work")],
            Vec::new(),
        );
        let mut checkpoint =
            build_initial_checkpoint(&workflow, "run_legacy_barrier", BTreeMap::new(), None);
        checkpoint.split_families.insert(
            "family".to_string(),
            SplitFamilyState {
                family_id: "family".to_string(),
                split_node_id: "split".to_string(),
                execution_epoch: 1,
                failure_policy: SplitFailurePolicy::BestEffortContinue,
                force_failed: false,
            },
        );
        let barrier_key = CollectorBarrierKey::new("root", "collector", 1);
        checkpoint
            .collector_barriers
            .insert(barrier_key.clone(), CollectorBarrierState::default());

        let mut legacy_checkpoint = serde_json::to_value(checkpoint).unwrap();
        legacy_checkpoint["splitFamilies"]["family"]["memberCursorIds"] =
            json!(["legacy-cursor"]);

        let restored: RuntimeCheckpoint = serde_json::from_value(legacy_checkpoint).unwrap();

        assert!(restored.split_families.contains_key("family"));
        assert_eq!(
            restored.collector_barriers[&barrier_key].representative_snapshot,
            None
        );
    }

    #[test]
    fn output_hash_history_is_capped_to_stagnation_window() {
        let key = "node".to_string();
        let mut hashes = BTreeMap::new();

        assert!(!record_output_hash(&mut hashes, key.clone(), "one"));
        assert!(!record_output_hash(&mut hashes, key.clone(), "two"));
        assert!(!record_output_hash(&mut hashes, key.clone(), "three"));
        assert!(!record_output_hash(&mut hashes, key.clone(), "four"));
        assert_eq!(
            hashes.get(&key).unwrap(),
            &vec![djb2("two"), djb2("three"), djb2("four")]
        );

        let mut stagnant_hashes = BTreeMap::new();
        assert!(!record_output_hash(
            &mut stagnant_hashes,
            key.clone(),
            "same"
        ));
        assert!(!record_output_hash(
            &mut stagnant_hashes,
            key.clone(),
            "same"
        ));
        assert!(record_output_hash(
            &mut stagnant_hashes,
            key.clone(),
            "same"
        ));
        assert_eq!(stagnant_hashes.get(&key).unwrap().len(), STAGNATION_WINDOW);
    }

    #[test]
    fn resolves_template_vars() {
        let node = WorkflowNode {
            id: "n2".to_string(),
            name: "Step 2".to_string(),
            kind: WorkflowNodeType::Task.into(),
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

    #[test]
    fn read_batch_items_resolves_node_output_path_parsed_output_path_and_var_binding() {
        let node = parallel_batch_node("batch", "unused", 2, "item", "body", None);
        let inbound_map = HashMap::new();
        let mut results = BTreeMap::new();
        results.insert(
            "plan".to_string(),
            NodeResult {
                success: true,
                output: json!({"tasks": ["from-output-a", "from-output-b"]}).to_string(),
                parsed_output: Some(json!({"tasks": ["from-parsed-a", "from-parsed-b"]})),
                ..Default::default()
            },
        );
        let mut vars = BTreeMap::new();
        vars.insert(
            "items".to_string(),
            json!(["from-var-a", "from-var-b"]).to_string(),
        );
        let context = TemplateRuntimeContext {
            current_node_id: &node.id,
            current_node: &node,
            all_results: &results,
            last_output: "",
            var_map: &vars,
            inbound_map: &inbound_map,
            last_branch_origin_id: None,
            last_branch_choice: None,
        };

        assert_eq!(
            read_batch_items(&context, "node:plan.output.tasks").unwrap(),
            vec![json!("from-output-a"), json!("from-output-b")]
        );
        assert_eq!(
            read_batch_items(&context, "node:plan.parsedOutput.tasks").unwrap(),
            vec![json!("from-parsed-a"), json!("from-parsed-b")]
        );
        assert_eq!(
            read_batch_items(&context, "var:items").unwrap(),
            vec![json!("from-var-a"), json!("from-var-b")]
        );
    }

    #[tokio::test]
    async fn active_pane_registry_keeps_duplicate_node_ids_per_cursor() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db);
        let run_id = "run_active_panes";
        ctx.registry.register(run_id).await;

        let first_key = active_pane_key("cursor-a", "shared");
        let second_key = active_pane_key("cursor-b", "shared");
        ctx.registry
            .set_active_pane(run_id, &first_key, "%pane-a")
            .await;
        ctx.registry
            .set_active_pane(run_id, &second_key, "%pane-b")
            .await;

        assert_eq!(
            ctx.registry.resolve_active_pane(run_id, &first_key).await,
            Some("%pane-a".to_string())
        );
        assert_eq!(
            ctx.registry.resolve_active_pane(run_id, &second_key).await,
            Some("%pane-b".to_string())
        );
        assert_eq!(
            ctx.registry.resolve_active_pane(run_id, "shared").await,
            Some("%pane-b".to_string())
        );
        assert_eq!(
            ctx.registry.resolve_active_pane(run_id, "active").await,
            None
        );

        let targets = ctx
            .registry
            .active_pane_targets_for_keys(run_id, &HashSet::from(["shared".to_string()]))
            .await;
        assert_eq!(targets, vec!["%pane-a".to_string(), "%pane-b".to_string()]);

        ctx.registry.clear_active_pane(run_id, &first_key).await;
        assert_eq!(
            ctx.registry.resolve_active_pane(run_id, "shared").await,
            Some("%pane-b".to_string())
        );
        ctx.registry.clear_active_pane(run_id, &second_key).await;
        assert_eq!(
            ctx.registry.resolve_active_pane(run_id, "shared").await,
            None
        );
    }

    #[tokio::test]
    async fn tmux_target_ownership_is_run_scoped_and_separate_from_active_panes() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db);
        let owning_run = "run-owner";
        let foreign_run = "run-foreign";
        ctx.registry.register(owning_run).await;
        ctx.registry.register(foreign_run).await;

        ctx.registry
            .register_owned_tmux_target(owning_run, "%owned-pane", Some("owned-session"))
            .await;
        ctx.registry
            .set_active_pane(
                owning_run,
                &active_pane_key("cursor", "attacker-controlled"),
                "%active-but-unowned-pane",
            )
            .await;

        assert!(ctx.registry.owns_tmux_pane(owning_run, "%owned-pane").await);
        assert!(
            ctx.registry
                .owns_tmux_session(owning_run, "owned-session")
                .await
        );
        assert!(
            !ctx.registry
                .owns_tmux_pane(owning_run, "%active-but-unowned-pane")
                .await,
            "an active-pane alias must not establish ownership"
        );
        assert!(
            !ctx.registry
                .owns_tmux_pane(foreign_run, "%owned-pane")
                .await
        );
        assert!(
            !ctx.registry
                .owns_tmux_session(foreign_run, "owned-session")
                .await
        );
    }

    #[tokio::test]
    async fn terminal_cleanup_targets_skip_persistent_panes_and_reused_aliases() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::new(db);
        let run_id = "run_cleanup_targets";
        ctx.registry.register(run_id).await;

        ctx.registry
            .set_active_pane_with_session(
                run_id,
                &active_pane_key("cursor", "source"),
                "%pane-source",
                Some("session-source".to_string()),
            )
            .await;
        ctx.registry
            .set_active_pane(run_id, &active_pane_key("cursor", "reused"), "%pane-source")
            .await;
        ctx.registry
            .set_active_pane_with_session(
                run_id,
                &active_pane_key("cursor", "other"),
                "%pane-other",
                Some("session-other".to_string()),
            )
            .await;

        let targets = ctx
            .registry
            .active_pane_cleanup_targets_except_keys(run_id, &HashSet::from(["source".to_string()]))
            .await;

        assert_eq!(
            targets,
            vec![crate::tmux_exec::PaneCleanupTarget::new(
                "%pane-other".to_string(),
                Some("session-other".to_string())
            )]
        );
    }

    #[tokio::test]
    async fn parallel_batch_active_panes_are_registered_per_child_cursor() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runner = PaneTrackingRunner::new(2);
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(runner.clone()));
        let mut workflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 2, "item", "body", None),
                task_node("body", "Body", "work {{var:item}}"),
            ],
            Vec::new(),
        );
        workflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: "[]".to_string(),
        }];
        let mut vars = BTreeMap::new();
        vars.insert("items".to_string(), json!(["a", "b"]).to_string());

        let run_id = runtime.start_run(workflow, vars, None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), runner.registered.wait())
            .await
            .expect("timed out waiting for batch panes to register");

        let registrations = runner.registrations.lock().await.clone();
        assert_eq!(registrations.len(), 2);
        let cursor_ids = registrations
            .iter()
            .map(|(cursor_id, _, _)| cursor_id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(cursor_ids.len(), 2);
        for (_, key, pane) in &registrations {
            assert!(key.ends_with(":body"));
            assert_eq!(
                runtime.registry.resolve_active_pane(&run_id, key).await,
                Some(pane.clone())
            );
        }

        let targets = runtime.registry.active_pane_targets(&run_id).await;
        assert_eq!(targets.len(), 2);
        let by_node = runtime
            .registry
            .resolve_active_pane(&run_id, "body")
            .await
            .expect("body node should resolve to one active pane");
        assert!(targets.contains(&by_node));

        tokio::time::timeout(Duration::from_secs(1), runner.release.wait())
            .await
            .expect("timed out releasing batch panes");
        let persisted = wait_for_terminal_run(&db, &run_id).await;
        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert!(
            runtime
                .registry
                .active_pane_targets(&run_id)
                .await
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn completed_run_cleans_non_persistent_active_panes() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let (invocation, kill_log, kill_marker) = fake_tmux_kill_invocation(&temp);
        let mut runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(PersistentPaneRunner::new("%completed-pane")),
        );
        runtime.run_invocation = Some(invocation);

        let workflow = workflow_from_parts(
            "work",
            vec![task_node("work", "Work", "complete work")],
            Vec::new(),
        );
        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        wait_for_path(&kill_marker).await;
        wait_for_registry_empty(&runtime.registry, &run_id).await;
        let recorded =
            fs::read_to_string(kill_log).expect("fake tmux should record completion cleanup");
        assert!(recorded.contains("kill-pane"));
        assert!(recorded.contains("%completed-pane"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_backstop_run_cleans_non_persistent_active_panes() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let (invocation, kill_log, kill_marker) = fake_tmux_kill_invocation(&temp);
        let mut runtime = RuntimeContext::new(db.clone());
        runtime.run_invocation = Some(invocation);
        let workflow = workflow_from_parts(
            "backstop",
            vec![task_node("work", "Work", "register pane")],
            Vec::new(),
        );
        let run_id = "run_failed_cleanup";
        let checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        runtime.registry.register(run_id).await;
        runtime
            .registry
            .set_active_pane(run_id, &active_pane_key("cursor", "work"), "%failed-pane")
            .await;

        fail_workflow_after_error(
            &runtime,
            workflow,
            checkpoint,
            run_id,
            anyhow::anyhow!("forced workflow failure"),
            Duration::from_millis(1),
        )
        .await
        .unwrap();
        let persisted = db
            .get_run(run_id)
            .await
            .unwrap()
            .expect("failed run should be persisted");

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Failed);
        wait_for_path(&kill_marker).await;
        wait_for_registry_empty(&runtime.registry, &run_id).await;
        let recorded =
            fs::read_to_string(kill_log).expect("fake tmux should record failed cleanup");
        assert!(recorded.contains("kill-pane"));
        assert!(recorded.contains("%failed-pane"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fail_workflow_backstop_cleans_registry_on_prelude_persistence_error() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("silverbond.db");
        let db = Database::new(db_path.clone());
        db.init().await.unwrap();
        let (invocation, _kill_log, kill_marker) = fake_tmux_kill_invocation(&temp);
        let mut runtime = RuntimeContext::new(db.clone());
        runtime.run_invocation = Some(invocation);
        let workflow = workflow_from_parts(
            "backstop_prelude_err",
            vec![task_node("work", "Work", "register pane")],
            Vec::new(),
        );
        let run_id = "run_backstop_prelude_err";
        let checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        runtime.registry.register(run_id).await;
        runtime
            .registry
            .set_active_pane(run_id, &active_pane_key("cursor", "work"), "%prelude-err-pane")
            .await;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: runtime.run_invocation.clone(),
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        {
            use rusqlite::Connection;
            let conn = Connection::open(&db_path).unwrap();
            conn.execute("DROP TABLE run_events", []).unwrap();
            conn.execute("DROP TABLE logs", []).unwrap();
        }

        let result = fail_workflow_after_error(
            &runtime,
            workflow,
            checkpoint,
            run_id,
            anyhow::anyhow!("forced workflow failure"),
            Duration::from_millis(1),
        )
        .await;
        assert!(
            result.is_err(),
            "expected finalize persistence to fail after best-effort prelude: {result:?}"
        );
        wait_for_path(&kill_marker).await;
        assert!(
            !runtime.registry.active_run_ids().await.contains(run_id),
            "backstop should clear registry even when prelude persistence fails"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn finalize_run_cleans_registry_on_persistence_error() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("silverbond.db");
        let db = Database::new(db_path.clone());
        db.init().await.unwrap();
        let (invocation, _kill_log, kill_marker) = fake_tmux_kill_invocation(&temp);
        let mut runtime = RuntimeContext::new(db.clone());
        runtime.run_invocation = Some(invocation);
        let workflow = workflow_from_parts(
            "finalize_err",
            vec![task_node("work", "Work", "register pane")],
            Vec::new(),
        );
        let run_id = "run_finalize_persist_err";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Completed;
        runtime.registry.register(run_id).await;
        runtime
            .registry
            .set_active_pane(run_id, &active_pane_key("cursor", "work"), "%err-pane")
            .await;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: runtime.run_invocation.clone(),
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        {
            use rusqlite::Connection;
            let conn = Connection::open(&db_path).unwrap();
            conn.execute("DROP TABLE logs", []).unwrap();
        }

        let result = finalize_run(
            &runtime,
            &workflow,
            checkpoint,
            Duration::from_millis(1),
        )
        .await;
        assert!(result.is_err(), "expected persistence to fail: {result:?}");
        wait_for_path(&kill_marker).await;
        assert!(
            !runtime.registry.active_run_ids().await.contains(run_id),
            "registry entry should be cleared even when persistence fails"
        );
    }

    #[tokio::test]
    async fn resume_warns_when_pending_approval_has_no_waiting_cursor() {
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
            "approve",
            vec![
                approval_node("gate", "Gate", "approve?"),
                task_node("after", "After", "after"),
            ],
            vec![success_edge("gate_after", "gate", "after", None)],
        );
        let run_id = "run_inconsistent_approval";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Paused;
        checkpoint.pending_approval = Some(PendingApproval {
            cursor_id: "cursor-1".to_string(),
            node_id: "gate".to_string(),
            node_name: "Gate".to_string(),
            prompt: "approve?".to_string(),
            last_output: String::new(),
        });
        checkpoint.active_cursors = vec![CursorState {
            cursor_id: "cursor-1".to_string(),
            node_id: "gate".to_string(),
            execution_epoch: checkpoint.execution_epoch,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: String::new(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            var_map: BTreeMap::new(),
            call_stack: Vec::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::Runnable,
        }];
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();
        let events = wait_for_event(&db, run_id, "workflow_warn").await;
        let warn = events
            .iter()
            .find(|event| event.kind == "workflow_warn")
            .expect("expected workflow_warn when pending approval disagrees with cursor state");
        assert_eq!(warn.data.get("nodeId").and_then(Value::as_str), Some("gate"));
        assert_eq!(
            warn.data.get("cursorId").and_then(Value::as_str),
            Some("cursor-1")
        );
    }

    #[tokio::test]
    async fn finalize_run_distinct_logs_for_same_workflow_same_second() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let workflow = workflow_from_parts("wf", vec![task_node("work", "Work", "")], vec![]);
        for run_id in ["run_log_collision_a", "run_log_collision_b"] {
            let mut checkpoint =
                build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
            checkpoint.status = RuntimeStatus::Completed;
            checkpoint.active_cursors.clear();
            finalize_run(
                &runtime,
                &workflow,
                checkpoint,
                Duration::from_millis(1),
            )
            .await
            .unwrap();
        }
        let logs = db.list_logs().await.unwrap();
        assert_eq!(logs.len(), 2, "expected two distinct history rows");
        assert_ne!(logs[0].id, logs[1].id);
        let run_ids: BTreeSet<String> = logs
            .iter()
            .filter_map(|log| log.run_id.clone())
            .collect();
        assert_eq!(
            run_ids,
            BTreeSet::from([
                "run_log_collision_a".to_string(),
                "run_log_collision_b".to_string()
            ])
        );
    }

    #[test]
    fn build_log_id_derives_from_run_id() {
        let id = build_log_id(
            "My Flow!",
            "run_018f1234-5678-7abc-def0-123456789abc",
        );
        assert_eq!(
            id,
            "My Flow__018f1234-5678-7abc-def0-123456789abc"
        );
        assert!(crate::util::safe_name(&id).is_ok());
    }

    fn waiting_approval_cursor(
        checkpoint: &RuntimeCheckpoint,
        cursor_id: &str,
        node_id: &str,
    ) -> CursorState {
        CursorState {
            cursor_id: cursor_id.to_string(),
            node_id: node_id.to_string(),
            execution_epoch: checkpoint.execution_epoch,
            parent_cursor_id: None,
            incoming_edge_id: None,
            incoming_node_id: None,
            split_family_ids: Vec::new(),
            last_output: String::new(),
            loop_counters: BTreeMap::new(),
            visit_counters: BTreeMap::new(),
            var_map: BTreeMap::new(),
            call_stack: Vec::new(),
            last_branch_origin_id: None,
            last_branch_choice: None,
            cancel_requested: false,
            state: CursorRuntimeState::WaitingApproval,
        }
    }

    #[tokio::test]
    async fn restore_pending_approval_fallback_binds_rehydrated_cursor_once() {
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
            "approve",
            vec![
                approval_node("gate", "Gate", "approve?"),
                task_node("after", "After", "after"),
            ],
            vec![success_edge("gate_after", "gate", "after", None)],
        );
        let run_id = "run_restore_fallback_bind";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Paused;
        checkpoint.pending_approval = Some(PendingApproval {
            cursor_id: "stale-cursor-id".to_string(),
            node_id: "gate".to_string(),
            node_name: "Gate".to_string(),
            prompt: "approve?".to_string(),
            last_output: String::new(),
        });
        checkpoint.current_node_id = Some("gate".to_string());
        checkpoint.current_node_name = Some("Gate".to_string());
        checkpoint.active_cursors.clear();
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();
        let events = wait_for_event(&db, run_id, "approval_required").await;
        let required = events
            .iter()
            .find(|event| event.kind == "approval_required")
            .expect("expected approval_required after fallback bind");
        let persisted = wait_for_run(&db, run_id, |persisted| {
            persisted
                .checkpoint
                .active_cursors
                .iter()
                .any(|cursor| cursor.state == CursorRuntimeState::WaitingApproval)
        })
        .await;
        let bound_cursor_id = persisted
            .checkpoint
            .active_cursors
            .iter()
            .find(|cursor| cursor.state == CursorRuntimeState::WaitingApproval)
            .map(|cursor| cursor.cursor_id.as_str())
            .expect("expected waiting cursor after rehydration");
        assert_eq!(
            required.data.get("cursorId").and_then(Value::as_str),
            Some(bound_cursor_id)
        );
        assert_eq!(
            required.data.get("nodeId").and_then(Value::as_str),
            Some("gate")
        );

        runtime
            .approve_run(run_id, true, String::new())
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, run_id).await;
        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted
                .checkpoint
                .all_results
                .get("gate")
                .map(|result| result.agent.as_str()),
            Some("user")
        );
        let approval_events = db.list_events(run_id).await.unwrap();
        assert_eq!(
            approval_events
                .iter()
                .filter(|event| event.kind == "approval_required")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn restore_pending_approval_fallback_drops_on_node_id_mismatch() {
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
            "approve",
            vec![
                approval_node("gate", "Gate", "approve?"),
                task_node("after", "After", "after"),
            ],
            vec![success_edge("gate_after", "gate", "after", None)],
        );
        let run_id = "run_restore_fallback_mismatch";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Paused;
        checkpoint.pending_approval = Some(PendingApproval {
            cursor_id: "stale-cursor-id".to_string(),
            node_id: "gate".to_string(),
            node_name: "Gate".to_string(),
            prompt: "approve?".to_string(),
            last_output: String::new(),
        });
        checkpoint.active_cursors = vec![waiting_approval_cursor(
            &checkpoint,
            "other-cursor",
            "after",
        )];
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();
        let events = wait_for_event(&db, run_id, "workflow_warn").await;
        assert!(
            events.iter().any(|event| event.kind == "workflow_warn"),
            "expected workflow_warn when fallback cannot bind"
        );
        assert!(
            !events.iter().any(|event| event.kind == "approval_required"),
            "must not emit approval_required for mismatched node"
        );
    }

    #[tokio::test]
    async fn restore_pending_approval_fallback_drops_when_cursor_has_queued_approval() {
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
            "approve",
            vec![
                approval_node("gate", "Gate", "approve?"),
                task_node("after", "After", "after"),
            ],
            vec![success_edge("gate_after", "gate", "after", None)],
        );
        let run_id = "run_restore_fallback_queued_owner";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Paused;
        checkpoint.pending_approval = Some(PendingApproval {
            cursor_id: "stale-cursor-id".to_string(),
            node_id: "gate".to_string(),
            node_name: "Gate".to_string(),
            prompt: "approve?".to_string(),
            last_output: String::new(),
        });
        checkpoint.active_cursors = vec![waiting_approval_cursor(
            &checkpoint,
            "owned-cursor",
            "gate",
        )];
        checkpoint.queued_approvals.push(QueuedApproval {
            approval: PendingApproval {
                cursor_id: "owned-cursor".to_string(),
                node_id: "gate".to_string(),
                node_name: "Gate".to_string(),
                prompt: "approve?".to_string(),
                last_output: String::new(),
            },
        });
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();
        let events = wait_for_event(&db, run_id, "workflow_warn").await;
        assert!(
            events.iter().any(|event| event.kind == "workflow_warn"),
            "cursor with its own queued approval must not be rebound via fallback"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn abort_kills_active_panes_before_draining_running_tasks() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let (invocation, kill_log, kill_marker) = fake_tmux_kill_invocation(&temp);
        let runner = AbortBlockingRunner::new("%abort-pane", kill_marker.clone());
        let mut runtime = RuntimeContext::with_runner(db.clone(), Arc::new(runner.clone()));
        runtime.run_invocation = Some(invocation);

        let mut node = task_node("work", "Work", "long work");
        node.timeout = Some(60);
        let workflow = workflow_from_parts("work", vec![node], Vec::new());
        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), runner.registered.wait())
            .await
            .expect("timed out waiting for active pane registration");

        let started = Instant::now();
        runtime.abort_run(&run_id).await.unwrap();
        let persisted = tokio::time::timeout(
            TMUX_CLEANUP_TEST_TIMEOUT,
            wait_for_terminal_run(&db, &run_id),
        )
        .await
        .expect("abort did not reach a terminal state promptly");

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Aborted);
        assert!(
            started.elapsed() < TMUX_CLEANUP_TEST_TIMEOUT,
            "abort waited too long: {:?}",
            started.elapsed()
        );
        assert!(kill_marker.exists());
        let recorded =
            fs::read_to_string(kill_log).expect("fake tmux should record abort cleanup arguments");
        assert!(recorded.contains("kill-pane"));
        assert!(recorded.contains("%abort-pane"));
    }

    #[test]
    fn decide_outcome_selection_prefers_exact_then_substring() {
        let outcomes = vec!["approve".to_string(), "revise".to_string()];

        assert_eq!(
            select_decide_outcome(&outcomes, "revise", None),
            decide_matched("revise")
        );
        assert_eq!(
            select_decide_outcome(&outcomes, "I would approve this path.", None),
            decide_matched("approve")
        );
        assert_eq!(
            select_decide_outcome(&outcomes, "unknown", None),
            decide_unmatched()
        );
    }

    #[tokio::test]
    async fn split_spawns_collects_and_continues() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "branch-a".to_string(),
                    vec![ScriptedStep::success("alpha output").with_delay(60)],
                ),
                (
                    "branch-b".to_string(),
                    vec![ScriptedStep::success("beta output").with_delay(5)],
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
        assert_eq!(parsed["summary"]["total"], json!(2));
        assert_eq!(parsed["summary"]["succeeded"], json!(2));
        assert_eq!(parsed["inputs"]["alpha"]["output"], json!("alpha output"));
        assert_eq!(parsed["inputs"]["beta"]["output"], json!("beta output"));

        let events = db.list_events(&run_id).await.unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "cursor_spawned")
                .count(),
            2
        );
        assert!(events.iter().any(|event| event.kind == "aggregate_merged"));
        assert!(
            events
                .iter()
                .any(|event| event.kind == "collector_released")
        );
        assert!(
            events
                .iter()
                .filter(|event| event.kind == "transition")
                .all(|event| event.data.contains_key("cursorId"))
        );
    }

    #[tokio::test]
    async fn split_and_collector_maps_evict_after_completion_without_restart() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "branch-a".to_string(),
                    vec![ScriptedStep::success("alpha output").with_delay(30)],
                ),
                (
                    "branch-b".to_string(),
                    vec![ScriptedStep::success("beta output").with_delay(5)],
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
        let after_collector = wait_for_run(&db, &run_id, |persisted| {
            persisted.checkpoint.all_results.contains_key("collector")
                && persisted.checkpoint.collector_barriers.is_empty()
                && persisted.checkpoint.split_families.is_empty()
        })
        .await;
        assert!(after_collector.checkpoint.split_families.is_empty());
        assert!(after_collector.checkpoint.collector_barriers.is_empty());

        let persisted = wait_for_terminal_run(&db, &run_id).await;
        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert!(persisted.checkpoint.split_families.is_empty());
        assert!(persisted.checkpoint.collector_barriers.is_empty());
    }

    #[tokio::test]
    async fn parallel_batch_fans_out_three_items_with_cap_two() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "work a".to_string(),
                    vec![ScriptedStep::success("done a").with_delay(20)],
                ),
                (
                    "work b".to_string(),
                    vec![ScriptedStep::success("done b").with_delay(20)],
                ),
                (
                    "work c".to_string(),
                    vec![ScriptedStep::success("done c").with_delay(20)],
                ),
                (
                    "after".to_string(),
                    vec![ScriptedStep::success("after done")],
                ),
            ])),
        );
        let mut workflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 2, "item", "body", Some("batch_results")),
                task_node("body", "Body", "work {{var:item}}"),
                task_node("after", "After", "after"),
            ],
            vec![success_edge("batch_after", "batch", "after", None)],
        );
        workflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: "[]".to_string(),
        }];
        let mut vars = BTreeMap::new();
        vars.insert("items".to_string(), json!(["a", "b", "c"]).to_string());

        let run_id = runtime.start_run(workflow, vars, None).await.unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert!(persisted.checkpoint.all_results.contains_key("after"));
        let batch = persisted.checkpoint.all_results.get("batch").unwrap();
        let parsed = batch.parsed_output.as_ref().unwrap();
        assert_eq!(parsed["summary"]["total"], json!(3));
        assert_eq!(parsed["summary"]["succeeded"], json!(3));
        assert_eq!(parsed["summary"]["maxConcurrent"], json!(2));

        let collected = persisted
            .checkpoint
            .var_map
            .get("batch_results")
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .unwrap();
        assert_eq!(collected.as_array().unwrap().len(), 3);
    }

    #[cfg(unix)]
    fn fake_tmux_blocking_oneshot_invocation(temp: &TempDir) -> (TmuxInvocation, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("tmux-decide-prefix.sh");
        let log = temp.path().join("tmux-decide-args.log");
        let marker = temp.path().join("tmux-decide-killed");
        fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nmarker=\"$2\"\nshift 2\nprintf '%s\\n' \"$@\" >> \"$log\"\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"-L\" ]; then shift 2; fi\ncase \"$1\" in\n  new-session)\n    printf '%%decide-pane\\n'\n    ;;\n  capture-pane)\n    printf '\\n'\n    ;;\n  kill-session|kill-pane)\n    touch \"$marker\"\n    ;;\nesac\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        (
            TmuxInvocation {
                prefix: vec![
                    script.to_string_lossy().into_owned(),
                    log.to_string_lossy().into_owned(),
                    marker.to_string_lossy().into_owned(),
                ],
                socket: Some("decide-abort-socket".to_string()),
                tmux_bin: "tmux".to_string(),
            },
            log,
            marker,
        )
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn decide_abort_returns_promptly_and_kills_pane() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let (invocation, kill_log, kill_marker) = fake_tmux_blocking_oneshot_invocation(&temp);
        let mut runtime = RuntimeContext::with_runner(db.clone(), Arc::new(EchoRunner));
        runtime.run_invocation = Some(invocation);

        let mut decide = task_node("decide", "Decide", "");
        decide.kind = NodeKind::Decide {
            decide_config: model::DecideConfig {
                inputs: Vec::new(),
                prompt: "Choose an outcome".to_string(),
                model: None,
                outcomes: vec!["approve".to_string()],
            },
        };
        decide.agent = None;

        let workflow = workflow_from_parts("decide", vec![decide], vec![]);
        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_event(&db, &run_id, "node_start").await;

        let abort_started = Instant::now();
        runtime.abort_run(&run_id).await.unwrap();
        let persisted =
            tokio::time::timeout(Duration::from_secs(2), wait_for_terminal_run(&db, &run_id))
                .await
                .expect("decide abort did not reach a terminal state promptly");

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Aborted);
        assert!(
            abort_started.elapsed() < Duration::from_millis(500),
            "decide abort waited too long: {:?}",
            abort_started.elapsed()
        );
        wait_for_path(&kill_marker).await;
        let recorded =
            fs::read_to_string(kill_log).expect("fake tmux should record decide abort cleanup");
        assert!(
            recorded.contains("kill-session") || recorded.contains("kill-pane"),
            "decide abort should kill the oneshot pane; args={recorded}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn orchestrator_refinement_err_falls_back_to_original_prompt() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let (run_as, _) = fake_run_as_config(&temp, "orch-fail");
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "first prompt".to_string(),
                    vec![ScriptedStep::success("previous output")],
                ),
                (
                    "second prompt".to_string(),
                    vec![ScriptedStep::success("second done")],
                ),
            ])),
        );

        let mut workflow = workflow_from_parts(
            "first",
            vec![
                task_node("first", "First", "first prompt"),
                task_node("second", "Second", "second prompt"),
            ],
            vec![success_edge("first_second", "first", "second", None)],
        );
        workflow.use_orchestrator = true;
        workflow.cwd = temp.path().to_string_lossy().into_owned();
        workflow.run_as = Some(run_as);

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted.checkpoint.all_results["second"].output,
            "second done"
        );
        let events = db.list_events(&run_id).await.unwrap();
        assert!(events.iter().any(|event| event.kind == "orchestrator_warn"));
        assert!(!events.iter().any(|event| event.kind == "workflow_error"));
    }

    #[tokio::test]
    async fn parallel_batch_abort_cancels_pending_items_after_next_completion() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "work a".to_string(),
                    vec![ScriptedStep::success("done a").with_delay(2000)],
                ),
                (
                    "work b".to_string(),
                    vec![ScriptedStep::success("done b").with_delay(300)],
                ),
                (
                    "work c".to_string(),
                    vec![ScriptedStep::success("done c").with_delay(300)],
                ),
                (
                    "work d".to_string(),
                    vec![ScriptedStep::success("done d").with_delay(300)],
                ),
            ])),
        );
        let mut workflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 1, "item", "body", None),
                task_node("body", "Body", "work {{var:item}}"),
            ],
            Vec::new(),
        );
        workflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: "[]".to_string(),
        }];
        let mut vars = BTreeMap::new();
        vars.insert("items".to_string(), json!(["a", "b", "c", "d"]).to_string());

        let run_id = runtime.start_run(workflow, vars, None).await.unwrap();
        wait_for_event(&db, &run_id, "cursor_spawned").await;

        let abort_started = Instant::now();
        runtime.abort_run(&run_id).await.unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Aborted);
        assert!(
            abort_started.elapsed() < Duration::from_millis(500),
            "parallel_batch abort waited for the whole batch: {:?}",
            abort_started.elapsed()
        );
        assert!(!persisted.checkpoint.all_results.contains_key("after"));
        let batch = persisted.checkpoint.all_results.get("batch").unwrap();
        let parsed = batch.parsed_output.as_ref().unwrap();
        assert_eq!(parsed["summary"]["total"], json!(4));
        assert_eq!(parsed["summary"]["succeeded"], json!(0));
        assert_eq!(parsed["summary"]["cancelled"], json!(4));
        assert_eq!(
            parsed["items"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["cancelled"] == json!(true))
                .count(),
            4
        );

        let events = db.list_events(&run_id).await.unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "cursor_spawned")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn parallel_batch_resume_skips_checkpointed_item_results() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                ("work b".to_string(), vec![ScriptedStep::success("done b")]),
                ("work c".to_string(), vec![ScriptedStep::success("done c")]),
                (
                    "after".to_string(),
                    vec![ScriptedStep::success("after done")],
                ),
            ])),
        );
        let mut workflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 1, "item", "body", None),
                task_node("body", "Body", "work {{var:item}}"),
                task_node("after", "After", "after"),
            ],
            vec![success_edge("batch_after", "batch", "after", None)],
        );
        workflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: "[]".to_string(),
        }];
        let mut vars = BTreeMap::new();
        vars.insert("items".to_string(), json!(["a", "b", "c"]).to_string());
        let run_id = "run_batch_resume";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, vars, None);
        let batch_key = parallel_batch_checkpoint_key(&checkpoint.active_cursors[0], "batch");
        checkpoint.batch_item_results.insert(
            batch_key,
            BTreeMap::from([(
                0,
                json!({
                    "index": 0,
                    "item": "a",
                    "cursorId": "completed-a",
                    "nodeId": "body",
                    "success": true,
                    "output": "done a",
                    "stderr": "",
                    "exitCode": 0,
                    "parsedOutput": Value::Null,
                }),
            )]),
        );
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: None,
            checkpoint,
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        runtime.resume_run(run_id).await.unwrap();
        let persisted = wait_for_terminal_run(&db, run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert!(persisted.checkpoint.batch_item_results.is_empty());
        assert!(persisted.checkpoint.all_results.contains_key("after"));
        let batch = persisted.checkpoint.all_results.get("batch").unwrap();
        let items = batch.parsed_output.as_ref().unwrap()["items"]
            .as_array()
            .unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["output"], json!("done a"));
        assert_eq!(items[1]["output"], json!("done b"));
        assert_eq!(items[2]["output"], json!("done c"));

        let events = db.list_events(run_id).await.unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "cursor_spawned")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn parallel_batch_item_task_error_fails_run_and_emits_done() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([(
                "work a".to_string(),
                vec![ScriptedStep::success("done a")],
            )])),
        );
        let mut workflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 2, "item", "body", None),
                task_node("body", "Body", "work {{var:item}}"),
            ],
            Vec::new(),
        );
        workflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: "[]".to_string(),
        }];
        let mut vars = BTreeMap::new();
        vars.insert("items".to_string(), json!(["a", "b"]).to_string());

        let run_id = runtime.start_run(workflow, vars, None).await.unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Failed);
        let batch = persisted.checkpoint.all_results.get("batch").unwrap();
        let parsed = batch.parsed_output.as_ref().unwrap();
        assert_eq!(parsed["summary"]["total"], json!(2));
        assert_eq!(parsed["summary"]["failed"], json!(1));
        let failed_item = parsed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["success"] == json!(false))
            .unwrap();
        assert!(
            failed_item["stderr"]
                .as_str()
                .unwrap()
                .contains("No scripted step for prompt")
        );

        let events = wait_for_event(&db, &run_id, "done").await;
        assert!(events.iter().any(|event| event.kind == "done"));
    }

    #[tokio::test]
    async fn workflow_error_backstop_marks_run_failed_and_emits_done() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(EchoRunner));
        let workflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node(
                    "batch",
                    "node:missing.output.tasks",
                    2,
                    "item",
                    "body",
                    None,
                ),
                task_node("body", "Body", "work {{var:item}}"),
            ],
            Vec::new(),
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Failed);
        let events = wait_for_event(&db, &run_id, "done").await;
        assert!(events.iter().any(|event| event.kind == "workflow_error"));
        assert!(events.iter().any(|event| event.kind == "done"));
    }

    #[tokio::test]
    async fn parallel_batch_collector_var_inside_subflow_is_scoped_to_cursor() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(EchoRunner));
        let mut subflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 2, "item", "body", Some("batch_results")),
                task_node("body", "Body", "work {{var:item}}"),
                task_node("check", "Check", "collected {{var:batch_results}}"),
            ],
            vec![success_edge("batch_check", "batch", "check", None)],
        );
        subflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: json!(["a", "b"]).to_string(),
        }];
        let mut workflow = workflow_from_parts(
            "call",
            vec![
                call_node("call", "batch_subflow", "check", Vec::new()),
                task_node("after", "After", "root sees {{var:batch_results}}"),
            ],
            vec![success_edge("call_after", "call", "after", None)],
        );
        workflow
            .subflows
            .insert("batch_subflow".to_string(), Box::new(subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        let call_output = &persisted.checkpoint.all_results["call"].output;
        let collected_json = call_output.strip_prefix("collected ").unwrap();
        let collected = serde_json::from_str::<Value>(collected_json).unwrap();
        let collected_items = collected.as_array().unwrap();
        assert_eq!(collected_items.len(), 2);
        assert_eq!(collected_items[0]["item"], json!("a"));
        assert_eq!(collected_items[0]["output"], json!("work a"));
        assert_eq!(collected_items[1]["item"], json!("b"));
        assert_eq!(collected_items[1]["output"], json!("work b"));
        assert!(!persisted.checkpoint.var_map.contains_key("batch_results"));
        assert_eq!(
            persisted.checkpoint.all_results["after"].output,
            "root sees {{var:batch_results}}"
        );
    }

    #[tokio::test]
    async fn continue_session_from_inside_subflow_keeps_source_pane_active() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runner = ContinuationPaneRunner::new();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(runner.clone()));

        let source = task_node("source", "Source", "source prompt");
        let mut continuation = task_node("continuation", "Continuation", "continue prompt");
        continuation.continue_session_from = Some("source".to_string());
        let subflow = workflow_from_parts(
            "source",
            vec![source, continuation],
            vec![success_edge(
                "source_continuation",
                "source",
                "continuation",
                None,
            )],
        );
        let mut workflow = workflow_from_parts(
            "call",
            vec![call_node(
                "call",
                "session_subflow",
                "continuation",
                Vec::new(),
            )],
            Vec::new(),
        );
        workflow
            .subflows
            .insert("session_subflow".to_string(), Box::new(subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted.checkpoint.all_results["call"].output,
            "pane-source"
        );
        let captures = runner.captures().await;
        let source_config = captures
            .iter()
            .find(|capture| capture.node_id == "source")
            .and_then(|capture| capture.config.as_ref())
            .expect("source node should have been captured with config");
        assert!(
            !source_config.ephemeral_session,
            "subflow source node referenced by continueSessionFrom must stay persistent"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_cleanup_kills_consumed_continue_session_source_pane() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let (invocation, kill_log, kill_marker) = fake_tmux_kill_invocation(&temp);
        let mut runtime = RuntimeContext::new(db.clone());
        runtime.run_invocation = Some(invocation);

        let source = task_node("source", "Source", "source prompt");
        let mut continuation = task_node("continuation", "Continuation", "continue prompt");
        continuation.continue_session_from = Some("source".to_string());
        let workflow = workflow_from_parts(
            "chain",
            vec![source, continuation],
            vec![success_edge(
                "source_continuation",
                "source",
                "continuation",
                None,
            )],
        );
        let run_id = "run_terminal_continue_cleanup";
        let mut checkpoint = build_initial_checkpoint(&workflow, run_id, BTreeMap::new(), None);
        checkpoint.status = RuntimeStatus::Completed;
        checkpoint.active_cursors.clear();
        checkpoint
            .all_results
            .insert("source".to_string(), NodeResult::default());
        checkpoint
            .all_results
            .insert("continuation".to_string(), NodeResult::default());
        runtime.registry.register(run_id).await;
        runtime
            .registry
            .set_active_pane(
                run_id,
                &active_pane_key("cursor", "source"),
                "%continue-source-pane",
            )
            .await;
        db.upsert_run(&PersistedRun {
            stream_token: new_stream_token(),
            tmux_invocation: runtime.run_invocation.clone(),
            checkpoint: checkpoint.clone(),
            workflow: workflow.clone(),
        })
        .await
        .unwrap();

        finalize_run(
            &runtime,
            &workflow,
            checkpoint,
            Duration::from_millis(1),
        )
        .await
        .unwrap();

        wait_for_path(&kill_marker).await;
        wait_for_registry_empty(&runtime.registry, &run_id).await;
        let recorded = fs::read_to_string(kill_log)
            .expect("fake tmux should record consumed continuation source cleanup");
        assert!(recorded.contains("kill-pane"));
        assert!(
            recorded.contains("%continue-source-pane"),
            "consumed continueSessionFrom source pane should be killed at terminal cleanup"
        );
    }

    #[tokio::test]
    async fn subflow_dispatch_uses_subflow_scoped_cwd_defaults_and_orchestrator() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runner = CapturingRunner::new();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(runner.clone()));

        let mut subflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 2, "item", "body", None),
                task_node("body", "Body", "body {{var:item}}"),
                task_node("after", "After", "after {{previous_output}}"),
            ],
            vec![success_edge("batch_after", "batch", "after", None)],
        );
        subflow.cwd = "/repo-sub".to_string();
        subflow.use_orchestrator = false;
        subflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: json!(["a", "b"]).to_string(),
        }];
        subflow.agent_defaults.insert(
            "mock".to_string(),
            AgentDefaults {
                model: Some("sub-model".to_string()),
                ..Default::default()
            },
        );

        let mut workflow = workflow_from_parts(
            "call",
            vec![call_node("call", "scoped_subflow", "after", Vec::new())],
            Vec::new(),
        );
        workflow.cwd = "/repo-root".to_string();
        workflow.use_orchestrator = true;
        workflow.agent_defaults.insert(
            "mock".to_string(),
            AgentDefaults {
                model: Some("root-model".to_string()),
                ..Default::default()
            },
        );
        workflow
            .subflows
            .insert("scoped_subflow".to_string(), Box::new(subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        let captures = runner.captures().await;
        assert_eq!(
            captures
                .iter()
                .filter(|capture| capture.node_id == "body")
                .count(),
            2
        );
        let after = captures
            .iter()
            .find(|capture| capture.node_id == "after")
            .expect("subflow after task should have run");
        for capture in captures
            .iter()
            .filter(|capture| capture.node_id == "body" || capture.node_id == "after")
        {
            let config = capture.config.as_ref().expect("agent config should exist");
            assert_eq!(capture.cwd, "/repo-sub");
            assert_eq!(config.cwd, "/repo-sub");
            assert_eq!(config.model.as_deref(), Some("sub-model"));
        }

        assert_eq!(after.cwd, "/repo-sub");
        let events = db.list_events(&run_id).await.unwrap();
        assert!(
            !events
                .iter()
                .any(|event| event.kind == "orchestrator_start"),
            "subflow useOrchestrator=false must override root useOrchestrator=true"
        );
    }

    #[tokio::test]
    async fn subflow_call_returns_exit_output_with_isolated_scope() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "first one".to_string(),
                    vec![ScriptedStep::success("internal one")],
                ),
                (
                    "exit one internal one".to_string(),
                    vec![ScriptedStep::success("returned one")],
                ),
                (
                    "after {{entry}} returned one".to_string(),
                    vec![ScriptedStep::success("done one")],
                ),
                (
                    "first two".to_string(),
                    vec![ScriptedStep::success("internal two")],
                ),
                (
                    "exit two internal two".to_string(),
                    vec![ScriptedStep::success("returned two")],
                ),
                (
                    "after {{entry}} returned two".to_string(),
                    vec![ScriptedStep::success("done two")],
                ),
            ])),
        );
        let subflow = workflow_from_parts(
            "entry",
            vec![
                task_node("entry", "Entry", "first {{var:message}}"),
                task_node("exit", "Exit", "exit {{var:message}} {{previous_output}}"),
            ],
            vec![success_edge("entry_exit", "entry", "exit", None)],
        );

        async fn run_parent(
            runtime: &RuntimeContext,
            db: &Database,
            subflow: WorkflowV3,
            input: &str,
        ) -> PersistedRun {
            let mut workflow = workflow_from_parts(
                "call",
                vec![
                    call_node(
                        "call",
                        "echo_subflow",
                        "exit",
                        vec![InputBinding {
                            name: "message".to_string(),
                            source: "var:input".to_string(),
                        }],
                    ),
                    task_node("after", "After", "after {{entry}} {{call}}"),
                ],
                vec![success_edge("call_after", "call", "after", None)],
            );
            workflow.variables = vec![WorkflowVariable {
                name: "input".to_string(),
                default: String::new(),
            }];
            workflow
                .subflows
                .insert("echo_subflow".to_string(), Box::new(subflow));

            let mut vars = BTreeMap::new();
            vars.insert("input".to_string(), input.to_string());
            let run_id = runtime.start_run(workflow, vars, None).await.unwrap();
            wait_for_terminal_run(db, &run_id).await
        }

        let first = run_parent(&runtime, &db, subflow.clone(), "one").await;
        let second = run_parent(&runtime, &db, subflow, "two").await;

        assert_eq!(first.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(second.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(first.checkpoint.all_results["call"].output, "returned one");
        assert_eq!(second.checkpoint.all_results["call"].output, "returned two");
        assert!(!first.checkpoint.all_results.contains_key("entry"));
        assert!(!second.checkpoint.all_results.contains_key("entry"));
        assert_eq!(first.checkpoint.all_results["after"].output, "done one");
        assert_eq!(second.checkpoint.all_results["after"].output, "done two");
    }

    #[tokio::test]
    async fn subflow_entry_resolves_previous_output_from_parent_context() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "upstream".to_string(),
                    vec![ScriptedStep::success("parent-output")],
                ),
                (
                    "entry parent-output".to_string(),
                    vec![ScriptedStep::success("entry-seen")],
                ),
                (
                    "exit entry-seen".to_string(),
                    vec![ScriptedStep::success("sub-result")],
                ),
            ])),
        );
        let subflow = workflow_from_parts(
            "entry",
            vec![
                // Mirrors post-extraction entry prompt after {{previous_output}} → {{var:…}} rewrite.
                task_node("entry", "Entry", "entry {{var:previous_output}}"),
                task_node("exit", "Exit", "exit {{previous_output}}"),
            ],
            vec![success_edge("entry_exit", "entry", "exit", None)],
        );

        let mut workflow = workflow_from_parts(
            "upstream",
            vec![
                task_node("upstream", "Upstream", "upstream"),
                call_node(
                    "call",
                    "echo_subflow",
                    "exit",
                    vec![InputBinding {
                        name: "previous_output".to_string(),
                        source: "previous_output".to_string(),
                    }],
                ),
            ],
            vec![
                success_edge("upstream_call", "upstream", "call", None),
            ],
        );
        workflow
            .subflows
            .insert("echo_subflow".to_string(), Box::new(subflow));

        let run_id = runtime.start_run(workflow, BTreeMap::new(), None).await.unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(persisted.checkpoint.all_results["call"].output, "sub-result");
    }

    #[tokio::test]
    async fn zero_variable_subflow_does_not_capture_root_vars() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(EchoRunner));
        let subflow = workflow_from_parts(
            "entry",
            vec![task_node("entry", "Entry", "inside {{var:X}}")],
            Vec::new(),
        );
        let mut workflow = workflow_from_parts(
            "call",
            vec![
                call_node("call", "empty_scope", "entry", Vec::new()),
                task_node("after", "After", "after {{call}} {{var:X}}"),
            ],
            vec![success_edge("call_after", "call", "after", None)],
        );
        workflow.variables = vec![WorkflowVariable {
            name: "X".to_string(),
            default: String::new(),
        }];
        workflow
            .subflows
            .insert("empty_scope".to_string(), Box::new(subflow));
        let mut vars = BTreeMap::new();
        vars.insert("X".to_string(), "root-value".to_string());

        let run_id = runtime.start_run(workflow, vars, None).await.unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted.checkpoint.all_results["call"].output,
            "inside {{var:X}}"
        );
        assert_eq!(
            persisted.checkpoint.all_results["after"].output,
            "after inside {{var:X}} root-value"
        );
        assert!(!persisted.checkpoint.all_results.contains_key("entry"));
    }

    #[tokio::test]
    async fn subflow_exit_parallel_batch_returns_output_to_parent_call() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(EchoRunner));
        let mut subflow = workflow_from_parts(
            "batch",
            vec![
                parallel_batch_node("batch", "items", 2, "item", "body", None),
                task_node("body", "Body", "batch item {{var:item}}"),
            ],
            Vec::new(),
        );
        subflow.variables = vec![WorkflowVariable {
            name: "items".to_string(),
            default: json!(["a", "b"]).to_string(),
        }];
        let mut workflow = workflow_from_parts(
            "call",
            vec![
                call_node("call", "batch_exit", "batch", Vec::new()),
                task_node("after", "After", "after {{call}}"),
            ],
            vec![success_edge("call_after", "call", "after", None)],
        );
        workflow
            .subflows
            .insert("batch_exit".to_string(), Box::new(subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        let call_result = persisted.checkpoint.all_results.get("call").unwrap();
        let parsed = call_result.parsed_output.as_ref().unwrap();
        assert_eq!(parsed["summary"]["total"], json!(2));
        assert_eq!(parsed["summary"]["succeeded"], json!(2));
        assert!(persisted.checkpoint.all_results.contains_key("after"));
        assert!(!persisted.checkpoint.all_results.contains_key("batch"));
    }

    #[tokio::test]
    async fn subflow_exit_collector_returns_output_to_parent_call() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(EchoRunner));
        let subflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("branch_a", "Branch A", "collector alpha"),
                task_node("branch_b", "Branch B", "collector beta"),
                collector_node("collector"),
            ],
            vec![
                success_edge("split_a", "split", "branch_a", Some("alpha")),
                success_edge("split_b", "split", "branch_b", Some("beta")),
                success_edge("join_a", "branch_a", "collector", Some("alpha")),
                success_edge("join_b", "branch_b", "collector", Some("beta")),
            ],
        );
        let mut workflow = workflow_from_parts(
            "call",
            vec![
                call_node("call", "collector_exit", "collector", Vec::new()),
                task_node("after", "After", "after {{call}}"),
            ],
            vec![success_edge("call_after", "call", "after", None)],
        );
        workflow
            .subflows
            .insert("collector_exit".to_string(), Box::new(subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        let call_result = persisted.checkpoint.all_results.get("call").unwrap();
        let parsed = call_result.parsed_output.as_ref().unwrap();
        assert_eq!(parsed["summary"]["total"], json!(2));
        assert_eq!(
            parsed["inputs"]["alpha"]["output"],
            json!("collector alpha")
        );
        assert_eq!(parsed["inputs"]["beta"]["output"], json!("collector beta"));
        assert!(persisted.checkpoint.all_results.contains_key("after"));
        assert!(!persisted.checkpoint.all_results.contains_key("collector"));
    }

    #[tokio::test]
    async fn subflow_exit_collector_returns_when_all_branches_fail() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "sub-a".to_string(),
                    vec![ScriptedStep::failure("alpha failed", "alpha boom")],
                ),
                (
                    "sub-b".to_string(),
                    vec![ScriptedStep::failure("beta failed", "beta boom").with_delay(5)],
                ),
                (
                    "parent-after".to_string(),
                    vec![ScriptedStep::success("parent done")],
                ),
            ])),
        );
        let subflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("branch_a", "Branch A", "sub-a"),
                task_node("branch_b", "Branch B", "sub-b"),
                collector_node("collector"),
            ],
            vec![
                success_edge("split_a", "split", "branch_a", Some("alpha")),
                success_edge("split_b", "split", "branch_b", Some("beta")),
                success_edge("join_a", "branch_a", "collector", Some("alpha")),
                success_edge("join_b", "branch_b", "collector", Some("beta")),
            ],
        );
        let mut workflow = workflow_from_parts(
            "call",
            vec![
                call_node("call", "failing_collector", "collector", Vec::new()),
                task_node("after", "After", "parent-after"),
            ],
            vec![success_edge("call_after", "call", "after", None)],
        );
        workflow
            .subflows
            .insert("failing_collector".to_string(), Box::new(subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        let call_result = persisted.checkpoint.all_results.get("call").unwrap();
        assert_eq!(
            call_result.parsed_output.as_ref().unwrap()["summary"]["failed"],
            json!(2)
        );
        assert!(persisted.checkpoint.all_results.contains_key("after"));
        assert!(!persisted.checkpoint.all_results.contains_key("collector"));
    }

    #[tokio::test]
    async fn collector_barriers_are_scoped_per_subflow_call_frame() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "left alpha".to_string(),
                    vec![ScriptedStep::success("left alpha").with_delay(5)],
                ),
                (
                    "right alpha".to_string(),
                    vec![ScriptedStep::success("right alpha").with_delay(10)],
                ),
                (
                    "left beta".to_string(),
                    vec![ScriptedStep::success("left beta").with_delay(30)],
                ),
                (
                    "right beta".to_string(),
                    vec![ScriptedStep::success("right beta").with_delay(40)],
                ),
                (
                    "done left left alpha left beta".to_string(),
                    vec![ScriptedStep::success("left complete")],
                ),
                (
                    "done right right alpha right beta".to_string(),
                    vec![ScriptedStep::success("right complete")],
                ),
            ])),
        );
        let mut subflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("alpha", "Alpha", "{{var:label}} alpha"),
                task_node("beta", "Beta", "{{var:label}} beta"),
                collector_node("collector"),
                task_node(
                    "after",
                    "After",
                    "done {{var:label}} {{node:collector.parsedOutput.inputs.alpha.output}} {{node:collector.parsedOutput.inputs.beta.output}}",
                ),
            ],
            vec![
                success_edge("split_alpha", "split", "alpha", Some("alpha")),
                success_edge("split_beta", "split", "beta", Some("beta")),
                success_edge("join_alpha", "alpha", "collector", Some("alpha")),
                success_edge("join_beta", "beta", "collector", Some("beta")),
                success_edge("collector_after", "collector", "after", None),
            ],
        );
        subflow.variables = vec![WorkflowVariable {
            name: "label".to_string(),
            default: String::new(),
        }];
        let mut workflow = workflow_from_parts(
            "root_split",
            vec![
                split_node("root_split", SplitFailurePolicy::BestEffortContinue),
                call_node(
                    "call_left",
                    "merge_subflow",
                    "after",
                    vec![InputBinding {
                        name: "label".to_string(),
                        source: "var:left_label".to_string(),
                    }],
                ),
                call_node(
                    "call_right",
                    "merge_subflow",
                    "after",
                    vec![InputBinding {
                        name: "label".to_string(),
                        source: "var:right_label".to_string(),
                    }],
                ),
            ],
            vec![
                success_edge("root_left", "root_split", "call_left", Some("left")),
                success_edge("root_right", "root_split", "call_right", Some("right")),
            ],
        );
        workflow.variables = vec![
            WorkflowVariable {
                name: "left_label".to_string(),
                default: "left".to_string(),
            },
            WorkflowVariable {
                name: "right_label".to_string(),
                default: "right".to_string(),
            },
        ];
        workflow
            .subflows
            .insert("merge_subflow".to_string(), Box::new(subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted.checkpoint.all_results["call_left"].output,
            "left complete"
        );
        assert_eq!(
            persisted.checkpoint.all_results["call_right"].output,
            "right complete"
        );

        let events = db.list_events(&run_id).await.unwrap();
        let collector_releases = events
            .iter()
            .filter(|event| {
                event.kind == "collector_released"
                    && event.data.get("nodeId") == Some(&json!("collector"))
            })
            .count();
        assert_eq!(collector_releases, 2);
    }

    #[tokio::test]
    async fn collector_barrier_resets_when_loop_reenters_collector() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let mut gate = task_node("gate", "Gate", "again");
        gate.loop_condition = Some(StructuredCondition {
            field: "again".to_string(),
            operator: "==".to_string(),
            value: "true".to_string(),
        });
        gate.loop_max_iterations = Some(2);
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "left".to_string(),
                    vec![
                        ScriptedStep::success("left first").with_delay(5),
                        ScriptedStep::success("left second").with_delay(5),
                    ],
                ),
                (
                    "right".to_string(),
                    vec![
                        ScriptedStep::success("right first").with_delay(10),
                        ScriptedStep::success("right second").with_delay(10),
                    ],
                ),
                (
                    "again".to_string(),
                    vec![
                        ScriptedStep::success("loop first")
                            .with_parsed_output(json!({"again": true})),
                        ScriptedStep::success("loop second")
                            .with_parsed_output(json!({"again": true})),
                    ],
                ),
                ("done".to_string(), vec![ScriptedStep::success("done")]),
            ])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("left", "Left", "left"),
                task_node("right", "Right", "right"),
                collector_node("collector"),
                gate,
                task_node("done", "Done", "done"),
            ],
            vec![
                success_edge("split_left", "split", "left", Some("left")),
                success_edge("split_right", "split", "right", Some("right")),
                success_edge("left_collect", "left", "collector", Some("left")),
                success_edge("right_collect", "right", "collector", Some("right")),
                success_edge("collector_gate", "collector", "gate", None),
                WorkflowEdge {
                    id: "gate_continue".to_string(),
                    from: "gate".to_string(),
                    to: "split".to_string(),
                    outcome: WorkflowEdgeOutcome::LoopContinue,
                    label: Some("again".to_string()),
                    branch_id: None,
                    condition: None,
                },
                WorkflowEdge {
                    id: "gate_exit".to_string(),
                    from: "gate".to_string(),
                    to: "done".to_string(),
                    outcome: WorkflowEdgeOutcome::LoopExit,
                    label: Some("done".to_string()),
                    branch_id: None,
                    condition: None,
                },
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert!(persisted.checkpoint.all_results.contains_key("done"));
        let events = db.list_events(&run_id).await.unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "collector_released")
                .count(),
            2
        );
        assert!(!events.iter().any(|event| {
            event.kind == "workflow_error"
                && event.data.get("message")
                    == Some(&json!("Collector is blocked waiting on missing inputs."))
        }));
    }

    #[tokio::test]
    async fn duplicate_collector_merge_key_warns_for_start_run_callers() {
        let capture = WarningCapture::default();
        let warnings = capture.warnings.clone();
        let subscriber = tracing_subscriber::registry().with(capture);
        let _guard = tracing::subscriber::set_default(subscriber);
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "left".to_string(),
                    vec![ScriptedStep::success("left output").with_delay(5)],
                ),
                (
                    "right".to_string(),
                    vec![ScriptedStep::success("right output").with_delay(10)],
                ),
                (
                    "after".to_string(),
                    vec![
                        ScriptedStep::success("done first"),
                        ScriptedStep::success("done second"),
                    ],
                ),
            ])),
        );
        let workflow = workflow_from_parts(
            "split",
            vec![
                split_node("split", SplitFailurePolicy::BestEffortContinue),
                task_node("left", "Left", "left"),
                task_node("right", "Right", "right"),
                collector_node("collector"),
                task_node("after", "After", "after"),
            ],
            vec![
                success_edge("split_left", "split", "left", Some("left")),
                success_edge("split_right", "split", "right", Some("right")),
                success_edge("left_collect", "left", "collector", Some("dup")),
                success_edge("right_collect", "right", "collector", Some("dup")),
                success_edge("collector_after", "collector", "after", None),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        let warnings = warnings.lock().unwrap().join("\n");
        assert!(
            warnings.contains("duplicate collector input key in barrier configuration"),
            "expected duplicate collector warning, got {warnings}"
        );
        assert!(warnings.contains("collector"));
        assert!(warnings.contains("dup"));
    }

    #[tokio::test]
    async fn best_effort_continue_keeps_run_completable_after_branch_failure() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "branch-a".to_string(),
                    vec![ScriptedStep::success("alpha ok")],
                ),
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
    async fn best_effort_continue_keeps_policy_after_all_branches_fail() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "branch-a".to_string(),
                    vec![ScriptedStep::failure("alpha failed", "alpha boom")],
                ),
                (
                    "branch-b".to_string(),
                    vec![ScriptedStep::failure("beta failed", "beta boom").with_delay(5)],
                ),
                (
                    "after".to_string(),
                    vec![ScriptedStep::failure("after failed", "after boom")],
                ),
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
        assert_eq!(
            collector.parsed_output.as_ref().unwrap()["summary"]["failed"],
            json!(2)
        );
    }

    #[tokio::test]
    async fn drain_then_fail_waits_for_siblings_before_failing_run() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "branch-a".to_string(),
                    vec![ScriptedStep::success("alpha ok").with_delay(30)],
                ),
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
        assert!(
            persisted
                .checkpoint
                .split_families
                .values()
                .all(|family| family.force_failed)
        );
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
    async fn nested_fail_fast_split_keeps_outer_policy_and_cancels_siblings() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "outer-sibling".to_string(),
                    vec![ScriptedStep::success("outer done").with_delay(5)],
                ),
                (
                    "inner-fail".to_string(),
                    vec![ScriptedStep::failure("failed", "boom").with_delay(30)],
                ),
                (
                    "inner-slow".to_string(),
                    vec![ScriptedStep::success("slow done").with_delay(200)],
                ),
            ])),
        );
        let workflow = workflow_from_parts(
            "outer_split",
            vec![
                split_node("outer_split", SplitFailurePolicy::FailFastCancel),
                split_node("inner_split", SplitFailurePolicy::BestEffortContinue),
                task_node("outer_sibling", "Outer Sibling", "outer-sibling"),
                task_node("inner_fail", "Inner Fail", "inner-fail"),
                task_node("inner_slow", "Inner Slow", "inner-slow"),
            ],
            vec![
                success_edge("outer_inner", "outer_split", "inner_split", Some("inner")),
                success_edge(
                    "outer_sibling_edge",
                    "outer_split",
                    "outer_sibling",
                    Some("sibling"),
                ),
                success_edge("inner_fail_edge", "inner_split", "inner_fail", Some("fail")),
                success_edge("inner_slow_edge", "inner_split", "inner_slow", Some("slow")),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Failed);
        assert!(persisted.checkpoint.all_results.contains_key("inner_fail"));
        assert!(!persisted.checkpoint.all_results.contains_key("inner_slow"));
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
        assert_eq!(
            pending_a.checkpoint.queued_approvals[0].approval.node_id,
            "approve_b"
        );

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
            events
                .iter()
                .filter(|event| event.kind == "approval_queued")
                .count(),
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
    async fn restart_from_drains_active_executor_before_spawning_replacement() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::new(db.clone());
        let workflow = workflow_from_parts(
            "approve",
            vec![approval_node("approve", "Approve", "continue?")],
            Vec::new(),
        );
        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_run(&db, &run_id, |persisted| {
            persisted.checkpoint.pending_approval.is_some()
        })
        .await;

        let restarted_run_id = runtime.restart_from(&run_id, "approve").await.unwrap();
        let active_run_ids = runtime.registry.active_run_ids().await;

        assert!(!active_run_ids.contains(&run_id));
        assert!(active_run_ids.contains(&restarted_run_id));

        runtime.abort_run(&restarted_run_id).await.unwrap();
        wait_for_terminal_run(&db, &restarted_run_id).await;
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

        let restarted_run_id = runtime
            .restart_from(&run_id, "approve_branch")
            .await
            .unwrap();
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
        assert!(!finished.checkpoint.all_results.contains_key("collector"));
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
        // Bulleted list format used for the structured-output prompt fallback
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
            version: 4,
            name: None,
            goal: "test".to_string(),
            cwd: "/tmp".to_string(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 10,
                max_visits_per_node: 5,
            },
            nodes: vec![n1, n2, n3],
            edges: vec![],
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        };

        let graph = wf.graph();
        let set = super::build_session_persistence_set(&graph, &BTreeMap::new());
        assert!(set.contains("n1"), "n1 should need persistent session");
        assert!(set.contains("n2"), "n2 should need persistent session");
        assert!(!set.contains("n3"), "n3 is not referenced by anyone");
    }

    #[test]
    fn build_session_persistence_set_ignores_completed_continuations() {
        let n1 = task_node("n1", "Step 1", "prompt1");
        let mut n2 = task_node("n2", "Step 2", "prompt2");
        n2.continue_session_from = Some("n1".to_string());

        let wf = WorkflowV3 {
            version: 4,
            name: None,
            goal: "test".to_string(),
            cwd: "/tmp".to_string(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 10,
                max_visits_per_node: 5,
            },
            nodes: vec![n1, n2],
            edges: vec![],
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        };
        let completed = BTreeMap::from([("n2".to_string(), NodeResult::default())]);

        let graph = wf.graph();
        let set = super::build_session_persistence_set(&graph, &completed);
        assert!(
            set.is_empty(),
            "completed continuation nodes should no longer keep sources alive"
        );
    }

    // -----------------------------------------------------------------------
    // Stage 9: Integration — config resolution + driver build_args pipeline
    // -----------------------------------------------------------------------

    #[test]
    fn integration_mixed_agents_config_resolution() {
        // Workflow with Claude and Codex nodes using different configs
        let mut defaults = BTreeMap::new();
        defaults.insert(
            "claude".to_string(),
            AgentDefaults {
                model: Some("sonnet".to_string()),
                max_turns: Some(5),
                ..Default::default()
            },
        );
        defaults.insert(
            "codex".to_string(),
            AgentDefaults {
                model: Some("o3-mini".to_string()),
                reasoning_level: Some(ReasoningLevel::Medium),
                ..Default::default()
            },
        );

        // Claude node with node-level override
        let mut claude_node = task_node("n1", "Claude Step", "prompt1");
        claude_node.agent = Some("claude".to_string());
        claude_node.kind.set_agent_config(Some(AgentNodeConfig {
            base: AgentDefaults {
                max_budget_usd: Some(1.5),
                ..Default::default()
            },
            ..Default::default()
        }));

        let claude_config = resolve_agent_config(
            &defaults,
            "/work",
            "claude",
            &claude_node,
            None,
            false,
            None,
        );
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

        let codex_config =
            resolve_agent_config(&defaults, "/work", "codex", &codex_node, None, false, None);
        assert_eq!(codex_config.model.as_deref(), Some("o3-mini"));
        assert_eq!(codex_config.reasoning_level, Some(ReasoningLevel::Medium));

        let codex_driver = get_driver("codex").unwrap();
        let codex_cmd = codex_driver.build_session_args(&codex_config).unwrap();
        assert!(codex_cmd.args.iter().any(|a| a == "--model"));
        assert!(
            codex_cmd
                .args
                .iter()
                .any(|a| a.contains("model_reasoning_effort=medium"))
        );
    }

    #[test]
    fn integration_session_args_no_json_mode_flags() {
        // With PTY mode, no agent should have --json-schema, --print, --json, etc.
        let claude_driver = get_driver("claude").unwrap();
        let claude_cmd = claude_driver
            .build_session_args(&Default::default())
            .unwrap();
        assert!(!claude_cmd.args.iter().any(|a| a == "--json-schema"));
        assert!(!claude_cmd.args.iter().any(|a| a == "--print"));
        assert!(!claude_cmd.args.iter().any(|a| a == "--output-format"));

        let codex_driver = get_driver("codex").unwrap();
        let codex_cmd = codex_driver
            .build_session_args(&Default::default())
            .unwrap();
        assert!(!codex_cmd.args.iter().any(|a| a == "--json"));
        assert!(!codex_cmd.args.iter().any(|a| a == "--output-schema"));
    }

    #[test]
    fn integration_session_reuse_config_resolution() {
        let defaults = BTreeMap::new();
        let node = task_node("n2", "Step 2", "continue");

        // With session ID and persistence needed
        let config = resolve_agent_config(
            &defaults,
            "/work",
            "claude",
            &node,
            Some("sess-abc".to_string()),
            true,
            None,
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

            // Built-in drivers should handle all access modes without error.
            for agent_name in &["claude", "codex"] {
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
                    _ => {} // Other combos already tested individually
                }

                if let Some(dir) = cmd.temp_dir {
                    let _ = std::fs::remove_dir_all(dir);
                }
            }

            // Registry-backed agents resolve mapped profiles or fall back to default.
            for agent_name in &["cursor", "agy"] {
                let Some(driver) = get_driver(agent_name) else {
                    continue;
                };
                let result = driver.build_session_args(&config);
                if *agent_name == "agy" && *mode == AccessMode::ReadOnly {
                    assert_eq!(
                        result.unwrap_err().to_string(),
                        "agent agy has no read-only access profile"
                    );
                    continue;
                }
                assert!(
                    result.is_ok(),
                    "registry agent {agent_name} failed with {mode:?}: {:?}",
                    result.as_ref().err()
                );

                if let Ok(cmd) = result {
                    if let Some(dir) = cmd.temp_dir {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
            }
        }
    }

    #[test]
    fn integration_per_node_cwd_override() {
        let defaults = BTreeMap::new();
        let mut node = task_node("n1", "Step", "prompt");

        // Without node cwd → uses workflow cwd
        let config =
            resolve_agent_config(&defaults, "/workspace", "claude", &node, None, false, None);
        assert_eq!(config.cwd, "/workspace");

        // With node cwd → uses node cwd
        node.cwd = Some("/other/project".to_string());
        let config =
            resolve_agent_config(&defaults, "/workspace", "claude", &node, None, false, None);
        assert_eq!(config.cwd, "/other/project");
    }

    #[test]
    fn integration_workflow_with_agent_defaults_roundtrip() {
        let json = json!({
            "version": 4,
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
                "agent": "claude",
                "prompt": "hello",
                "kind": {
                    "type": "task",
                    "agentConfig": {
                        "maxBudgetUsd": 1.5,
                        "toolToggles": { "webSearch": false }
                    }
                },
                "cwd": "/custom"
            }, {
                "id": "n2",
                "name": "Step 2",
                "agent": "codex",
                "prompt": "world",
                "continueSessionFrom": null,
                "kind": { "type": "task" }
            }],
            "edges": [{ "id": "e1", "from": "n1", "to": "n2", "outcome": "success" }]
        });
        let result = normalize_workflow_value(json);
        assert!(
            result.is_ok(),
            "Workflow with agent config should parse: {:?}",
            result.err()
        );
        let w = result.unwrap().workflow;

        // Agent defaults parsed
        assert_eq!(w.agent_defaults.len(), 2);
        assert_eq!(w.agent_defaults["claude"].model.as_deref(), Some("sonnet"));
        assert_eq!(w.agent_defaults["claude"].max_turns, Some(5));
        assert_eq!(
            w.agent_defaults["codex"].reasoning_level,
            Some(ReasoningLevel::High)
        );

        // Node config parsed
        let n1 = &w.nodes[0];
        let agent_config = n1.kind.agent_config().unwrap();
        assert_eq!(agent_config.base.max_budget_usd, Some(1.5));
        assert_eq!(n1.cwd.as_deref(), Some("/custom"));
    }

    #[test]
    fn build_session_persistence_set_empty_when_no_references() {
        let wf = WorkflowV3 {
            version: 4,
            name: None,
            goal: "test".to_string(),
            cwd: "/tmp".to_string(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 10,
                max_visits_per_node: 5,
            },
            nodes: vec![task_node("n1", "Step 1", "prompt1")],
            edges: vec![],
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        };

        let graph = wf.graph();
        let set = super::build_session_persistence_set(&graph, &BTreeMap::new());
        assert!(set.is_empty());
    }

    // -----------------------------------------------------------------------
    // T15: RunAgent node integration — routes through ScriptedRunner seam
    // -----------------------------------------------------------------------

    /// Helper: build a RunAgent node. The ScriptedRunner sees the resolved prompt,
    /// so we match on the `run_agent_config.prompt` value.
    fn run_agent_node(id: &str, name: &str, agent: &str, prompt: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: name.to_string(),
            kind: NodeKind::RunAgent {
                run_agent_config: model::RunAgentConfig {
                    agent: Some(agent.to_string()),
                    prompt: Some(prompt.to_string()),
                    ..Default::default()
                },
                agent_config: None,
            },
            agent: Some(agent.to_string()),
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
            cwd: None,
            continue_session_from: None,
        }
    }

    #[tokio::test]
    async fn minimal_run_agent_node_completes() {
        // Phase 1 gate: a minimal workflow with a single RunAgent node runs via the
        // engine and produces a completed checkpoint with the scripted output.
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([(
                "echo hello".to_string(),
                vec![ScriptedStep::success("agent output")],
            )])),
        );

        let workflow = workflow_from_parts(
            "agent",
            vec![run_agent_node("agent", "Run Echo", "mock", "echo hello")],
            vec![],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted.checkpoint.all_results["agent"].output,
            "agent output"
        );
    }

    #[tokio::test]
    async fn run_agent_node_followed_by_task_uses_previous_output() {
        // RunAgent output is available to a downstream Task via {{previous_output}}.
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "run agent prompt".to_string(),
                    vec![ScriptedStep::success("agent result")],
                ),
                (
                    "after agent result".to_string(),
                    vec![ScriptedStep::success("downstream done")],
                ),
            ])),
        );

        let workflow = workflow_from_parts(
            "agent",
            vec![
                run_agent_node("agent", "Run Agent", "mock", "run agent prompt"),
                task_node("after", "After", "after {{previous_output}}"),
            ],
            vec![success_edge("agent_after", "agent", "after", None)],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted.checkpoint.all_results["after"].output,
            "downstream done"
        );
    }

    // -----------------------------------------------------------------------
    // T15: Decide routing — branch-edge selection by output label
    // These tests verify the label-matching logic via the
    // `select_decide_outcome` function. Full engine-level Decide tests would
    // require a real agent pane; we test routing mechanics via the unit-level fn.
    // -----------------------------------------------------------------------

    #[test]
    fn decide_routing_exact_match_takes_priority_over_substring() {
        let outcomes = vec![
            "approve".to_string(),
            "revise".to_string(),
            "reject".to_string(),
        ];
        // Exact match
        assert_eq!(
            select_decide_outcome(&outcomes, "revise", None),
            decide_matched("revise")
        );
        // Substring match when no exact match
        assert_eq!(
            select_decide_outcome(&outcomes, "I would approve this.", None),
            decide_matched("approve")
        );
        // No match
        assert_eq!(
            select_decide_outcome(&outcomes, "unknown", None),
            decide_unmatched()
        );
    }

    #[test]
    fn decide_routing_trims_quotes_and_whitespace() {
        let outcomes = vec![
            "NEXT_STORY".to_string(),
            "DONE".to_string(),
            "BLOCKED".to_string(),
        ];
        assert_eq!(
            select_decide_outcome(&outcomes, "  \"NEXT_STORY\"  ", None),
            decide_matched("NEXT_STORY")
        );
        assert_eq!(
            select_decide_outcome(&outcomes, "`DONE`", None),
            decide_matched("DONE")
        );
    }

    #[test]
    fn decide_routing_uses_sentinel_extracted_answer_for_multiline_prompt() {
        let outcomes = vec!["approve".to_string(), "reject".to_string()];
        let prompt = "Choose exactly one outcome:\n- approve\n- reject";
        let capture = concat!(
            "ready\n",
            "Choose exactly one outcome:\n",
            "- approve\n",
            "- reject\n",
            "SB_PROMPT_END_decide\n",
            "reject\n",
            "SB_RESPONSE_DONE_decide\n"
        );

        let (response, _) = crate::tmux_exec::extract_after_prompt_with_sentinels(
            "ready\n",
            capture,
            prompt,
            Some("SB_PROMPT_END_decide"),
            Some("SB_RESPONSE_DONE_decide"),
        );

        assert_eq!(response.trim(), "reject");
        assert_eq!(
            select_decide_outcome(&outcomes, &response, None),
            decide_matched("reject")
        );
    }

    #[test]
    fn decide_routing_prefers_json_artifact_outcome_over_prose() {
        let outcomes = vec!["approve".to_string(), "reject".to_string()];
        let node = task_node("decide", "Decide", "prompt");
        let response = r#"{"outcome":"approve","reason":"I would not reject this."}"#;
        let parsed = parse_decide_structured_response(&node, response);

        assert_eq!(
            select_decide_outcome(&outcomes, response, parsed.as_ref()),
            decide_matched("approve")
        );
    }

    #[test]
    fn decide_routing_invalid_json_artifact_outcome_does_not_fall_back_to_prose() {
        let outcomes = vec!["approve".to_string(), "reject".to_string()];
        let node = task_node("decide", "Decide", "prompt");
        let response = r#"{"outcome":"maybe","reason":"approve"}"#;
        let parsed = parse_decide_structured_response(&node, response);

        assert_eq!(
            select_decide_outcome(&outcomes, response, parsed.as_ref()),
            decide_structured_label_mismatch()
        );
    }

    #[test]
    fn decide_outcome_overlapping_labels_prefers_longest_at_same_offset() {
        let outcomes = vec!["approve".to_string(), "approve_with_changes".to_string()];
        assert_eq!(
            select_decide_outcome(&outcomes, "approve_with_changes", None),
            decide_matched("approve_with_changes")
        );
        assert_eq!(
            select_decide_outcome(&outcomes, "My decision is approve_with_changes.", None),
            decide_matched("approve_with_changes")
        );
    }

    #[test]
    fn decide_outcome_reasoning_bleed_does_not_misroute_on_substring_sibling() {
        let outcomes = vec!["revise".to_string(), "approve".to_string()];
        assert_eq!(
            select_decide_outcome(&outcomes, "I approve this revision.", None),
            decide_matched("approve")
        );
    }

    #[test]
    fn decide_outcome_ambiguous_multi_match_returns_none() {
        let outcomes = vec!["approve".to_string(), "approve-with-changes".to_string()];
        assert_eq!(
            select_decide_outcome(&outcomes, "approve-with-changes", None),
            decide_matched("approve-with-changes")
        );

        let outcomes = vec!["yes".to_string(), "no".to_string()];
        assert_eq!(
            select_decide_outcome(&outcomes, "yes and no are both valid", None),
            decide_matched("yes")
        );

        let outcomes = vec!["pick".to_string(), "pick".to_string()];
        assert_eq!(
            select_decide_outcome(&outcomes, "I choose pick.", None),
            decide_unmatched()
        );
    }

    #[test]
    fn decide_routing_numeric_json_outcome_matches_numeric_label() {
        let outcomes = vec!["1".to_string(), "2".to_string()];
        let parsed: Value = json!({"outcome": 1});
        assert_eq!(
            select_decide_outcome(&outcomes, "", Some(&parsed)),
            decide_matched("1")
        );
    }

    #[test]
    fn decide_routing_boolean_json_outcome_reports_type_specific_failure() {
        let outcomes = vec!["approve".to_string(), "reject".to_string()];
        let parsed: Value = json!({"outcome": true});
        assert_eq!(
            select_decide_outcome(&outcomes, "", Some(&parsed)),
            DecideOutcomeSelection::Failed(DecideOutcomeFailure::StructuredNonScalar {
                json_type: "boolean"
            })
        );
    }

    #[test]
    fn decide_routing_object_json_outcome_does_not_fall_back_to_prose() {
        let outcomes = vec!["approve".to_string(), "reject".to_string()];
        let response = r#"{"outcome":{"choice":"approve"},"reason":"approve"}"#;
        let parsed: Value = serde_json::from_str(response).unwrap();
        assert_eq!(
            select_decide_outcome(&outcomes, response, Some(&parsed)),
            DecideOutcomeSelection::Failed(DecideOutcomeFailure::StructuredNonScalar {
                json_type: "object"
            })
        );
    }

    #[test]
    fn decide_agent_failure_skips_outcome_routing_even_when_output_matches() {
        let mut node = task_node("decide", "Decide", "prompt");
        node.kind = NodeKind::Decide {
            decide_config: model::DecideConfig {
                prompt: "pick".to_string(),
                outcomes: vec!["approve".to_string(), "reject".to_string()],
                inputs: vec![],
                model: None,
            },
        };
        let config = match &node.kind {
            NodeKind::Decide { decide_config } => decide_config.clone(),
            _ => unreachable!(),
        };
        let agent_result = NodeResult {
            success: false,
            output: "approve".to_string(),
            stderr: "Timeout waiting for agent response".to_string(),
            exit_code: -2,
            duration: "1.0".to_string(),
            agent: "llm".to_string(),
            prompt: config.prompt.clone(),
            metadata: AgentExecutionMetadata {
                error_type: Some("timeout".to_string()),
                agent_session_id: Some("%pane".to_string()),
                cost_usd: Some(0.42),
                ..Default::default()
            },
            ..Default::default()
        };
        let merged = apply_decide_outcome_to_agent_result(
            agent_result,
            &node,
            &config,
            &BTreeMap::new(),
            "resolved",
            "2.0".to_string(),
        );
        assert!(!merged.success);
        assert_eq!(merged.exit_code, -2);
        assert_eq!(merged.output, "approve");
        assert_eq!(
            merged.metadata.error_type.as_deref(),
            Some("timeout")
        );
        assert_eq!(merged.metadata.agent_session_id.as_deref(), Some("%pane"));
        assert_eq!(merged.metadata.cost_usd, Some(0.42));
    }

    #[test]
    fn decide_success_preserves_agent_metadata_and_applies_outcome() {
        let mut node = task_node("decide", "Decide", "prompt");
        node.kind = NodeKind::Decide {
            decide_config: model::DecideConfig {
                prompt: "pick".to_string(),
                outcomes: vec!["approve".to_string(), "reject".to_string()],
                inputs: vec![],
                model: None,
            },
        };
        let config = match &node.kind {
            NodeKind::Decide { decide_config } => decide_config.clone(),
            _ => unreachable!(),
        };
        let agent_result = NodeResult {
            success: true,
            output: "approve".to_string(),
            exit_code: 0,
            duration: "1.0".to_string(),
            agent: "llm".to_string(),
            prompt: config.prompt.clone(),
            metadata: AgentExecutionMetadata {
                agent_session_id: Some("%pane".to_string()),
                cost_usd: Some(1.25),
                input_tokens: Some(100),
                output_tokens: Some(20),
                ..Default::default()
            },
            ..Default::default()
        };
        let merged = apply_decide_outcome_to_agent_result(
            agent_result,
            &node,
            &config,
            &BTreeMap::new(),
            "resolved",
            "2.0".to_string(),
        );
        assert!(merged.success);
        assert_eq!(merged.output, "approve");
        assert_eq!(merged.metadata.agent_session_id.as_deref(), Some("%pane"));
        assert_eq!(merged.metadata.cost_usd, Some(1.25));
        assert_eq!(merged.metadata.input_tokens, Some(100));
    }

    #[test]
    fn timeout_for_node_decide_uses_configured_node_timeout() {
        let mut node = task_node("decide", "Decide", "prompt");
        node.timeout = Some(12);
        node.kind = NodeKind::Decide {
            decide_config: model::DecideConfig {
                prompt: "pick".to_string(),
                outcomes: vec!["a".to_string()],
                inputs: vec![],
                model: None,
            },
        };
        assert_eq!(timeout_for_node(&node), Some(12));
    }

    // -----------------------------------------------------------------------
    // T15: Compound subflow — two parents call the same subflow with different
    //      inputs and receive isolated, correct outputs (extends the existing test).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn subflow_called_from_two_parents_with_different_inputs_isolated() {
        // This test complements the existing `subflow_call_returns_exit_output_with_isolated_scope`.
        // We run BOTH parents through the same RuntimeContext (shared ScriptedRunner)
        // and verify each gets the correct, independently-scoped output.
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                // input = "alpha"
                (
                    "compute alpha".to_string(),
                    vec![ScriptedStep::success("result-alpha")],
                ),
                // input = "beta"
                (
                    "compute beta".to_string(),
                    vec![ScriptedStep::success("result-beta")],
                ),
            ])),
        );

        // Subflow: single task node named "compute" that receives `{{var:input}}`.
        let subflow = workflow_from_parts(
            "compute",
            vec![task_node("compute", "Compute", "compute {{var:input}}")],
            vec![],
        );

        async fn run_with_input(
            runtime: &RuntimeContext,
            db: &Database,
            subflow: WorkflowV3,
            input: &str,
        ) -> PersistedRun {
            let mut wf = workflow_from_parts(
                "call",
                vec![call_node(
                    "call",
                    "compute_sub",
                    "compute",
                    vec![InputBinding {
                        name: "input".to_string(),
                        source: "var:input".to_string(),
                    }],
                )],
                vec![],
            );
            wf.variables = vec![WorkflowVariable {
                name: "input".to_string(),
                default: String::new(),
            }];
            wf.subflows
                .insert("compute_sub".to_string(), Box::new(subflow));
            let mut vars = BTreeMap::new();
            vars.insert("input".to_string(), input.to_string());
            let run_id = runtime.start_run(wf, vars, None).await.unwrap();
            wait_for_terminal_run(db, &run_id).await
        }

        let alpha = run_with_input(&runtime, &db, subflow.clone(), "alpha").await;
        let beta = run_with_input(&runtime, &db, subflow, "beta").await;

        assert_eq!(alpha.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(beta.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(alpha.checkpoint.all_results["call"].output, "result-alpha");
        assert_eq!(beta.checkpoint.all_results["call"].output, "result-beta");
        // The subflow's internal node must NOT appear in the parent's results
        assert!(!alpha.checkpoint.all_results.contains_key("compute"));
        assert!(!beta.checkpoint.all_results.contains_key("compute"));
    }

    // -----------------------------------------------------------------------
    // T15: Subflow — recursion depth bound enforced at runtime
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn subflow_exceeding_max_depth_fails_run() {
        // A subflow with max_depth=1 that tries to call itself again must fail.
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();

        // The subflow body is just a call node back to itself (depth = 1 allows
        // one level of nesting; a second attempt exceeds it).
        let mut recurse_call = call_node("recurse", "self_ref", "recurse", vec![]);
        // Override max_depth to 1 so the second call is caught.
        if let NodeKind::Call {
            ref mut subflow_config,
        } = recurse_call.kind
        {
            let cfg = subflow_config;
            cfg.max_depth = 1;
        }

        let recurse_body = workflow_from_parts("recurse", vec![recurse_call], vec![]);
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(ScriptedRunner::new([])));
        let mut wf = workflow_from_parts(
            "call",
            vec![{
                let mut n = call_node("call", "self_ref", "recurse", vec![]);
                if let NodeKind::Call {
                    ref mut subflow_config,
                } = n.kind
                {
                    let cfg = subflow_config;
                    cfg.max_depth = 1;
                }
                n
            }],
            vec![],
        );
        wf.subflows
            .insert("self_ref".to_string(), Box::new(recurse_body));

        let run_id = runtime.start_run(wf, BTreeMap::new(), None).await.unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        // The run must fail (not hang) when the depth bound is exceeded.
        assert_eq!(
            persisted.checkpoint.status,
            RuntimeStatus::Failed,
            "expected Failed when maxDepth exceeded"
        );
    }

    // -----------------------------------------------------------------------
    // T15: Compound subflow — resume a run paused *inside* a subflow
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn subflow_with_approval_inside_can_be_resumed() {
        // Verify that a run paused inside a subflow (waiting for human approval)
        // can be resumed and completes correctly — nested checkpointing works.
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([
                (
                    "pre work".to_string(),
                    vec![ScriptedStep::success("pre done")],
                ),
                (
                    "post work".to_string(),
                    vec![ScriptedStep::success("post done")],
                ),
            ])),
        );

        // Subflow: pre-task → approval → post-task
        // The approval acts as the pause point inside the subflow.
        let subflow = workflow_from_parts(
            "pre",
            vec![
                task_node("pre", "Pre", "pre work"),
                approval_node("gate", "Gate", "approve?"),
                task_node("post", "Post", "post work"),
            ],
            vec![
                success_edge("pre_gate", "pre", "gate", None),
                success_edge("gate_post", "gate", "post", None),
            ],
        );

        let mut wf = workflow_from_parts(
            "call",
            vec![call_node("call", "sub", "post", vec![])],
            vec![],
        );
        wf.subflows.insert("sub".to_string(), Box::new(subflow));

        let run_id = runtime.start_run(wf, BTreeMap::new(), None).await.unwrap();

        // Wait for the approval inside the subflow to be raised
        let pending = wait_for_run(&db, &run_id, |p| p.checkpoint.pending_approval.is_some()).await;
        assert!(
            pending.checkpoint.pending_approval.is_some(),
            "expected approval pending inside subflow"
        );

        // Approve → run should resume and complete
        runtime
            .approve_run(&run_id, true, "approved".to_string())
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(
            persisted.checkpoint.status,
            RuntimeStatus::Completed,
            "run should complete after approving inside subflow"
        );
        // The call node's output = the subflow exit node's (post) output
        assert_eq!(persisted.checkpoint.all_results["call"].output, "post done");
    }

    // -----------------------------------------------------------------------
    // T15: Engine-level — stubbed serial loop that mirrors the epic-dev template
    //      (Decide nodes replaced with Task nodes to stay hermetic)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn stubbed_epic_dev_serial_loop_completes() {
        // Models the epic-dev pattern: read-status → pick-story → run-story → loop.
        // Decide routing is replaced by deterministic Task→success edges so the
        // run is fully hermetic (no LLM calls).
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();

        let runner = Arc::new(ScriptedRunner::new([
            (
                "read sprint status".to_string(),
                vec![ScriptedStep::success("sprint: 1 story remaining")],
            ),
            (
                "implement story".to_string(),
                vec![ScriptedStep::success("story done")],
            ),
            (
                "generate report".to_string(),
                vec![ScriptedStep::success("epic complete")],
            ),
        ]));
        let runtime = RuntimeContext::with_runner(db.clone(), runner);

        // Simplified epic-dev: read → implement → report (no Decide/loop in stubbed version)
        let workflow = workflow_from_parts(
            "read",
            vec![
                run_agent_node("read", "Read Sprint Status", "mock", "read sprint status"),
                run_agent_node("impl", "Implement Story", "mock", "implement story"),
                run_agent_node("report", "Generate Report", "mock", "generate report"),
            ],
            vec![
                success_edge("read_impl", "read", "impl", None),
                success_edge("impl_report", "impl", "report", None),
            ],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        assert_eq!(
            persisted.checkpoint.all_results["read"].output,
            "sprint: 1 story remaining"
        );
        assert_eq!(
            persisted.checkpoint.all_results["impl"].output,
            "story done"
        );
        assert_eq!(
            persisted.checkpoint.all_results["report"].output,
            "epic complete"
        );
    }

    // -----------------------------------------------------------------------
    // T15: Engine-level — stubbed multi-agent-plan through consolidation
    //      (ParallelBatch + call node + approval merge gate, all scripted)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn stubbed_multi_agent_plan_through_consolidation() {
        // Models the multi-agent-plan-implementation template:
        // plan → ParallelBatch(impl, cap 2) → approval merge gate →
        // call dual-review subflow → consolidate.
        // All agent work is scripted; the merge gate is approved programmatically.
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();

        let runner = Arc::new(ScriptedRunner::new([
            // plan
            (
                "plan tasks".to_string(),
                vec![ScriptedStep::success("tasks: a,b")],
            ),
            // batch items — ScriptedRunner matches the resolved prompt after var substitution
            (
                "implement a".to_string(),
                vec![ScriptedStep::success("impl-a done")],
            ),
            (
                "implement b".to_string(),
                vec![ScriptedStep::success("impl-b done")],
            ),
            // dual-review subflow body
            (
                "review tasks: a,b".to_string(),
                vec![ScriptedStep::success("review ok")],
            ),
            // consolidate
            (
                "consolidate".to_string(),
                vec![ScriptedStep::success("consolidated")],
            ),
        ]));
        let runtime = RuntimeContext::with_runner(db.clone(), runner);

        // Dual-review subflow: single task node (acts as exit).
        let review_sub = workflow_from_parts(
            "review",
            vec![task_node("review", "Review", "review {{var:scope}}")],
            vec![],
        );

        let mut wf = workflow_from_parts(
            "plan",
            vec![
                run_agent_node("plan", "Plan", "mock", "plan tasks"),
                parallel_batch_node(
                    "batch",
                    "tasks",
                    2,
                    "task",
                    "impl_body",
                    Some("impl_results"),
                ),
                task_node("impl_body", "Impl", "implement {{var:task}}"),
                approval_node("merge_gate", "Merge Gate", "approve merge?"),
                call_node(
                    "review_call",
                    "dual_review",
                    "review",
                    vec![InputBinding {
                        name: "scope".to_string(),
                        source: "node:plan.output".to_string(),
                    }],
                ),
                task_node("consolidate", "Consolidate", "consolidate"),
            ],
            vec![
                success_edge("plan_batch", "plan", "batch", None),
                success_edge("batch_gate", "batch", "merge_gate", None),
                success_edge("gate_review", "merge_gate", "review_call", None),
                success_edge("review_cons", "review_call", "consolidate", None),
            ],
        );
        wf.variables = vec![WorkflowVariable {
            name: "tasks".to_string(),
            default: "[]".to_string(),
        }];
        wf.subflows
            .insert("dual_review".to_string(), Box::new(review_sub));

        let mut vars = BTreeMap::new();
        vars.insert("tasks".to_string(), json!(["a", "b"]).to_string());
        let run_id = runtime.start_run(wf, vars, None).await.unwrap();

        // Wait for the merge gate approval
        wait_for_run(&db, &run_id, |p| p.checkpoint.pending_approval.is_some()).await;
        runtime
            .approve_run(&run_id, true, "merged".to_string())
            .await
            .unwrap();

        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(
            persisted.checkpoint.status,
            RuntimeStatus::Completed,
            "multi-agent plan should complete through consolidation"
        );
        assert!(persisted.checkpoint.all_results.contains_key("plan"));
        assert!(persisted.checkpoint.all_results.contains_key("batch"));
        assert!(persisted.checkpoint.all_results.contains_key("review_call"));
        assert_eq!(
            persisted.checkpoint.all_results["consolidate"].output,
            "consolidated"
        );
    }

    #[tokio::test]
    async fn bundled_multi_agent_plan_template_runs_all_three_batches() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(db.clone(), Arc::new(FlagshipTemplateRunner));

        let template_json =
            std::fs::read_to_string("templates/multi-agent-plan-implementation.json").unwrap();
        let mut workflow = normalize_workflow_value(serde_json::from_str(&template_json).unwrap())
            .unwrap()
            .workflow;
        let review_subflow = workflow_from_parts(
            "review",
            vec![run_agent_node(
                "review",
                "Review",
                "mock",
                "Review batch results: {{var:review_scope}}",
            )],
            Vec::new(),
        );
        workflow
            .subflows
            .insert("dual-review".to_string(), Box::new(review_subflow));

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        wait_for_run(&db, &run_id, |persisted| {
            persisted
                .checkpoint
                .pending_approval
                .as_ref()
                .is_some_and(|pending| pending.node_id == "merge-gate")
        })
        .await;
        runtime
            .approve_run(&run_id, true, "merge approved".to_string())
            .await
            .unwrap();

        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Completed);
        for batch_id in ["batch-impl", "batch-test", "batch-fixes"] {
            let batch = persisted.checkpoint.all_results.get(batch_id).unwrap();
            let parsed = batch.parsed_output.as_ref().unwrap();
            assert_eq!(parsed["summary"]["total"], json!(2));
            assert_eq!(parsed["summary"]["succeeded"], json!(2));
        }
        assert_eq!(
            persisted.checkpoint.all_results["consolidate"].output,
            "consolidated"
        );
    }

    #[tokio::test]
    async fn loop_cap_without_exit_marks_run_failed() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let runtime = RuntimeContext::with_runner(
            db.clone(),
            Arc::new(ScriptedRunner::new([(
                "loop".to_string(),
                vec![ScriptedStep::success("iter")],
            )])),
        );
        let mut gate = task_node("gate", "Gate", "loop");
        gate.loop_max_iterations = Some(1);
        let workflow = workflow_from_parts(
            "gate",
            vec![gate, task_node("done", "Done", "done")],
            vec![WorkflowEdge {
                id: "gate_continue".to_string(),
                from: "gate".to_string(),
                to: "gate".to_string(),
                outcome: WorkflowEdgeOutcome::LoopContinue,
                label: Some("again".to_string()),
                branch_id: None,
                condition: None,
            }],
        );

        let run_id = runtime
            .start_run(workflow, BTreeMap::new(), None)
            .await
            .unwrap();
        let persisted = wait_for_terminal_run(&db, &run_id).await;

        assert_eq!(persisted.checkpoint.status, RuntimeStatus::Failed);
        let events = db.list_events(&run_id).await.unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "workflow_error"
                && event
                    .data
                    .get("message")
                    .and_then(|value| value.as_str())
                    .is_some_and(|message| message.contains("no loop_exit edge"))
        }));
    }

    #[tokio::test]
    async fn decide_missing_branch_edge_degrades_instead_of_bailing() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let ctx = RuntimeContext::with_runner(db.clone(), Arc::new(EchoRunner));

        let mut decide = task_node("decide", "Decide", "");
        decide.kind = NodeKind::Decide {
            decide_config: crate::model::DecideConfig {
                inputs: Vec::new(),
                prompt: "pick".to_string(),
                model: None,
                outcomes: vec!["yes".to_string()],
            },
        };
        decide.agent = None;

        let workflow = workflow_from_parts(
            "decide",
            vec![decide, task_node("after", "After", "after")],
            vec![success_edge("decide_after", "decide", "after", None)],
        );
        let mut checkpoint =
            build_initial_checkpoint(&workflow, "run_decide", BTreeMap::new(), None);
        let graph = workflow.graph();
        let node = workflow.nodes[0].clone();
        let cursor_id = checkpoint.active_cursors[0].cursor_id.clone();
        let result = NodeResult {
            success: true,
            output: "missing".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration: "0".to_string(),
            agent: "llm".to_string(),
            prompt: String::new(),
            raw_output: None,
            parsed_output: None,
            parse_error: None,
            resolved_prompt: None,
            stale: false,
            preserved_from_run_id: None,
            metadata: Default::default(),
        };

        let decision = select_next_decision(
            &ctx,
            &workflow,
            &graph,
            &mut checkpoint,
            &cursor_id,
            &node,
            &result,
            1,
        )
        .await
        .unwrap();

        assert_eq!(decision.control_type, "success");
        assert_eq!(
            decision.next_edge.as_ref().map(|edge| edge.to.as_str()),
            Some("after")
        );
        let events = db.list_events(&checkpoint.run_id).await.unwrap();
        assert!(events.iter().any(|event| {
            event.kind == "workflow_error"
                && event
                    .data
                    .get("message")
                    .and_then(|value| value.as_str())
                    .is_some_and(|message| message.contains("no matching branch edge"))
        }));
    }
}
