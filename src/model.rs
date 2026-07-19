use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::driver::{AccessMode, AgentConfig, ReasoningLevel, ToolToggles};

pub const WORKFLOW_SCHEMA_VERSION: u32 = 4;
const LEGACY_WORKFLOW_SCHEMA_VERSION: u64 = 2;
const FLAT_NODE_WORKFLOW_SCHEMA_VERSION: u64 = 3;
pub(crate) const MAX_WORKFLOW_SUBFLOWS: usize = 1_024;
pub(crate) const MAX_SUBFLOW_CALL_EDGES: usize = 4_096;

// ---------------------------------------------------------------------------
// Orchestrator configuration
// ---------------------------------------------------------------------------

/// When the orchestrator LLM is activated to classify ambiguous output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratorActivation {
    /// Only call orchestrator after N seconds of no output (default).
    #[default]
    StaleOnly,
    /// Classify all output that doesn't match a known regex pattern.
    AlwaysOn,
}

/// Configuration for the orchestrator LLM that classifies ambiguous PTY output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub activation: OrchestratorActivation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_timeout_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_timeout_secs: Option<u32>,
}

/// Default agent used when a node has no explicit `agent` field.
pub const DEFAULT_AGENT: &str = "claude";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowVariable {
    pub name: String,
    #[serde(default)]
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContextSource {
    pub name: String,
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StructuredCondition {
    pub field: String,
    pub operator: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkipCondition {
    #[serde(default = "default_previous_output_source")]
    pub source: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
}

fn default_previous_output_source() -> String {
    "previous_output".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResponseFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeType {
    Task,
    Approval,
    Split,
    Collector,
    Decide,
    ParallelBatch,
    Subflow,
    Call,
    Spawn,
    Send,
    Wait,
    Capture,
    Kill,
    RunAgent,
}

impl WorkflowNodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowNodeType::Task => "task",
            WorkflowNodeType::Approval => "approval",
            WorkflowNodeType::Split => "split",
            WorkflowNodeType::Collector => "collector",
            WorkflowNodeType::Decide => "decide",
            WorkflowNodeType::ParallelBatch => "parallel_batch",
            WorkflowNodeType::Subflow => "subflow",
            WorkflowNodeType::Call => "call",
            WorkflowNodeType::Spawn => "spawn",
            WorkflowNodeType::Send => "send",
            WorkflowNodeType::Wait => "wait",
            WorkflowNodeType::Capture => "capture",
            WorkflowNodeType::Kill => "kill",
            WorkflowNodeType::RunAgent => "run_agent",
        }
    }
}

fn is_default<T>(value: &T) -> bool
where
    T: Default + PartialEq,
{
    value == &T::default()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SplitFailurePolicy {
    #[default]
    BestEffortContinue,
    FailFastCancel,
    DrainThenFail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEdgeOutcome {
    Success,
    Reject,
    Branch,
    LoopContinue,
    LoopExit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowLimits {
    #[serde(default = "default_max_total_steps")]
    pub max_total_steps: u32,
    #[serde(default = "default_max_visits_per_node")]
    pub max_visits_per_node: u32,
}

pub fn default_max_total_steps() -> u32 {
    50
}

pub fn default_max_visits_per_node() -> u32 {
    10
}

/// True when limits are absent-equivalent (`{0,0}`) or explicitly canonical (`{50,10}`).
pub fn limits_are_canonical(limits: &WorkflowLimits) -> bool {
    (limits.max_total_steps == 0 && limits.max_visits_per_node == 0)
        || (limits.max_total_steps == default_max_total_steps()
            && limits.max_visits_per_node == default_max_visits_per_node())
}

pub fn default_max_call_depth() -> u32 {
    10
}

// ---------------------------------------------------------------------------
// Agent configuration types (workflow-level defaults + per-node overrides)
// ---------------------------------------------------------------------------

/// Workflow-level default agent configuration, keyed by agent name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_level: Option<ReasoningLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_mode: Option<AccessMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_toggles: Option<ToolToggles>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestrator: Option<OrchestratorConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunAsConfig {
    pub user: Option<String>,
    pub command: Option<Vec<String>>,
    pub socket: Option<String>,
}

/// Per-node agent configuration override. Extends `AgentDefaults` with fine-grained tool control.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentNodeConfig {
    #[serde(flatten)]
    pub base: AgentDefaults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disallowed_tools: Option<Vec<String>>,
}

/// Merge workflow-level agent defaults + per-node overrides into a resolved `AgentConfig`.
///
/// Priority: node `agent_config` → workflow `agent_defaults[agent]` → built-in defaults.
pub fn resolve_agent_config(
    workflow_defaults: &BTreeMap<String, AgentDefaults>,
    workflow_cwd: &str,
    agent_name: &str,
    node: &WorkflowNode,
    resume_session_id: Option<String>,
    needs_session_persistence: bool,
    json_schema: Option<Value>,
) -> AgentConfig {
    let defaults = workflow_defaults.get(agent_name);
    let node_base = node.kind.agent_config().map(|o| &o.base);

    // Helper: node override → workflow default → None
    macro_rules! merge {
        ($field:ident) => {
            node_base
                .and_then(|o| o.$field.clone())
                .or_else(|| defaults.and_then(|d| d.$field.clone()))
        };
    }

    let tool_toggles = merge!(tool_toggles).unwrap_or_default();
    let access_mode = merge!(access_mode).unwrap_or_default();
    let access_profile_override = match &node.kind {
        NodeKind::RunAgent {
            run_agent_config, ..
        } => run_agent_config.access.clone(),
        _ => None,
    };

    let cwd = node.cwd.clone().unwrap_or_else(|| workflow_cwd.to_string());

    let overrides = node.kind.agent_config();

    AgentConfig {
        model: merge!(model),
        reasoning_level: merge!(reasoning_level),
        system_prompt: merge!(system_prompt),
        max_turns: merge!(max_turns),
        max_budget_usd: merge!(max_budget_usd),
        resume_session_id,
        ephemeral_session: !needs_session_persistence,
        json_schema,
        access_mode,
        access_profile_override,
        tool_toggles,
        allowed_tools: overrides.and_then(|o| o.allowed_tools.clone()),
        disallowed_tools: overrides.and_then(|o| o.disallowed_tools.clone()),
        cwd,
        auto_approve: merge!(auto_approve).unwrap_or(false),
        orchestrator: merge!(orchestrator),
    }
}

fn default_split_failure_policy() -> SplitFailurePolicy {
    SplitFailurePolicy::BestEffortContinue
}

fn is_default_split_failure_policy(policy: &SplitFailurePolicy) -> bool {
    *policy == SplitFailurePolicy::BestEffortContinue
}

fn deserialize_split_failure_policy<'de, D>(deserializer: D) -> Result<SplitFailurePolicy, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<SplitFailurePolicy>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_else(default_split_failure_policy))
}

/// Deserializes `output_schema` accepting both the legacy `{"field": "type"}` format
/// and full JSON Schema objects. Legacy format is auto-converted to a proper JSON Schema.
fn deserialize_output_schema<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    Ok(Some(migrate_output_schema(value)))
}

/// If value is a flat `{"field": "type"}` object (legacy format), convert to JSON Schema.
/// Otherwise, return as-is (already a proper JSON Schema).
pub fn migrate_output_schema(value: Value) -> Value {
    let Some(obj) = value.as_object() else {
        return value;
    };
    // Detect legacy format: all values are plain strings (not objects/arrays)
    // and the object has no "type" key (which would indicate it's already a JSON Schema).
    let is_legacy =
        !obj.is_empty() && !obj.contains_key("type") && obj.values().all(|v| v.is_string());
    if !is_legacy {
        return value;
    }
    // Convert legacy {"field": "type_hint"} → JSON Schema
    let properties: serde_json::Map<String, Value> = obj
        .iter()
        .map(|(field, ty)| {
            (
                field.clone(),
                serde_json::json!({ "type": ty.as_str().unwrap_or("string") }),
            )
        })
        .collect();
    let required: Vec<Value> = obj.keys().map(|k| Value::String(k.clone())).collect();
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InputBinding {
    pub name: String,
    pub source: String,
}

pub fn default_decide_model() -> String {
    "claude-haiku-4-5".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DecideConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputBinding>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<String>,
}

pub fn default_batch_max_concurrent() -> u32 {
    4
}

/// Upper bound for parallel_batch maxConcurrent at runtime (and in the editor).
pub const MAX_PARALLEL_BATCH_CONCURRENT: u32 = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchConfig {
    pub items_binding: String,
    #[serde(default = "default_batch_max_concurrent")]
    pub max_concurrent: u32,
    pub item_var: String,
    pub body_entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collector_var: Option<String>,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            items_binding: String::new(),
            max_concurrent: default_batch_max_concurrent(),
            item_var: String::new(),
            body_entry: String::new(),
            collector_var: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpawnConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SendConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub text: String,
    pub enter: bool,
}

impl Default for SendConfig {
    fn default() -> Self {
        Self {
            target: None,
            text: String::new(),
            enter: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaitMode {
    #[default]
    Idle,
    Ready,
    Until,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WaitConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub mode: WaitMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_stable_seconds: Option<f64>,
}

impl Default for WaitConfig {
    fn default() -> Self {
        Self {
            target: None,
            mode: WaitMode::Idle,
            marker: None,
            timeout: None,
            idle_seconds: None,
            ready_stable_seconds: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub ansi: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct KillConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct RunAgentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_stable_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    pub kill_after: bool,
}

impl Default for RunAgentConfig {
    fn default() -> Self {
        Self {
            agent: None,
            prompt: None,
            cwd: None,
            access: None,
            extra_args: Vec::new(),
            name: None,
            timeout: None,
            idle_seconds: None,
            ready_stable_seconds: None,
            until: None,
            kill_after: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubflowConfig {
    #[serde(default, alias = "workflow", alias = "subflowName")]
    pub workflow_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputBinding>,
    #[serde(default = "default_max_call_depth")]
    pub max_depth: u32,
}

impl Default for SubflowConfig {
    fn default() -> Self {
        Self {
            workflow_name: String::new(),
            exit_node_id: None,
            inputs: Vec::new(),
            max_depth: default_max_call_depth(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCanvasViewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for WorkflowCanvasViewport {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCanvasNodeState {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCanvasUi {
    #[serde(default)]
    pub viewport: WorkflowCanvasViewport,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nodes: BTreeMap<String, WorkflowCanvasNodeState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowUi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<WorkflowCanvasUi>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeKind {
    Task {
        #[serde(
            default,
            rename = "agentConfig",
            skip_serializing_if = "Option::is_none"
        )]
        agent_config: Option<AgentNodeConfig>,
    },
    Approval,
    Split,
    Collector,
    Decide {
        #[serde(default, rename = "decideConfig")]
        decide_config: DecideConfig,
    },
    ParallelBatch {
        #[serde(default, rename = "batchConfig", alias = "parallelBatchConfig")]
        batch_config: BatchConfig,
    },
    Subflow {
        #[serde(default, rename = "subflowConfig")]
        subflow_config: SubflowConfig,
    },
    Call {
        #[serde(default, rename = "subflowConfig")]
        subflow_config: SubflowConfig,
    },
    Spawn {
        #[serde(default, rename = "spawnConfig", skip_serializing_if = "is_default")]
        spawn_config: SpawnConfig,
    },
    Send {
        #[serde(default, rename = "sendConfig", skip_serializing_if = "is_default")]
        send_config: SendConfig,
    },
    Wait {
        #[serde(default, rename = "waitConfig", skip_serializing_if = "is_default")]
        wait_config: WaitConfig,
    },
    Capture {
        #[serde(default, rename = "captureConfig", skip_serializing_if = "is_default")]
        capture_config: CaptureConfig,
    },
    Kill {
        #[serde(default, rename = "killConfig", skip_serializing_if = "is_default")]
        kill_config: KillConfig,
    },
    RunAgent {
        #[serde(default, rename = "runAgentConfig", skip_serializing_if = "is_default")]
        run_agent_config: RunAgentConfig,
        #[serde(
            default,
            rename = "agentConfig",
            skip_serializing_if = "Option::is_none"
        )]
        agent_config: Option<AgentNodeConfig>,
    },
}

impl NodeKind {
    pub fn node_type(&self) -> WorkflowNodeType {
        match self {
            NodeKind::Task { .. } => WorkflowNodeType::Task,
            NodeKind::Approval => WorkflowNodeType::Approval,
            NodeKind::Split => WorkflowNodeType::Split,
            NodeKind::Collector => WorkflowNodeType::Collector,
            NodeKind::Decide { .. } => WorkflowNodeType::Decide,
            NodeKind::ParallelBatch { .. } => WorkflowNodeType::ParallelBatch,
            NodeKind::Subflow { .. } => WorkflowNodeType::Subflow,
            NodeKind::Call { .. } => WorkflowNodeType::Call,
            NodeKind::Spawn { .. } => WorkflowNodeType::Spawn,
            NodeKind::Send { .. } => WorkflowNodeType::Send,
            NodeKind::Wait { .. } => WorkflowNodeType::Wait,
            NodeKind::Capture { .. } => WorkflowNodeType::Capture,
            NodeKind::Kill { .. } => WorkflowNodeType::Kill,
            NodeKind::RunAgent { .. } => WorkflowNodeType::RunAgent,
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.node_type().as_str()
    }

    pub fn agent_config(&self) -> Option<&AgentNodeConfig> {
        match self {
            NodeKind::Task { agent_config } | NodeKind::RunAgent { agent_config, .. } => {
                agent_config.as_ref()
            }
            NodeKind::Approval
            | NodeKind::Split
            | NodeKind::Collector
            | NodeKind::Decide { .. }
            | NodeKind::ParallelBatch { .. }
            | NodeKind::Subflow { .. }
            | NodeKind::Call { .. }
            | NodeKind::Spawn { .. }
            | NodeKind::Send { .. }
            | NodeKind::Wait { .. }
            | NodeKind::Capture { .. }
            | NodeKind::Kill { .. } => None,
        }
    }

    pub fn set_agent_config(&mut self, config: Option<AgentNodeConfig>) {
        match self {
            NodeKind::Task { agent_config } | NodeKind::RunAgent { agent_config, .. } => {
                *agent_config = config;
            }
            NodeKind::Approval
            | NodeKind::Split
            | NodeKind::Collector
            | NodeKind::Decide { .. }
            | NodeKind::ParallelBatch { .. }
            | NodeKind::Subflow { .. }
            | NodeKind::Call { .. }
            | NodeKind::Spawn { .. }
            | NodeKind::Send { .. }
            | NodeKind::Wait { .. }
            | NodeKind::Capture { .. }
            | NodeKind::Kill { .. } => {}
        }
    }
}

impl From<WorkflowNodeType> for NodeKind {
    fn from(node_type: WorkflowNodeType) -> Self {
        match node_type {
            WorkflowNodeType::Task => NodeKind::Task { agent_config: None },
            WorkflowNodeType::Approval => NodeKind::Approval,
            WorkflowNodeType::Split => NodeKind::Split,
            WorkflowNodeType::Collector => NodeKind::Collector,
            WorkflowNodeType::Decide => NodeKind::Decide {
                decide_config: DecideConfig::default(),
            },
            WorkflowNodeType::ParallelBatch => NodeKind::ParallelBatch {
                batch_config: BatchConfig::default(),
            },
            WorkflowNodeType::Subflow => NodeKind::Subflow {
                subflow_config: SubflowConfig::default(),
            },
            WorkflowNodeType::Call => NodeKind::Call {
                subflow_config: SubflowConfig::default(),
            },
            WorkflowNodeType::Spawn => NodeKind::Spawn {
                spawn_config: SpawnConfig::default(),
            },
            WorkflowNodeType::Send => NodeKind::Send {
                send_config: SendConfig::default(),
            },
            WorkflowNodeType::Wait => NodeKind::Wait {
                wait_config: WaitConfig::default(),
            },
            WorkflowNodeType::Capture => NodeKind::Capture {
                capture_config: CaptureConfig::default(),
            },
            WorkflowNodeType::Kill => NodeKind::Kill {
                kill_config: KillConfig::default(),
            },
            WorkflowNodeType::RunAgent => NodeKind::RunAgent {
                run_agent_config: RunAgentConfig::default(),
                agent_config: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    pub id: String,
    pub name: String,
    /// Workflow schema v4 persists node kind in this exact nested, internally
    /// tagged shape:
    /// `{ "id": "...", "name": "...", "kind": { "type": "task", "agentConfig": { ... } }, "agent": "claude", "prompt": "..." }`.
    /// Variant-specific config fields such as `decideConfig`, `batchConfig`,
    /// `spawnConfig`, `runAgentConfig`, `captureConfig`, and `killConfig` live
    /// inside `kind`; common execution/UI fields stay on the node object.
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_sources: Vec<ContextSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_output_schema"
    )]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_delay: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_condition: Option<SkipCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_max_iterations: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_condition: Option<StructuredCondition>,
    #[serde(
        default = "default_split_failure_policy",
        deserialize_with = "deserialize_split_failure_policy",
        skip_serializing_if = "is_default_split_failure_policy"
    )]
    pub split_failure_policy: SplitFailurePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continue_session_from: Option<String>,
}

impl WorkflowNode {
    pub fn node_type(&self) -> WorkflowNodeType {
        self.kind.node_type()
    }

    pub fn node_type_str(&self) -> &'static str {
        self.kind.as_str()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub outcome: WorkflowEdgeOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<StructuredCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowV3 {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub use_orchestrator: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_as: Option<RunAsConfig>,
    pub entry_node_id: String,
    #[serde(default)]
    pub variables: Vec<WorkflowVariable>,
    #[serde(default)]
    pub limits: WorkflowLimits,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_defaults: BTreeMap<String, AgentDefaults>,
    /// Root-level catalog of saved workflows referenced by subflow/call nodes.
    /// Subflow names are globally scoped at the root; nested catalogs on subflow
    /// bodies are unsupported and rejected at validation ingress.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subflows: BTreeMap<String, Box<WorkflowV3>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<WorkflowUi>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphMetadata {
    #[serde(default)]
    pub reachable_node_ids: Vec<String>,
    #[serde(default)]
    pub unreachable_node_ids: Vec<String>,
    #[serde(default)]
    pub dead_end_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedWorkflow {
    pub workflow: WorkflowV3,
    #[serde(default)]
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub workflow: WorkflowV3,
    #[serde(default)]
    pub notices: Vec<String>,
    #[serde(default)]
    pub issues: Vec<ValidationIssue>,
    pub graph: GraphMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct WorkflowGraph<'a> {
    pub node_map: HashMap<&'a str, &'a WorkflowNode>,
    pub outgoing: HashMap<&'a str, Vec<&'a WorkflowEdge>>,
    pub inbound: HashMap<&'a str, Vec<&'a WorkflowEdge>>,
}

impl WorkflowV3 {
    pub fn graph(&self) -> WorkflowGraph<'_> {
        let mut node_map = HashMap::new();
        let mut outgoing: HashMap<&str, Vec<&WorkflowEdge>> = HashMap::new();
        let mut inbound: HashMap<&str, Vec<&WorkflowEdge>> = HashMap::new();
        for node in &self.nodes {
            node_map.insert(node.id.as_str(), node);
        }
        for edge in &self.edges {
            outgoing.entry(edge.from.as_str()).or_default().push(edge);
            inbound.entry(edge.to.as_str()).or_default().push(edge);
        }
        WorkflowGraph {
            node_map,
            outgoing,
            inbound,
        }
    }
}

impl<'a> WorkflowGraph<'a> {
    pub fn outgoing_for(&self, node_id: &str) -> &[&'a WorkflowEdge] {
        self.outgoing.get(node_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn inbound_for(&self, node_id: &str) -> &[&'a WorkflowEdge] {
        self.inbound.get(node_id).map(Vec::as_slice).unwrap_or(&[])
    }
}

pub fn normalize_workflow_value(value: Value) -> anyhow::Result<NormalizedWorkflow> {
    let mut value = value;
    let mut notices = Vec::new();
    let version = migrate_workflow_value_to_v4(&mut value)?;
    if version != WORKFLOW_SCHEMA_VERSION as u64 {
        notices.push(format!(
            "Migrated workflow schema from version {version} to version {WORKFLOW_SCHEMA_VERSION}."
        ));
    }
    let workflow: WorkflowV3 =
        serde_json::from_value(value).context("failed to deserialize workflow")?;
    Ok(NormalizedWorkflow {
        workflow: ensure_defaults(workflow),
        notices,
    })
}

pub fn ensure_defaults(mut workflow: WorkflowV3) -> WorkflowV3 {
    workflow.version = WORKFLOW_SCHEMA_VERSION;
    if limits_are_canonical(&workflow.limits) {
        if workflow.limits.max_total_steps == 0 {
            workflow.limits.max_total_steps = default_max_total_steps();
            workflow.limits.max_visits_per_node = default_max_visits_per_node();
        }
    } else {
        if workflow.limits.max_total_steps == 0 {
            workflow.limits.max_total_steps = default_max_total_steps();
        }
        if workflow.limits.max_visits_per_node == 0 {
            workflow.limits.max_visits_per_node = default_max_visits_per_node();
        }
    }
    workflow
}

fn migrate_workflow_value_to_v4(value: &mut Value) -> anyhow::Result<u64> {
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .context("workflow version is required")?;
    match version {
        LEGACY_WORKFLOW_SCHEMA_VERSION | FLAT_NODE_WORKFLOW_SCHEMA_VERSION => {}
        version if version == WORKFLOW_SCHEMA_VERSION as u64 => {}
        version => anyhow::bail!(
            "Only workflow versions 2, 3, and 4 are supported. Received version {}.",
            version
        ),
    }

    let workflow = value
        .as_object_mut()
        .context("workflow must be a JSON object")?;

    migrate_v2_nodes_to_v3_kind(workflow)?;

    if let Some(subflows) = workflow.get_mut("subflows") {
        let subflows = subflows
            .as_object_mut()
            .context("workflow subflows must be a JSON object")?;
        for (name, subflow) in subflows {
            migrate_workflow_value_to_v4(subflow)
                .with_context(|| format!("failed to migrate subflow \"{}\"", name))?;
        }
    }

    workflow.insert("version".to_string(), Value::from(WORKFLOW_SCHEMA_VERSION));
    Ok(version)
}

fn migrate_v2_nodes_to_v3_kind(workflow: &mut Map<String, Value>) -> anyhow::Result<()> {
    let Some(nodes) = workflow.get_mut("nodes") else {
        return Ok(());
    };
    let nodes = nodes
        .as_array_mut()
        .context("workflow nodes must be a JSON array")?;
    for node in nodes {
        let node_object = node
            .as_object()
            .context("workflow node must be a JSON object")?;
        if !node_object.contains_key("kind") && node_object.contains_key("type") {
            migrate_v2_node_to_v3_kind(node)?;
        }
    }
    Ok(())
}

fn migrate_v2_node_to_v3_kind(node: &mut Value) -> anyhow::Result<()> {
    let node = node
        .as_object_mut()
        .context("workflow node must be a JSON object")?;
    if node.contains_key("kind") {
        return Ok(());
    }

    let node_type = node
        .remove("type")
        .context("workflow node type is required for version 2 migration")?;
    let node_type = node_type
        .as_str()
        .context("workflow node type must be a string for version 2 migration")?
        .to_string();
    let mut kind = Map::new();
    kind.insert("type".to_string(), Value::String(node_type.clone()));

    match node_type.as_str() {
        "task" => {
            move_v2_config_field(node, &mut kind, "agentConfig", "agentConfig");
        }
        "approval" | "split" | "collector" => {}
        "decide" => {
            move_v2_config_field(node, &mut kind, "decideConfig", "decideConfig");
        }
        "parallel_batch" => {
            move_v2_batch_config_field(node, &mut kind);
        }
        "subflow" | "call" => {
            move_v2_config_field(node, &mut kind, "subflowConfig", "subflowConfig");
        }
        "spawn" => {
            move_v2_config_field(node, &mut kind, "spawnConfig", "spawnConfig");
        }
        "send" => {
            move_v2_config_field(node, &mut kind, "sendConfig", "sendConfig");
        }
        "wait" => {
            move_v2_config_field(node, &mut kind, "waitConfig", "waitConfig");
        }
        "capture" => {
            move_v2_config_field(node, &mut kind, "captureConfig", "captureConfig");
            if !kind.contains_key("captureConfig") {
                kind.insert("captureConfig".to_string(), Value::Object(Map::new()));
            }
        }
        "kill" => {
            move_v2_config_field(node, &mut kind, "killConfig", "killConfig");
            if !kind.contains_key("killConfig") {
                kind.insert("killConfig".to_string(), Value::Object(Map::new()));
            }
        }
        "run_agent" => {
            move_v2_config_field(node, &mut kind, "runAgentConfig", "runAgentConfig");
            move_v2_config_field(node, &mut kind, "agentConfig", "agentConfig");
        }
        _ => {}
    }

    for field in [
        "agentConfig",
        "decideConfig",
        "batchConfig",
        "parallelBatchConfig",
        "subflowConfig",
        "spawnConfig",
        "sendConfig",
        "waitConfig",
        "captureConfig",
        "killConfig",
        "runAgentConfig",
    ] {
        node.remove(field);
    }

    node.insert("kind".to_string(), Value::Object(kind));
    Ok(())
}

fn move_v2_config_field(
    node: &mut Map<String, Value>,
    kind: &mut Map<String, Value>,
    from: &str,
    to: &str,
) {
    if let Some(value) = node.remove(from)
        && !value.is_null()
    {
        kind.insert(to.to_string(), value);
    }
}

fn move_v2_batch_config_field(node: &mut Map<String, Value>, kind: &mut Map<String, Value>) {
    if let Some(value) = node
        .remove("batchConfig")
        .or_else(|| node.remove("parallelBatchConfig"))
        && !value.is_null()
    {
        kind.insert("batchConfig".to_string(), value);
    }
}

fn validate_absolute_cwd(
    cwd: &str,
    label: &str,
    node_id: Option<&str>,
    issues: &mut Vec<ValidationIssue>,
) {
    if cwd.is_empty() {
        return;
    }
    if !Path::new(cwd).is_absolute() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: node_id.map(ToString::to_string),
            scope: None,
            message: format!("{label} must be an absolute path (got \"{cwd}\")."),
        });
    }
}

/// Whether a `WorkflowV3` body is the root document or a nested subflow body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowBodyScope {
    Root,
    Subflow,
}

/// Root-only execution settings that may exist on subflow bodies as dead data.
fn validate_subflow_body_root_only_fields(
    workflow: &WorkflowV3,
    subflow_name: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if workflow.run_as.is_some() {
        issues.push(ValidationIssue {
            severity: "warning".to_string(),
            node_id: None,
            scope: None,
            message: format!(
                "Subflow \"{subflow_name}\" defines runAs, but only the root workflow runAs is honored at execution time."
            ),
        });
    }

    if !limits_are_canonical(&workflow.limits) {
        issues.push(ValidationIssue {
            severity: "warning".to_string(),
            node_id: None,
            scope: None,
            message: format!(
                "Subflow \"{subflow_name}\" defines custom limits, but only the root workflow limits are honored at execution time."
            ),
        });
    }
}

fn validate_workflow_body_for_scope(
    workflow: &WorkflowV3,
    scope: WorkflowBodyScope,
    subflow_name: Option<&str>,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), ValidationIssue> {
    match scope {
        WorkflowBodyScope::Root => Ok(()),
        WorkflowBodyScope::Subflow => {
            let subflow_name = subflow_name.ok_or_else(|| ValidationIssue {
                severity: "error".to_string(),
                node_id: None,
                scope: None,
                message: "internal error: subflow scope requires a subflow name".to_string(),
            })?;

            if !workflow.subflows.is_empty() {
                return Err(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: None,
                    scope: None,
                    message: format!(
                        "Subflow \"{subflow_name}\" contains a nested subflow catalog; subflow names are globally scoped at the root and nested catalogs are unsupported."
                    ),
                });
            }

            validate_subflow_body_root_only_fields(workflow, subflow_name, issues);
            Ok(())
        }
    }
}

fn reject_nested_subflow_catalogs(workflow: &WorkflowV3) -> Result<(), ValidationIssue> {
    let mut issues = Vec::new();
    for (subflow_name, subflow) in &workflow.subflows {
        validate_workflow_body_for_scope(
            subflow,
            WorkflowBodyScope::Subflow,
            Some(subflow_name),
            &mut issues,
        )?;
    }
    Ok(())
}

pub fn validate_workflow(workflow: WorkflowV3) -> ValidationResult {
    let workflow = ensure_defaults(workflow);
    if let Err(issue) = validate_workflow_input_bounds(&workflow) {
        return ValidationResult {
            workflow,
            notices: Vec::new(),
            issues: vec![issue],
            graph: GraphMetadata::default(),
        };
    }
    let mut issues = Vec::new();

    validate_run_as_config(workflow.run_as.as_ref(), &mut issues);
    let _ = validate_workflow_body_for_scope(&workflow, WorkflowBodyScope::Root, None, &mut issues);
    for (subflow_name, subflow) in &workflow.subflows {
        validate_subflow_body_root_only_fields(subflow, subflow_name, &mut issues);
    }
    validate_graph_body(&workflow, &workflow.subflows, "", &mut issues);

    let graph = workflow.graph();
    if graph
        .node_map
        .get(workflow.entry_node_id.as_str())
        .is_none()
    {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: None,
            scope: None,
            message: format!(
                "entryNodeId \"{}\" references a non-existent node.",
                workflow.entry_node_id
            ),
        });
    }

    validate_subflow_catalog(&workflow, &mut issues);
    validate_subflow_call_cycles(&workflow, &mut issues);

    let graph_meta = compute_graph_metadata(&workflow);
    for node_id in &graph_meta.unreachable_node_ids {
        let name = graph
            .node_map
            .get(node_id.as_str())
            .map(|node| node.name.clone())
            .unwrap_or_else(|| node_id.clone());
        issues.push(ValidationIssue {
            severity: "warning".to_string(),
            node_id: Some(node_id.clone()),
            scope: None,
            message: format!("\"{}\" is unreachable from the entry node.", name),
        });
    }

    ValidationResult {
        workflow,
        notices: Vec::new(),
        issues,
        graph: graph_meta,
    }
}

pub(crate) fn validate_workflow_input_bounds(workflow: &WorkflowV3) -> Result<(), ValidationIssue> {
    reject_nested_subflow_catalogs(workflow)?;

    let subflow_count = workflow.subflows.len();
    if subflow_count > MAX_WORKFLOW_SUBFLOWS {
        return Err(ValidationIssue {
            severity: "error".to_string(),
            node_id: None,
            scope: None,
            message: format!(
                "Workflow contains {subflow_count} subflows; at most {MAX_WORKFLOW_SUBFLOWS} are allowed."
            ),
        });
    }

    let call_edge_count = std::iter::once(workflow)
        .chain(workflow.subflows.values().map(Box::as_ref))
        .flat_map(|candidate| &candidate.nodes)
        .filter(|node| match &node.kind {
            NodeKind::Subflow { subflow_config } | NodeKind::Call { subflow_config } => {
                !subflow_config.workflow_name.trim().is_empty()
            }
            _ => false,
        })
        .count();
    if call_edge_count > MAX_SUBFLOW_CALL_EDGES {
        return Err(ValidationIssue {
            severity: "error".to_string(),
            node_id: None,
            scope: None,
            message: format!(
                "Workflow contains {call_edge_count} subflow call edges; at most {MAX_SUBFLOW_CALL_EDGES} are allowed."
            ),
        });
    }

    Ok(())
}

fn validate_graph_body(
    workflow: &WorkflowV3,
    subflows: &BTreeMap<String, Box<WorkflowV3>>,
    prefix: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let issue_start = issues.len();

    if !workflow.cwd.is_empty() {
        let label = if prefix.is_empty() {
            "Workflow cwd".to_string()
        } else {
            format!("{prefix} cwd")
        };
        validate_absolute_cwd(&workflow.cwd, &label, None, issues);
    }

    let graph = workflow.graph();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();

    for node in &workflow.nodes {
        if !seen_nodes.insert(node.id.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!("Duplicate node id \"{}\".", node.id),
            });
        }

        if node.output_schema.is_some() && node.response_format != Some(ResponseFormat::Json) {
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" has an output schema but responseFormat is not json.",
                    node.name
                ),
            });
        }

        if let Some(cwd) = &node.cwd {
            validate_absolute_cwd(
                cwd,
                &format!("\"{}\" cwd", node.name),
                Some(&node.id),
                issues,
            );
        }

        if let Some(skip) = &node.skip_condition {
            if skip.kind == "regex" {
                if let Err(error) = regex_engine::Regex::new(&skip.value) {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!(
                            "\"{}\" has invalid skip condition regex: {}",
                            node.name, error
                        ),
                    });
                }
            }
        }

        let outgoing = graph.outgoing_for(&node.id);
        let inbound = graph.inbound_for(&node.id);
        let branch_edges = outgoing
            .iter()
            .filter(|edge| edge.outcome == WorkflowEdgeOutcome::Branch)
            .count();
        let loop_edges = outgoing
            .iter()
            .filter(|edge| {
                matches!(
                    edge.outcome,
                    WorkflowEdgeOutcome::LoopContinue | WorkflowEdgeOutcome::LoopExit
                )
            })
            .count();
        let success_edges = outgoing
            .iter()
            .filter(|edge| edge.outcome == WorkflowEdgeOutcome::Success)
            .count();
        let reject_edges = outgoing
            .iter()
            .filter(|edge| edge.outcome == WorkflowEdgeOutcome::Reject)
            .count();

        match &node.kind {
            NodeKind::Task { .. } => {
                if node.agent.as_deref().unwrap_or("").is_empty() {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!("\"{}\" has no agent assigned.", node.name),
                    });
                }

                if node.prompt.trim().is_empty() {
                    issues.push(ValidationIssue {
                        severity: "warning".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!("\"{}\" has an empty prompt.", node.name),
                    });
                }
            }
            NodeKind::Decide { decide_config } => {
                validate_decide_node_config(node, decide_config, outgoing, issues);
            }
            NodeKind::ParallelBatch { batch_config } => {
                validate_batch_node_config(node, batch_config, &graph, issues);
            }
            NodeKind::Subflow { subflow_config } | NodeKind::Call { subflow_config } => {
                validate_subflow_node_config(node, subflow_config, subflows, issues);
            }
            NodeKind::Spawn { spawn_config } => {
                let has_agent = spawn_config
                    .agent
                    .as_deref()
                    .or(node.agent.as_deref())
                    .is_some_and(|agent| !agent.trim().is_empty());
                let has_command = spawn_config
                    .command
                    .as_deref()
                    .is_some_and(|command| !command.trim().is_empty());
                if !has_agent && !has_command {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!(
                            "\"{}\" spawn node requires an agent or command.",
                            node.name
                        ),
                    });
                }
                if let Some(cwd) = &spawn_config.cwd {
                    validate_absolute_cwd(
                        cwd,
                        &format!("\"{}\" spawn cwd", node.name),
                        Some(&node.id),
                        issues,
                    );
                }
            }
            NodeKind::Send { send_config } => {
                let text = if send_config.text.is_empty() {
                    node.prompt.as_str()
                } else {
                    send_config.text.as_str()
                };
                if text.trim().is_empty() && node.prompt.trim().is_empty() {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!("\"{}\" send node requires text or prompt.", node.name),
                    });
                }
            }
            NodeKind::Wait { wait_config } => {
                validate_wait_timing_and_marker(
                    &node.id,
                    &node.name,
                    wait_config.marker.as_deref(),
                    wait_config.mode == WaitMode::Until,
                    wait_config.idle_seconds,
                    wait_config.ready_stable_seconds,
                    issues,
                );
            }
            NodeKind::RunAgent {
                run_agent_config, ..
            } => {
                let has_agent = run_agent_config
                    .agent
                    .as_deref()
                    .or(node.agent.as_deref())
                    .is_some_and(|agent| !agent.trim().is_empty());
                if !has_agent {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!("\"{}\" run_agent node requires an agent.", node.name),
                    });
                }
                let prompt = run_agent_config
                    .prompt
                    .as_deref()
                    .unwrap_or(node.prompt.as_str());
                if prompt.trim().is_empty() {
                    issues.push(ValidationIssue {
                        severity: "warning".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!("\"{}\" run_agent node has an empty prompt.", node.name),
                    });
                }
                if let Some(cwd) = &run_agent_config.cwd {
                    validate_absolute_cwd(
                        cwd,
                        &format!("\"{}\" run_agent cwd", node.name),
                        Some(&node.id),
                        issues,
                    );
                }
                validate_wait_timing_and_marker(
                    &node.id,
                    &node.name,
                    run_agent_config.until.as_deref(),
                    false,
                    run_agent_config.idle_seconds,
                    run_agent_config.ready_stable_seconds,
                    issues,
                );
            }
            NodeKind::Approval
            | NodeKind::Split
            | NodeKind::Collector
            | NodeKind::Capture { .. }
            | NodeKind::Kill { .. } => {}
        }

        if success_edges > 1 {
            if !matches!(&node.kind, NodeKind::Split) {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!("\"{}\" has more than one success edge.", node.name),
                });
            }
        }
        if reject_edges > 1 {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!("\"{}\" has more than one reject edge.", node.name),
            });
        }
        if branch_edges > 0 && loop_edges > 0 {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!("\"{}\" mixes branch and loop control edges.", node.name),
            });
        }
        let loop_continue_edges = outgoing
            .iter()
            .filter(|edge| edge.outcome == WorkflowEdgeOutcome::LoopContinue)
            .count();
        let loop_exit_edges = outgoing
            .iter()
            .filter(|edge| edge.outcome == WorkflowEdgeOutcome::LoopExit)
            .count();
        if loop_continue_edges > 0 && loop_exit_edges == 0 {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" has a loop_continue edge but no loop_exit edge.",
                    node.name
                ),
            });
        }
        if matches!(&node.kind, NodeKind::Approval) && branch_edges > 0 {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "Approval node \"{}\" cannot branch via agent logic.",
                    node.name
                ),
            });
        }
        if node.loop_condition.is_some() && node.response_format != Some(ResponseFormat::Json) {
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" has a loop condition but responseFormat is not json.",
                    node.name
                ),
            });
        }
        if branch_edges > 0
            && outgoing
                .iter()
                .filter(|edge| {
                    edge.outcome == WorkflowEdgeOutcome::Branch && edge.condition.is_some()
                })
                .count()
                > 0
            && node.response_format != Some(ResponseFormat::Json)
        {
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" has deterministic branch conditions but responseFormat is not json.",
                    node.name
                ),
            });
        }
        if branch_edges == 0 && loop_edges == 0 && success_edges == 0 && reject_edges == 0 {
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!("\"{}\" is a terminal node.", node.name),
            });
        }

        if matches!(&node.kind, NodeKind::Split | NodeKind::Collector)
            && has_task_execution_config(node)
        {
            let kind_label = if matches!(&node.kind, NodeKind::Split) {
                "split"
            } else {
                "collector"
            };
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" is a {} node, so task execution fields are ignored.",
                    node.name, kind_label
                ),
            });
        }

        if matches!(&node.kind, NodeKind::Split) {
            if branch_edges > 0 || loop_edges > 0 || reject_edges > 0 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!(
                        "\"{}\" can only use success edges for split fan-out.",
                        node.name
                    ),
                });
            }
            if success_edges == 0 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!("\"{}\" has no outbound split edges.", node.name),
                });
            } else if success_edges < 2 {
                issues.push(ValidationIssue {
                    severity: "warning".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!("\"{}\" fans out to fewer than two branches.", node.name),
                });
            }
        }

        if matches!(&node.kind, NodeKind::Collector) {
            if inbound.is_empty() {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!("\"{}\" has no inbound branches to collect.", node.name),
                });
            }
            if success_edges != 1 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!(
                        "\"{}\" must have exactly one outbound success edge.",
                        node.name
                    ),
                });
            }
            if branch_edges > 0 || loop_edges > 0 || reject_edges > 0 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!(
                        "\"{}\" can only use a success edge after collecting inputs.",
                        node.name
                    ),
                });
            }

            let mut seen_merge_keys = BTreeSet::new();
            for edge in inbound {
                let merge_key = edge.label.clone().unwrap_or_else(|| edge.from.clone());
                if !seen_merge_keys.insert(merge_key.clone()) {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!(
                            "\"{}\" has duplicate collector input key \"{}\".",
                            node.name, merge_key
                        ),
                    });
                }
            }
        }

        // Validate continue_session_from references
        if let Some(ref source_id) = node.continue_session_from {
            if let Some(source_node) = graph.node_map.get(source_id.as_str()) {
                // Must be a node type that can produce an agent session id.
                if !matches!(
                    &source_node.kind,
                    NodeKind::Task { .. } | NodeKind::RunAgent { .. }
                ) {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!(
                            "\"{}\" continues session from \"{}\" which is not an agent-running node.",
                            node.name, source_node.name
                        ),
                    });
                }
                // Must use the same agent
                let current_agent = node.agent.as_deref().unwrap_or(DEFAULT_AGENT);
                let source_agent = source_node.agent.as_deref().unwrap_or(DEFAULT_AGENT);
                if current_agent != source_agent {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        scope: None,
                        message: format!(
                            "\"{}\" continues session from \"{}\" but they use different agents ({} vs {}).",
                            node.name, source_node.name, current_agent, source_agent
                        ),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!(
                        "\"{}\" references unknown node \"{}\" for session continuation.",
                        node.name, source_id
                    ),
                });
            }
        }
    }

    for edge in &workflow.edges {
        if !seen_edges.insert(edge.id.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(edge.from.clone()),
                scope: None,
                message: format!("Duplicate edge id \"{}\".", edge.id),
            });
        }
        if graph.node_map.get(edge.from.as_str()).is_none() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(edge.from.clone()),
                scope: None,
                message: format!("Edge \"{}\" references unknown source node.", edge.id),
            });
        }
        if graph.node_map.get(edge.to.as_str()).is_none() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(edge.from.clone()),
                scope: None,
                message: format!("Edge \"{}\" references unknown target node.", edge.id),
            });
        }
    }

    scope_graph_body_issues(prefix, &mut issues[issue_start..]);
}

fn scope_graph_body_issues(prefix: &str, issues: &mut [ValidationIssue]) {
    if prefix.is_empty() {
        return;
    }

    for issue in issues {
        issue.scope = Some(prefix.to_string());
        issue.message = format!("{prefix}: {}", issue.message);
    }
}

fn validate_run_as_config(run_as: Option<&RunAsConfig>, issues: &mut Vec<ValidationIssue>) {
    let Some(run_as) = run_as else {
        return;
    };

    if run_as.user.is_some() && run_as.command.is_some() {
        tracing::warn!("workflow runAs has both user and command set; command takes precedence");
    }

    if let Some(command) = run_as.command.as_ref() {
        if command.is_empty() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: None,
                scope: None,
                message: "runAs.command must not be empty.".to_string(),
            });
        } else if command.iter().any(|token| token.trim().is_empty()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: None,
                scope: None,
                message: "runAs.command must not contain blank tokens.".to_string(),
            });
        }
    }

    if let Some(user) = run_as.user.as_deref() {
        if user.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: None,
                scope: None,
                message: "runAs.user must not be empty.".to_string(),
            });
        } else if user.chars().any(is_run_as_user_shell_metachar) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: None,
                scope: None,
                message: "runAs.user contains shell metacharacters.".to_string(),
            });
        }
    }
}

fn is_run_as_user_shell_metachar(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ';' | '\''
                | '"'
                | '`'
                | '$'
                | '&'
                | '|'
                | '>'
                | '<'
                | '\\'
                | '!'
                | '*'
                | '?'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '#'
                | '~'
        )
}

fn validate_wait_timing_and_marker(
    node_id: &str,
    node_name: &str,
    until_marker: Option<&str>,
    require_marker: bool,
    idle_seconds: Option<f64>,
    ready_stable_seconds: Option<f64>,
    issues: &mut Vec<ValidationIssue>,
) {
    if require_marker && until_marker.is_none_or(|marker| marker.trim().is_empty()) {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node_id.to_string()),
            scope: None,
            message: format!(
                "\"{}\" wait node with until mode requires a marker.",
                node_name
            ),
        });
    }

    if let Some(marker) = until_marker.filter(|marker| !marker.trim().is_empty()) {
        if let Err(error) = regex_engine::Regex::new(marker) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node_id.to_string()),
                scope: None,
                message: format!("\"{}\" has invalid wait marker regex: {}", node_name, error),
            });
        }
    }

    for (field_name, value) in [
        ("idle_seconds", idle_seconds),
        ("ready_stable_seconds", ready_stable_seconds),
    ] {
        if let Some(value) = value
            && (!value.is_finite() || value < 0.0)
        {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node_id.to_string()),
                scope: None,
                message: format!(
                    "\"{}\" {} must be a finite non-negative number.",
                    node_name, field_name
                ),
            });
        }
    }
}

fn validate_subflow_catalog(workflow: &WorkflowV3, issues: &mut Vec<ValidationIssue>) {
    for (subflow_name, subflow) in &workflow.subflows {
        let subflow_graph = subflow.graph();
        if subflow_graph
            .node_map
            .get(subflow.entry_node_id.as_str())
            .is_none()
        {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: None,
                scope: None,
                message: format!(
                    "Subflow \"{}\" entryNodeId \"{}\" references a non-existent node.",
                    subflow_name, subflow.entry_node_id
                ),
            });
        }

        let prefix = format!("subflow:{subflow_name}");
        validate_graph_body(subflow, &workflow.subflows, &prefix, issues);
    }
}

fn validate_subflow_node_config(
    node: &WorkflowNode,
    config: &SubflowConfig,
    subflows: &BTreeMap<String, Box<WorkflowV3>>,
    issues: &mut Vec<ValidationIssue>,
) {
    let workflow_name = config.workflow_name.trim();
    if workflow_name.is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" {} node requires subflowConfig.workflowName.",
                node.name,
                node.node_type_str()
            ),
        });
        return;
    }

    let Some(subflow) = subflows
        .get(workflow_name)
        .map(|workflow| workflow.as_ref())
    else {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" references unknown subflow \"{}\".",
                node.name, workflow_name
            ),
        });
        return;
    };

    let subflow_graph = subflow.graph();
    if subflow_graph
        .node_map
        .get(subflow.entry_node_id.as_str())
        .is_none()
    {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" references subflow \"{}\" with missing entryNodeId \"{}\".",
                node.name, workflow_name, subflow.entry_node_id
            ),
        });
    }

    let terminal_node_ids = subflow
        .nodes
        .iter()
        .filter(|candidate| subflow_graph.outgoing_for(&candidate.id).is_empty())
        .map(|candidate| candidate.id.as_str())
        .collect::<Vec<_>>();
    if terminal_node_ids.len() != 1 {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" references subflow \"{}\" which must expose exactly one exit node; found {}.",
                node.name,
                workflow_name,
                terminal_node_ids.len()
            ),
        });
    }

    if let Some(exit_node_id) = config
        .exit_node_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        match subflow_graph.node_map.get(exit_node_id) {
            Some(_) if !subflow_graph.outgoing_for(exit_node_id).is_empty() => {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    scope: None,
                    message: format!(
                        "\"{}\" subflow exitNodeId \"{}\" must be terminal.",
                        node.name, exit_node_id
                    ),
                });
            }
            Some(_) => {}
            None => issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" subflow exitNodeId \"{}\" references a non-existent node.",
                    node.name, exit_node_id
                ),
            }),
        }
    } else if terminal_node_ids.len() != 1 {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" subflowConfig.exitNodeId is required unless the referenced subflow has exactly one terminal node.",
                node.name
            ),
        });
    }

    if config.max_depth == 0 {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!("\"{}\" subflow maxDepth must be at least 1.", node.name),
        });
    }

    let mut seen_inputs = BTreeSet::new();
    let subflow_variables = subflow
        .variables
        .iter()
        .map(|variable| variable.name.as_str())
        .collect::<BTreeSet<_>>();
    for input in &config.inputs {
        if input.name.trim().is_empty() || input.source.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" subflow node has an input binding with an empty name or source.",
                    node.name
                ),
            });
        }
        if !input.name.trim().is_empty() && !seen_inputs.insert(input.name.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" subflow node has duplicate input binding \"{}\".",
                    node.name, input.name
                ),
            });
        }
        if !input.name.trim().is_empty() && !subflow_variables.contains(input.name.as_str()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" binds unknown subflow variable \"{}\".",
                    node.name, input.name
                ),
            });
        }
    }

    for variable in &subflow.variables {
        if variable.default.is_empty() && !seen_inputs.contains(&variable.name) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" does not bind required subflow input \"{}\".",
                    node.name, variable.name
                ),
            });
        }
    }
}

fn validate_subflow_call_cycles(workflow: &WorkflowV3, issues: &mut Vec<ValidationIssue>) {
    let root_name = workflow
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("__root__")
        .to_string();
    let mut graph: BTreeMap<String, Vec<(String, String, u32)>> = BTreeMap::new();
    collect_subflow_calls(&root_name, workflow, &mut graph);
    for (name, subflow) in &workflow.subflows {
        collect_subflow_calls(name, subflow, &mut graph);
    }

    for component in subflow_strongly_connected_components(&graph) {
        let members = component.iter().cloned().collect::<BTreeSet<_>>();
        let self_loop = component.len() == 1
            && graph
                .get(&component[0])
                .is_some_and(|edges| edges.iter().any(|(callee, _, _)| callee == &component[0]));
        if component.len() == 1 && !self_loop {
            continue;
        }

        let (node_id, max_depth) = component
            .iter()
            .find_map(|caller| {
                graph.get(caller).and_then(|edges| {
                    edges
                        .iter()
                        .find(|(callee, _, _)| members.contains(callee))
                        .map(|(_, node_id, max_depth)| (node_id, max_depth))
                })
            })
            .expect("a cyclic component must contain an internal call edge");
        issues.push(ValidationIssue {
            severity: "warning".to_string(),
            node_id: Some(node_id.clone()),
            scope: None,
            message: format!(
                "Subflow call cycle detected among subflows {{{}}}. maxDepth ({}) bounds recursion at runtime.",
                component.join(", "),
                max_depth
            ),
        });
    }
}

fn collect_subflow_calls(
    workflow_name: &str,
    workflow: &WorkflowV3,
    graph: &mut BTreeMap<String, Vec<(String, String, u32)>>,
) {
    graph.entry(workflow_name.to_string()).or_default();
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
        let callee = config.workflow_name.trim();
        if callee.is_empty() {
            continue;
        }
        graph.entry(workflow_name.to_string()).or_default().push((
            callee.to_string(),
            node.id.clone(),
            config.max_depth,
        ));
    }
}

fn subflow_strongly_connected_components(
    graph: &BTreeMap<String, Vec<(String, String, u32)>>,
) -> Vec<Vec<String>> {
    // Tarjan's SCC algorithm with explicit DFS frames avoids native recursion.
    struct DfsFrame {
        vertex: String,
        next_edge: usize,
        parent: Option<String>,
    }

    let mut vertices = BTreeSet::new();
    for (caller, edges) in graph {
        vertices.insert(caller.clone());
        vertices.extend(edges.iter().map(|(callee, _, _)| callee.clone()));
    }

    let mut next_index = 0_usize;
    let mut indices = BTreeMap::new();
    let mut lowlinks = BTreeMap::new();
    let mut component_stack = Vec::new();
    let mut on_component_stack = BTreeSet::new();
    let mut components = Vec::new();

    for start in vertices {
        if indices.contains_key(&start) {
            continue;
        }

        indices.insert(start.clone(), next_index);
        lowlinks.insert(start.clone(), next_index);
        next_index += 1;
        component_stack.push(start.clone());
        on_component_stack.insert(start.clone());

        let mut dfs_stack = vec![DfsFrame {
            vertex: start,
            next_edge: 0,
            parent: None,
        }];
        while !dfs_stack.is_empty() {
            let next = {
                let frame = dfs_stack.last_mut().expect("stack is not empty");
                let next = graph
                    .get(&frame.vertex)
                    .and_then(|edges| edges.get(frame.next_edge))
                    .map(|(callee, _, _)| callee.clone());
                frame.next_edge += usize::from(next.is_some());
                next
            };

            if let Some(callee) = next {
                if !indices.contains_key(&callee) {
                    let parent = dfs_stack.last().expect("stack is not empty").vertex.clone();
                    indices.insert(callee.clone(), next_index);
                    lowlinks.insert(callee.clone(), next_index);
                    next_index += 1;
                    component_stack.push(callee.clone());
                    on_component_stack.insert(callee.clone());
                    dfs_stack.push(DfsFrame {
                        vertex: callee,
                        next_edge: 0,
                        parent: Some(parent),
                    });
                } else if on_component_stack.contains(&callee) {
                    let current = &dfs_stack.last().expect("stack is not empty").vertex;
                    let callee_index = indices[&callee];
                    let current_lowlink = lowlinks
                        .get_mut(current)
                        .expect("active vertex has a lowlink");
                    *current_lowlink = (*current_lowlink).min(callee_index);
                }
            } else {
                let finished = dfs_stack.pop().expect("stack is not empty");
                if let Some(parent) = finished.parent {
                    let finished_lowlink = lowlinks[&finished.vertex];
                    let parent_lowlink = lowlinks
                        .get_mut(&parent)
                        .expect("parent vertex has a lowlink");
                    *parent_lowlink = (*parent_lowlink).min(finished_lowlink);
                }

                if lowlinks[&finished.vertex] == indices[&finished.vertex] {
                    let mut component = Vec::new();
                    loop {
                        let member = component_stack
                            .pop()
                            .expect("component root is on the stack");
                        on_component_stack.remove(&member);
                        let is_root = member == finished.vertex;
                        component.push(member);
                        if is_root {
                            break;
                        }
                    }
                    component.sort();
                    components.push(component);
                }
            }
        }
    }

    components
}

fn validate_decide_node_config(
    node: &WorkflowNode,
    config: &DecideConfig,
    outgoing: &[&WorkflowEdge],
    issues: &mut Vec<ValidationIssue>,
) {
    if config.prompt.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: "warning".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!("\"{}\" decide node has an empty prompt.", node.name),
        });
    }

    let mut seen_inputs = BTreeSet::new();
    for input in &config.inputs {
        if input.name.trim().is_empty() || input.source.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" decide node has an input binding with an empty name or source.",
                    node.name
                ),
            });
        }
        if !input.name.trim().is_empty() && !seen_inputs.insert(input.name.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" decide node has duplicate input binding \"{}\".",
                    node.name, input.name
                ),
            });
        }
    }

    if config.outcomes.is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" decide node requires at least one outcome.",
                node.name
            ),
        });
        return;
    }

    let branch_labels = outgoing
        .iter()
        .filter(|edge| edge.outcome == WorkflowEdgeOutcome::Branch)
        .filter_map(|edge| edge.label.as_deref())
        .collect::<BTreeSet<_>>();
    let mut seen_outcomes = BTreeSet::new();
    for outcome in &config.outcomes {
        if outcome.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!("\"{}\" decide node has an empty outcome label.", node.name),
            });
            continue;
        }
        if !seen_outcomes.insert(outcome.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" decide node has duplicate outcome \"{}\".",
                    node.name, outcome
                ),
            });
        }
        if !branch_labels.contains(outcome.as_str()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" decide outcome \"{}\" does not match an outgoing branch edge label.",
                    node.name, outcome
                ),
            });
        }
    }

    let branch_edges = outgoing
        .iter()
        .filter(|edge| edge.outcome == WorkflowEdgeOutcome::Branch)
        .collect::<Vec<_>>();
    if branch_edges.len() != config.outcomes.len() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" decide node must have one outgoing branch edge per outcome.",
                node.name
            ),
        });
    }
    for edge in branch_edges {
        if edge
            .label
            .as_deref()
            .is_none_or(|label| label.trim().is_empty())
        {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                scope: None,
                message: format!(
                    "\"{}\" decide node branch edge \"{}\" requires a label matching an outcome.",
                    node.name, edge.id
                ),
            });
        }
    }
}

fn validate_batch_node_config(
    node: &WorkflowNode,
    config: &BatchConfig,
    graph: &WorkflowGraph<'_>,
    issues: &mut Vec<ValidationIssue>,
) {
    if config.items_binding.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" parallel_batch node requires itemsBinding.",
                node.name
            ),
        });
    }
    if config.item_var.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!("\"{}\" parallel_batch node requires itemVar.", node.name),
        });
    }
    if config.body_entry.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!("\"{}\" parallel_batch node requires bodyEntry.", node.name),
        });
    } else if graph.node_map.get(config.body_entry.as_str()).is_none() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" parallel_batch bodyEntry \"{}\" references a non-existent node.",
                node.name, config.body_entry
            ),
        });
    }
    if config.max_concurrent == 0 {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" parallel_batch maxConcurrent must be at least 1.",
                node.name
            ),
        });
    }
    if config
        .collector_var
        .as_ref()
        .is_some_and(|collector_var| collector_var.trim().is_empty())
    {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            scope: None,
            message: format!(
                "\"{}\" parallel_batch collectorVar cannot be empty when set.",
                node.name
            ),
        });
    }
}

fn has_task_execution_config(node: &WorkflowNode) -> bool {
    node.agent.is_some()
        || !node.prompt.trim().is_empty()
        || !node.context_sources.is_empty()
        || node.response_format.is_some()
        || node.output_schema.is_some()
        || node.retry_count.is_some()
        || node.retry_delay.is_some()
        || node.timeout.is_some()
        || node.skip_condition.is_some()
        || node.loop_max_iterations.is_some()
        || node.loop_condition.is_some()
        || node.kind.agent_config().is_some()
        || node.cwd.is_some()
}

pub fn compute_graph_metadata(workflow: &WorkflowV3) -> GraphMetadata {
    let graph = workflow.graph();
    let mut reachable = BTreeSet::new();
    let mut queue = VecDeque::from([workflow.entry_node_id.clone()]);
    while let Some(node_id) = queue.pop_front() {
        if !reachable.insert(node_id.clone()) {
            continue;
        }
        if let Some(node) = graph.node_map.get(node_id.as_str()) {
            if let NodeKind::ParallelBatch { batch_config } = &node.kind {
                let body_entry = batch_config.body_entry.clone();
                if !body_entry.trim().is_empty() {
                    queue.push_back(body_entry);
                }
            }
        }
        for edge in graph.outgoing_for(&node_id) {
            queue.push_back(edge.to.clone());
        }
    }

    let mut dead_ends = Vec::new();
    for node in &workflow.nodes {
        if graph.outgoing_for(&node.id).is_empty() {
            dead_ends.push(node.id.clone());
        }
    }

    let unreachable = workflow
        .nodes
        .iter()
        .filter(|node| !reachable.contains(&node.id))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();

    GraphMetadata {
        reachable_node_ids: reachable.into_iter().collect(),
        unreachable_node_ids: unreachable,
        dead_end_node_ids: dead_ends,
    }
}

pub fn get_nested_field<'a>(value: &'a Value, dot_path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in dot_path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

pub fn evaluate_condition(
    parsed_output: &Value,
    condition: &StructuredCondition,
) -> (bool, Option<String>) {
    let Some(field_value) = get_nested_field(parsed_output, &condition.field) else {
        return (false, Some(format!("field not found: {}", condition.field)));
    };

    let value_as_string = if let Some(string) = field_value.as_str() {
        string.to_string()
    } else {
        field_value.to_string()
    };
    let target = condition.value.as_str();

    match condition.operator.as_str() {
        "==" => (value_as_string == target, None),
        "!=" => (value_as_string != target, None),
        "contains" => (value_as_string.contains(target), None),
        "matches" => {
            if target.len() > 256 {
                return (
                    false,
                    Some("regex pattern too long (max 256 chars)".to_string()),
                );
            }
            match regex_is_match(target, &value_as_string) {
                Ok(matched) => (matched, None),
                Err(error) => (false, Some(error)),
            }
        }
        ">" | "<" | ">=" | "<=" => {
            let lhs = value_as_string.parse::<f64>();
            let rhs = target.parse::<f64>();
            let (Ok(lhs), Ok(rhs)) = (lhs, rhs) else {
                return (
                    false,
                    Some(format!(
                        "non-numeric comparison: {} {} {}",
                        value_as_string, condition.operator, target
                    )),
                );
            };
            let matched = match condition.operator.as_str() {
                ">" => lhs > rhs,
                "<" => lhs < rhs,
                ">=" => lhs >= rhs,
                "<=" => lhs <= rhs,
                _ => false,
            };
            (matched, None)
        }
        other => (false, Some(format!("unknown operator: {}", other))),
    }
}

fn regex_is_match(pattern: &str, input: &str) -> Result<bool, String> {
    regex_engine::Regex::new(pattern)
        .map_err(|error| format!("invalid regex: {}", error))
        .map(|regex| regex.is_match(input))
}

mod regex_engine {
    pub use regex::Regex;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn node(id: &str, name: &str, node_type: WorkflowNodeType) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: name.to_string(),
            kind: node_type.into(),
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

    fn workflow(
        nodes: Vec<WorkflowNode>,
        edges: Vec<WorkflowEdge>,
        entry_node_id: &str,
    ) -> WorkflowV3 {
        WorkflowV3 {
            version: WORKFLOW_SCHEMA_VERSION,
            name: Some("test".to_string()),
            goal: "goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: entry_node_id.to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits::default(),
            nodes,
            edges,
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        }
    }

    #[test]
    fn rejects_non_v3_workflow() {
        let error = normalize_workflow_value(json!({
            "steps": [],
            "_version": 2
        }))
        .unwrap_err();

        assert!(error.to_string().contains("workflow version is required"));
    }

    #[test]
    fn validates_missing_entry_node() {
        let result = validate_workflow(WorkflowV3 {
            version: WORKFLOW_SCHEMA_VERSION,
            name: None,
            goal: String::new(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: None,
            entry_node_id: "missing".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        });
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("entryNodeId"))
        );
    }

    #[test]
    fn validates_run_as_command_is_not_empty() {
        let mut workflow = workflow(Vec::new(), Vec::new(), "missing");
        workflow.run_as = Some(RunAsConfig {
            command: Some(Vec::new()),
            ..Default::default()
        });

        let result = validate_workflow(workflow);
        assert!(result.issues.iter().any(|issue| issue.severity == "error"
            && issue.message.contains("runAs.command must not be empty")));
    }

    #[test]
    fn validates_run_as_user_shell_safety() {
        let mut workflow = workflow(Vec::new(), Vec::new(), "missing");
        workflow.run_as = Some(RunAsConfig {
            user: Some("bad user".to_string()),
            ..Default::default()
        });

        let result = validate_workflow(workflow);
        assert!(result.issues.iter().any(|issue| {
            issue.severity == "error"
                && issue
                    .message
                    .contains("runAs.user contains shell metacharacters")
        }));
    }

    #[test]
    fn validates_run_as_rejects_blank_command_tokens_and_empty_user() {
        let blank_token = {
            let mut workflow = workflow(Vec::new(), Vec::new(), "missing");
            workflow.run_as = Some(RunAsConfig {
                command: Some(vec!["".to_string()]),
                ..Default::default()
            });
            validate_workflow(workflow)
        };
        assert!(
            blank_token.issues.iter().any(|issue| {
                issue.severity == "error" && issue.message.contains("blank tokens")
            })
        );

        let whitespace_token = {
            let mut workflow = workflow(Vec::new(), Vec::new(), "missing");
            workflow.run_as = Some(RunAsConfig {
                command: Some(vec!["tmux".to_string(), "   ".to_string()]),
                ..Default::default()
            });
            validate_workflow(workflow)
        };
        assert!(
            whitespace_token.issues.iter().any(|issue| {
                issue.severity == "error" && issue.message.contains("blank tokens")
            })
        );

        let empty_user = {
            let mut workflow = workflow(Vec::new(), Vec::new(), "missing");
            workflow.run_as = Some(RunAsConfig {
                user: Some("".to_string()),
                ..Default::default()
            });
            validate_workflow(workflow)
        };
        assert!(empty_user.issues.iter().any(|issue| {
            issue.severity == "error" && issue.message.contains("runAs.user must not be empty")
        }));
    }

    #[test]
    fn evaluates_conditions() {
        let (matched, error) = evaluate_condition(
            &json!({ "score": 12, "status": "ok" }),
            &StructuredCondition {
                field: "score".to_string(),
                operator: ">=".to_string(),
                value: "10".to_string(),
            },
        );
        assert!(matched);
        assert!(error.is_none());
    }

    #[test]
    fn validates_split_edge_rules_and_fanout_warning() {
        let result = validate_workflow(workflow(
            vec![
                node("split", "Split", WorkflowNodeType::Split),
                node("task_a", "Task A", WorkflowNodeType::Task),
                node("task_b", "Task B", WorkflowNodeType::Task),
            ],
            vec![
                success_edge("edge_success", "split", "task_a", None),
                WorkflowEdge {
                    id: "edge_branch".to_string(),
                    from: "split".to_string(),
                    to: "task_b".to_string(),
                    outcome: WorkflowEdgeOutcome::Branch,
                    label: Some("branch".to_string()),
                    branch_id: Some("branch".to_string()),
                    condition: None,
                },
            ],
            "split",
        ));

        assert!(result.issues.iter().any(|issue| {
            issue
                .message
                .contains("can only use success edges for split fan-out")
        }));
        assert!(result.issues.iter().any(|issue| {
            issue
                .message
                .contains("fans out to fewer than two branches")
        }));
    }

    #[test]
    fn validates_collector_duplicate_merge_keys_and_outbound_rules() {
        let result = validate_workflow(workflow(
            vec![
                node("task_a", "Task A", WorkflowNodeType::Task),
                node("task_b", "Task B", WorkflowNodeType::Task),
                node("collector", "Collector", WorkflowNodeType::Collector),
                node("task_c", "Task C", WorkflowNodeType::Task),
            ],
            vec![
                success_edge("edge_a", "task_a", "collector", Some("dup")),
                success_edge("edge_b", "task_b", "collector", Some("dup")),
                WorkflowEdge {
                    id: "edge_branch".to_string(),
                    from: "collector".to_string(),
                    to: "task_c".to_string(),
                    outcome: WorkflowEdgeOutcome::Branch,
                    label: Some("branch".to_string()),
                    branch_id: Some("branch".to_string()),
                    condition: None,
                },
            ],
            "task_a",
        ));

        assert!(result.issues.iter().any(|issue| {
            issue
                .message
                .contains("duplicate collector input key \"dup\"")
        }));
        assert!(result.issues.iter().any(|issue| {
            issue
                .message
                .contains("must have exactly one outbound success edge")
        }));
        assert!(result.issues.iter().any(|issue| {
            issue
                .message
                .contains("can only use a success edge after collecting inputs")
        }));
    }

    #[test]
    fn warns_when_non_task_nodes_keep_task_execution_fields() {
        let mut split = node("split", "Split", WorkflowNodeType::Split);
        split.agent = Some("claude".to_string());
        split.prompt = "ignored".to_string();

        let mut collector = node("collector", "Collector", WorkflowNodeType::Collector);
        collector.timeout = Some(30);

        let result = validate_workflow(workflow(
            vec![
                split,
                collector,
                node("task", "Task", WorkflowNodeType::Task),
            ],
            vec![
                success_edge("edge_a", "split", "collector", Some("alpha")),
                success_edge("edge_b", "collector", "task", None),
            ],
            "split",
        ));

        assert!(result.issues.iter().any(|issue| {
            issue
                .message
                .contains("split node, so task execution fields are ignored")
        }));
        assert!(result.issues.iter().any(|issue| {
            issue
                .message
                .contains("collector node, so task execution fields are ignored")
        }));
    }

    #[test]
    fn resolve_agent_config_uses_defaults_when_no_overrides() {
        let mut defaults = BTreeMap::new();
        defaults.insert(
            "claude".to_string(),
            AgentDefaults {
                model: Some("opus".to_string()),
                max_turns: Some(5),
                ..Default::default()
            },
        );
        let n = node("n1", "N1", WorkflowNodeType::Task);
        let config = resolve_agent_config(&defaults, "/work", "claude", &n, None, false, None);
        assert_eq!(config.model.as_deref(), Some("opus"));
        assert_eq!(config.max_turns, Some(5));
        assert_eq!(config.cwd, "/work");
        assert!(config.ephemeral_session);
    }

    #[test]
    fn resolve_agent_config_node_overrides_defaults() {
        let mut defaults = BTreeMap::new();
        defaults.insert(
            "claude".to_string(),
            AgentDefaults {
                model: Some("opus".to_string()),
                reasoning_level: Some(ReasoningLevel::Low),
                ..Default::default()
            },
        );
        let mut n = node("n1", "N1", WorkflowNodeType::Task);
        n.kind.set_agent_config(Some(AgentNodeConfig {
            base: AgentDefaults {
                model: Some("sonnet".to_string()),
                ..Default::default()
            },
            ..Default::default()
        }));
        n.cwd = Some("/custom".to_string());
        let config = resolve_agent_config(&defaults, "/work", "claude", &n, None, false, None);
        // Node override wins for model
        assert_eq!(config.model.as_deref(), Some("sonnet"));
        // Default still used for reasoning_level (no node override)
        assert_eq!(config.reasoning_level, Some(ReasoningLevel::Low));
        // Node cwd overrides workflow cwd
        assert_eq!(config.cwd, "/custom");
    }

    #[test]
    fn resolve_agent_config_session_persistence() {
        let defaults = BTreeMap::new();
        let n = node("n1", "N1", WorkflowNodeType::Task);
        let config = resolve_agent_config(
            &defaults,
            "/work",
            "claude",
            &n,
            Some("session-123".to_string()),
            true,
            None,
        );
        assert_eq!(config.resume_session_id.as_deref(), Some("session-123"));
        assert!(!config.ephemeral_session);
    }

    #[test]
    fn existing_workflow_json_loads_without_new_fields() {
        // Verify that a v3 workflow without any new fields deserializes fine
        let json = json!({
            "version": 4,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "n1",
                "name": "Step 1",
                "agent": "claude",
                "prompt": "hello",
                "kind": { "type": "task" }
            }],
            "edges": []
        });
        let result = normalize_workflow_value(json);
        assert!(result.is_ok());
        let w = result.unwrap().workflow;
        assert!(w.agent_defaults.is_empty());
        assert!(w.nodes[0].kind.agent_config().is_none());
        assert!(w.nodes[0].cwd.is_none());
    }

    #[test]
    fn v2_flat_node_shape_migrates_to_v3_nested_kind() {
        let json = json!({
            "version": 2,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "decide",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "decide",
                "name": "Pick",
                "type": "decide",
                "decideConfig": {
                    "prompt": "Pick one",
                    "outcomes": ["yes"]
                }
            }],
            "edges": []
        });

        let normalized = normalize_workflow_value(json).unwrap();

        assert_eq!(normalized.workflow.version, 4);
        assert!(normalized.notices.iter().any(|notice| {
            notice.contains("Migrated workflow schema from version 2 to version 4")
        }));
        assert!(matches!(
            &normalized.workflow.nodes[0].kind,
            NodeKind::Decide { decide_config }
                if decide_config.outcomes == vec!["yes".to_string()]
        ));
    }

    #[test]
    fn v3_flat_node_shape_migrates_structurally_to_v4() {
        let json = json!({
            "version": 3,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "decide",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "decide",
                "name": "Pick",
                "type": "decide",
                "decideConfig": {
                    "prompt": "Pick one",
                    "outcomes": ["yes"]
                }
            }],
            "edges": []
        });

        let normalized = normalize_workflow_value(json).unwrap();

        assert_eq!(normalized.workflow.version, 4);
        assert!(matches!(
            &normalized.workflow.nodes[0].kind,
            NodeKind::Decide { decide_config }
                if decide_config.outcomes == vec!["yes".to_string()]
        ));
    }

    #[test]
    fn send_node_without_config_defaults_to_enter() {
        let node: WorkflowNode = serde_json::from_value(json!({
            "id": "send",
            "name": "Send",
            "kind": { "type": "send" }
        }))
        .unwrap();

        assert!(matches!(
            node.kind,
            NodeKind::Send { send_config } if send_config.enter
        ));
    }

    #[test]
    fn run_agent_node_without_config_defaults_to_kill_after() {
        let node: WorkflowNode = serde_json::from_value(json!({
            "id": "run-agent",
            "name": "Run agent",
            "kind": { "type": "run_agent" }
        }))
        .unwrap();

        assert!(matches!(
            node.kind,
            NodeKind::RunAgent {
                run_agent_config,
                ..
            } if run_agent_config.kill_after
        ));
    }

    #[test]
    fn capture_node_without_config_defaults() {
        let node: WorkflowNode = serde_json::from_value(json!({
            "id": "capture",
            "name": "Capture",
            "kind": { "type": "capture" }
        }))
        .unwrap();

        assert!(matches!(
            node.kind,
            NodeKind::Capture { capture_config } if capture_config == CaptureConfig::default()
        ));
    }

    #[test]
    fn kill_node_without_config_defaults() {
        let node: WorkflowNode = serde_json::from_value(json!({
            "id": "kill",
            "name": "Kill",
            "kind": { "type": "kill" }
        }))
        .unwrap();

        assert!(matches!(
            node.kind,
            NodeKind::Kill { kill_config } if kill_config == KillConfig::default()
        ));
    }

    // -----------------------------------------------------------------------
    // T7: NodeKind round-trip and v2→v3 migration tests
    // -----------------------------------------------------------------------

    /// Confirm that every NodeKind variant serializes to the internally-tagged
    /// `{ "type": "<snake_case>", <variantConfig> }` form and deserializes back
    /// to the identical value.
    #[test]
    fn node_kind_round_trip_all_variants() {
        let variants: Vec<NodeKind> = vec![
            NodeKind::Task { agent_config: None },
            NodeKind::Approval,
            NodeKind::Split,
            NodeKind::Collector,
            NodeKind::Decide {
                decide_config: DecideConfig {
                    prompt: "Pick one".to_string(),
                    outcomes: vec!["yes".to_string(), "no".to_string()],
                    model: Some("claude-haiku-4-5".to_string()),
                    inputs: vec![InputBinding {
                        name: "ctx".to_string(),
                        source: "n0".to_string(),
                    }],
                },
            },
            NodeKind::ParallelBatch {
                batch_config: BatchConfig {
                    items_binding: "items".to_string(),
                    max_concurrent: 4,
                    item_var: "item".to_string(),
                    body_entry: "body".to_string(),
                    collector_var: Some("results".to_string()),
                },
            },
            NodeKind::Subflow {
                subflow_config: SubflowConfig {
                    workflow_name: "child".to_string(),
                    exit_node_id: Some("exit".to_string()),
                    inputs: vec![InputBinding {
                        name: "x".to_string(),
                        source: "src".to_string(),
                    }],
                    max_depth: 5,
                },
            },
            NodeKind::Call {
                subflow_config: SubflowConfig {
                    workflow_name: "sub".to_string(),
                    exit_node_id: None,
                    inputs: Vec::new(),
                    max_depth: default_max_call_depth(),
                },
            },
            NodeKind::Spawn {
                spawn_config: SpawnConfig {
                    agent: Some("claude".to_string()),
                    command: None,
                    access: None,
                    extra_args: Vec::new(),
                    cwd: Some("/tmp".to_string()),
                    name: None,
                    session_name: Some("my-session".to_string()),
                },
            },
            NodeKind::Send {
                send_config: SendConfig {
                    target: Some("session-1".to_string()),
                    text: "hello".to_string(),
                    enter: true,
                },
            },
            NodeKind::Wait {
                wait_config: WaitConfig {
                    target: Some("session-1".to_string()),
                    mode: WaitMode::Until,
                    marker: Some("DONE".to_string()),
                    timeout: Some(30),
                    idle_seconds: None,
                    ready_stable_seconds: None,
                },
            },
            NodeKind::Capture {
                capture_config: CaptureConfig {
                    target: Some("session-1".to_string()),
                    lines: Some(50),
                    all: false,
                    ansi: true,
                },
            },
            NodeKind::Kill {
                kill_config: KillConfig {
                    target: Some("session-1".to_string()),
                    session_name: None,
                },
            },
            NodeKind::RunAgent {
                run_agent_config: RunAgentConfig {
                    agent: Some("claude".to_string()),
                    prompt: Some("Do a thing".to_string()),
                    cwd: None,
                    access: None,
                    extra_args: Vec::new(),
                    name: None,
                    timeout: Some(60),
                    idle_seconds: Some(5.0),
                    ready_stable_seconds: None,
                    until: Some("DONE".to_string()),
                    kill_after: true,
                },
                agent_config: None,
            },
        ];

        for kind in &variants {
            let serialized = serde_json::to_value(kind)
                .unwrap_or_else(|e| panic!("failed to serialize {:?}: {}", kind.as_str(), e));

            // The internally-tagged form must have a "type" field at the top level.
            assert!(
                serialized.get("type").is_some(),
                "serialized NodeKind::{} missing top-level \"type\" field: {}",
                kind.as_str(),
                serialized
            );
            assert_eq!(
                serialized["type"].as_str().unwrap(),
                kind.as_str(),
                "NodeKind::{} \"type\" field mismatch",
                kind.as_str()
            );

            let deserialized: NodeKind =
                serde_json::from_value(serialized.clone()).unwrap_or_else(|e| {
                    panic!(
                        "failed to deserialize NodeKind::{} from {}: {}",
                        kind.as_str(),
                        serialized,
                        e
                    )
                });

            assert_eq!(
                kind,
                &deserialized,
                "round-trip mismatch for NodeKind::{}",
                kind.as_str()
            );
        }
    }

    /// Confirm a `WorkflowNode` with a nested `kind` serializes and deserializes
    /// correctly, with config fields living inside `kind` and common fields at
    /// the node level.
    #[test]
    fn workflow_node_round_trip_task_and_decide() {
        // Task node: kind.type = "task", agent/prompt at node level
        let task_node = WorkflowNode {
            id: "t1".to_string(),
            name: "My Task".to_string(),
            kind: NodeKind::Task {
                agent_config: Some(AgentNodeConfig {
                    base: AgentDefaults {
                        model: Some("claude-sonnet".to_string()),
                        max_turns: Some(10),
                        ..Default::default()
                    },
                    allowed_tools: Some(vec!["Bash".to_string()]),
                    disallowed_tools: None,
                }),
            },
            agent: Some("claude".to_string()),
            prompt: "Do something".to_string(),
            context_sources: Vec::new(),
            response_format: Some(ResponseFormat::Json),
            output_schema: None,
            retry_count: Some(2),
            retry_delay: None,
            timeout: Some(120),
            skip_condition: None,
            loop_max_iterations: None,
            loop_condition: None,
            split_failure_policy: SplitFailurePolicy::BestEffortContinue,
            cwd: Some("/work".to_string()),
            continue_session_from: None,
        };

        let serialized = serde_json::to_value(&task_node).expect("task node serialization failed");

        // Config fields must live inside `kind`, NOT at the top level.
        assert!(
            serialized.get("kind").is_some(),
            "serialized WorkflowNode missing \"kind\" field"
        );
        assert_eq!(serialized["kind"]["type"], "task");
        assert!(
            serialized["kind"].get("agentConfig").is_some(),
            "agentConfig should be inside kind"
        );
        // Common fields remain at node level.
        assert_eq!(serialized["agent"], "claude");
        assert_eq!(serialized["prompt"], "Do something");
        assert_eq!(serialized["responseFormat"], "json");

        let deserialized: WorkflowNode =
            serde_json::from_value(serialized).expect("task node deserialization failed");
        assert_eq!(task_node, deserialized);

        // Decide node: kind.type = "decide", decideConfig inside kind
        let decide_node = WorkflowNode {
            id: "d1".to_string(),
            name: "Route".to_string(),
            kind: NodeKind::Decide {
                decide_config: DecideConfig {
                    prompt: "Go left or right?".to_string(),
                    outcomes: vec!["left".to_string(), "right".to_string()],
                    model: None,
                    inputs: Vec::new(),
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
        };

        let serialized =
            serde_json::to_value(&decide_node).expect("decide node serialization failed");
        assert_eq!(serialized["kind"]["type"], "decide");
        assert!(
            serialized["kind"].get("decideConfig").is_some(),
            "decideConfig should be inside kind, got: {}",
            serialized["kind"]
        );
        // decideConfig must NOT be at the top level.
        assert!(
            serialized.get("decideConfig").is_none(),
            "decideConfig leaked to node top level"
        );

        let deserialized: WorkflowNode =
            serde_json::from_value(serialized).expect("decide node deserialization failed");
        assert_eq!(decide_node, deserialized);
    }

    /// Confirm a complete v2-style workflow (flat `type`+`decideConfig` shape at node
    /// level) loads via the migration path and produces a valid v3 `WorkflowV3` with
    /// the nested `kind` shape.  All node types that carried per-kind config in v2 are
    /// exercised here.
    #[test]
    fn v2_to_v3_migration_all_config_carrying_node_types() {
        let json = json!({
            "version": 2,
            "goal": "migration test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "task1",
            "variables": [],
            "limits": { "maxTotalSteps": 50, "maxVisitsPerNode": 10 },
            "nodes": [
                // Task with agentConfig at top level (v2 flat form)
                {
                    "id": "task1",
                    "name": "Task",
                    "type": "task",
                    "agent": "claude",
                    "prompt": "Do a task",
                    "agentConfig": { "model": "claude-opus" }
                },
                // Decide
                {
                    "id": "decide1",
                    "name": "Decide",
                    "type": "decide",
                    "decideConfig": {
                        "prompt": "Which way?",
                        "outcomes": ["a", "b"]
                    }
                },
                // Subflow
                {
                    "id": "sub1",
                    "name": "Subflow",
                    "type": "subflow",
                    "subflowConfig": { "workflowName": "child" }
                },
                // Spawn
                {
                    "id": "spawn1",
                    "name": "Spawn",
                    "type": "spawn",
                    "spawnConfig": { "agent": "claude", "sessionName": "sess" }
                },
                // Capture
                {
                    "id": "cap1",
                    "name": "Capture",
                    "type": "capture",
                    "captureConfig": { "all": true }
                },
                // Kill
                {
                    "id": "kill1",
                    "name": "Kill",
                    "type": "kill",
                    "killConfig": { "target": "sess" }
                },
                // RunAgent
                {
                    "id": "run1",
                    "name": "RunAgent",
                    "type": "run_agent",
                    "agent": "echo",
                    "prompt": "run",
                    "runAgentConfig": { "agent": "echo" }
                },
                // Nodes without extra config fields (v2 simple types)
                { "id": "approval1", "name": "Approval", "type": "approval" },
                { "id": "split1",    "name": "Split",    "type": "split"    },
                { "id": "collector1","name": "Collector","type": "collector" },
                { "id": "wait1",     "name": "Wait",     "type": "wait",
                  "waitConfig": { "mode": "idle" } }
            ],
            "edges": []
        });

        let normalized = normalize_workflow_value(json).expect("migration must succeed");

        // Schema bumped to 4
        assert_eq!(
            normalized.workflow.version, 4,
            "migrated workflow must be version 4"
        );
        assert!(
            normalized
                .notices
                .iter()
                .any(|n| n.contains("Migrated workflow schema from version 2 to version 4")),
            "expected migration notice, got: {:?}",
            normalized.notices
        );

        let nodes_by_id: std::collections::HashMap<&str, &WorkflowNode> = normalized
            .workflow
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n))
            .collect();

        // task1: kind.type == "task", agentConfig is inside kind
        let task = nodes_by_id["task1"];
        assert!(
            matches!(
                &task.kind,
                NodeKind::Task {
                    agent_config: Some(_)
                }
            ),
            "task1.kind should be Task with agentConfig, got {:?}",
            task.kind.as_str()
        );

        // decide1: kind.type == "decide", decideConfig inside kind
        let decide = nodes_by_id["decide1"];
        assert!(
            matches!(
                &decide.kind,
                NodeKind::Decide { decide_config }
                    if decide_config.outcomes == vec!["a".to_string(), "b".to_string()]
            ),
            "decide1.kind should be Decide with outcomes [a,b]"
        );

        // sub1: kind.type == "subflow"
        let sub = nodes_by_id["sub1"];
        assert!(
            matches!(
                &sub.kind,
                NodeKind::Subflow { subflow_config }
                    if subflow_config.workflow_name == "child"
            ),
            "sub1.kind should be Subflow with workflowName=\"child\""
        );

        // spawn1: kind.type == "spawn", spawnConfig inside kind
        let spawn = nodes_by_id["spawn1"];
        assert!(
            matches!(
                &spawn.kind,
                NodeKind::Spawn { spawn_config }
                    if spawn_config.session_name.as_deref() == Some("sess")
            ),
            "spawn1.kind should be Spawn with sessionName=\"sess\""
        );

        // cap1: kind.type == "capture", captureConfig inside kind (presence is structural)
        let cap = nodes_by_id["cap1"];
        assert!(
            matches!(
                &cap.kind,
                NodeKind::Capture { capture_config }
                    if capture_config.all
            ),
            "cap1.kind should be Capture with all=true"
        );

        // kill1: kind.type == "kill", killConfig inside kind
        let kill = nodes_by_id["kill1"];
        assert!(
            matches!(
                &kill.kind,
                NodeKind::Kill { kill_config }
                    if kill_config.target.as_deref() == Some("sess")
            ),
            "kill1.kind should be Kill with target=\"sess\""
        );

        // run1: kind.type == "run_agent"
        let run = nodes_by_id["run1"];
        assert!(
            matches!(&run.kind, NodeKind::RunAgent { .. }),
            "run1.kind should be RunAgent"
        );

        // approval1, split1, collector1: no config fields
        assert!(matches!(&nodes_by_id["approval1"].kind, NodeKind::Approval));
        assert!(matches!(&nodes_by_id["split1"].kind, NodeKind::Split));
        assert!(matches!(
            &nodes_by_id["collector1"].kind,
            NodeKind::Collector
        ));

        // wait1: kind.type == "wait"
        assert!(matches!(&nodes_by_id["wait1"].kind, NodeKind::Wait { .. }));

        // No legacy `type` / flat config fields should appear on the re-serialized node.
        for node in &normalized.workflow.nodes {
            let serialized = serde_json::to_value(node).expect("node serialization must succeed");
            assert!(
                serialized.get("type").is_none(),
                "node \"{}\" has a top-level \"type\" field (v2 leak): {}",
                node.id,
                serialized
            );
            for legacy_field in &[
                "decideConfig",
                "batchConfig",
                "parallelBatchConfig",
                "subflowConfig",
                "spawnConfig",
                "sendConfig",
                "waitConfig",
                "captureConfig",
                "killConfig",
                "runAgentConfig",
            ] {
                assert!(
                    serialized.get(legacy_field).is_none(),
                    "node \"{}\" has top-level legacy field \"{}\" (v2 leak): {}",
                    node.id,
                    legacy_field,
                    serialized
                );
            }
        }
    }

    #[test]
    fn v2_to_v3_migration_capture_kill_without_config() {
        let cases = [
            (
                "capture-missing-config",
                json!({
                    "id": "cap1",
                    "name": "Capture",
                    "type": "capture"
                }),
                NodeKind::Capture {
                    capture_config: CaptureConfig::default(),
                },
            ),
            (
                "capture-null-config",
                json!({
                    "id": "cap2",
                    "name": "Capture",
                    "type": "capture",
                    "captureConfig": null
                }),
                NodeKind::Capture {
                    capture_config: CaptureConfig::default(),
                },
            ),
            (
                "capture-empty-config",
                json!({
                    "id": "cap3",
                    "name": "Capture",
                    "type": "capture",
                    "captureConfig": {}
                }),
                NodeKind::Capture {
                    capture_config: CaptureConfig::default(),
                },
            ),
            (
                "kill-missing-config",
                json!({
                    "id": "kill1",
                    "name": "Kill",
                    "type": "kill"
                }),
                NodeKind::Kill {
                    kill_config: KillConfig::default(),
                },
            ),
            (
                "kill-null-config",
                json!({
                    "id": "kill2",
                    "name": "Kill",
                    "type": "kill",
                    "killConfig": null
                }),
                NodeKind::Kill {
                    kill_config: KillConfig::default(),
                },
            ),
            (
                "kill-empty-config",
                json!({
                    "id": "kill3",
                    "name": "Kill",
                    "type": "kill",
                    "killConfig": {}
                }),
                NodeKind::Kill {
                    kill_config: KillConfig::default(),
                },
            ),
        ];

        for (case_name, node_json, expected_kind) in cases {
            let json = json!({
                "version": 2,
                "goal": "capture/kill migration test",
                "cwd": "/tmp",
                "useOrchestrator": false,
                "entryNodeId": node_json["id"],
                "variables": [],
                "limits": { "maxTotalSteps": 50, "maxVisitsPerNode": 10 },
                "nodes": [node_json],
                "edges": []
            });

            let normalized = normalize_workflow_value(json)
                .unwrap_or_else(|error| panic!("{case_name} migration must succeed: {error}"));

            assert_eq!(
                normalized.workflow.version, 4,
                "{case_name}: migrated workflow must be version 4"
            );

            let node = normalized
                .workflow
                .nodes
                .first()
                .expect("{case_name}: workflow must contain one node");
            assert_eq!(
                node.kind, expected_kind,
                "{case_name}: migrated node kind mismatch"
            );

            let serialized =
                serde_json::to_value(node).expect("{case_name}: node serialization must succeed");
            assert!(
                serialized.get("type").is_none(),
                "{case_name}: migrated node must not leak top-level type"
            );
            assert!(
                serialized.get("captureConfig").is_none() && serialized.get("killConfig").is_none(),
                "{case_name}: migrated node must not leak top-level config fields"
            );

            let deserialized: WorkflowNode =
                serde_json::from_value(serialized).unwrap_or_else(|error| {
                    panic!("{case_name}: migrated node must deserialize as v3: {error}")
                });
            assert_eq!(
                deserialized.kind, expected_kind,
                "{case_name}: round-trip kind mismatch"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Stage 5: Output schema migration tests
    // -----------------------------------------------------------------------

    #[test]
    fn migrate_output_schema_converts_legacy_format() {
        let legacy = json!({"name": "string", "age": "integer"});
        let migrated = super::migrate_output_schema(legacy);
        assert_eq!(migrated["type"], "object");
        assert_eq!(migrated["properties"]["name"]["type"], "string");
        assert_eq!(migrated["properties"]["age"]["type"], "integer");
        let required = migrated["required"].as_array().unwrap();
        assert!(required.contains(&json!("name")));
        assert!(required.contains(&json!("age")));
    }

    #[test]
    fn migrate_output_schema_passes_through_json_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Name" }
            },
            "required": ["name"]
        });
        let result = super::migrate_output_schema(schema.clone());
        assert_eq!(result, schema);
    }

    #[test]
    fn migrate_output_schema_passes_through_empty_object() {
        let empty = json!({});
        let result = super::migrate_output_schema(empty.clone());
        assert_eq!(result, empty);
    }

    #[test]
    fn legacy_output_schema_deserialized_as_json_schema() {
        let json = json!({
            "version": 4,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "n1",
                "name": "Step 1",
                "agent": "claude",
                "prompt": "hello",
                "responseFormat": "json",
                "outputSchema": {"name": "string", "score": "number"},
                "kind": { "type": "task" }
            }],
            "edges": []
        });
        let result = normalize_workflow_value(json).unwrap();
        let schema = result.workflow.nodes[0].output_schema.as_ref().unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["score"]["type"], "number");
    }

    #[test]
    fn full_json_schema_deserialized_unchanged() {
        let full_schema = json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string", "description": "Brief" }
            },
            "required": ["summary"],
            "additionalProperties": false
        });
        let json = json!({
            "version": 4,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "n1",
                "name": "Step 1",
                "agent": "claude",
                "prompt": "hello",
                "responseFormat": "json",
                "outputSchema": full_schema,
                "kind": { "type": "task" }
            }],
            "edges": []
        });
        let result = normalize_workflow_value(json).unwrap();
        let schema = result.workflow.nodes[0].output_schema.as_ref().unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["summary"]["description"], "Brief");
        assert_eq!(schema["additionalProperties"], false);
    }

    // -----------------------------------------------------------------------
    // Stage 8: Session reuse validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn validates_continue_session_from_unknown_node() {
        let mut n1 = node("n1", "Step 1", WorkflowNodeType::Task);
        n1.agent = Some("claude".to_string());
        n1.continue_session_from = Some("nonexistent".to_string());

        let result = validate_workflow(workflow(vec![n1], vec![], "n1"));
        assert!(result.issues.iter().any(|i| {
            i.severity == "error"
                && i.message
                    .contains("references unknown node \"nonexistent\"")
        }));
    }

    #[test]
    fn validates_continue_session_from_different_agent() {
        let mut n1 = node("n1", "Step 1", WorkflowNodeType::Task);
        n1.agent = Some("claude".to_string());
        let mut n2 = node("n2", "Step 2", WorkflowNodeType::Task);
        n2.agent = Some("codex".to_string());
        n2.continue_session_from = Some("n1".to_string());

        let result = validate_workflow(workflow(
            vec![n1, n2],
            vec![success_edge("e1", "n1", "n2", None)],
            "n1",
        ));
        assert!(
            result
                .issues
                .iter()
                .any(|i| { i.severity == "error" && i.message.contains("different agents") })
        );
    }

    #[test]
    fn validates_continue_session_from_non_task_node() {
        let n1 = node("n1", "Split", WorkflowNodeType::Split);
        let mut n2 = node("n2", "Step 2", WorkflowNodeType::Task);
        n2.agent = Some("claude".to_string());
        n2.continue_session_from = Some("n1".to_string());

        let result = validate_workflow(workflow(
            vec![n1, n2],
            vec![success_edge("e1", "n1", "n2", None)],
            "n1",
        ));
        assert!(
            result.issues.iter().any(|i| {
                i.severity == "error" && i.message.contains("not an agent-running node")
            })
        );
    }

    #[test]
    fn validates_continue_session_from_valid() {
        let mut n1 = node("n1", "Step 1", WorkflowNodeType::Task);
        n1.agent = Some("claude".to_string());
        let mut n2 = node("n2", "Step 2", WorkflowNodeType::Task);
        n2.agent = Some("claude".to_string());
        n2.continue_session_from = Some("n1".to_string());

        let result = validate_workflow(workflow(
            vec![n1, n2],
            vec![success_edge("e1", "n1", "n2", None)],
            "n1",
        ));
        // No session-related errors
        assert!(
            !result
                .issues
                .iter()
                .any(|i| { i.severity == "error" && i.message.contains("session") })
        );
    }

    #[test]
    fn parses_minimal_run_agent_node() {
        let value = json!({
            "version": 4,
            "goal": "Run one agent",
            "entryNodeId": "run",
            "nodes": [{
                "id": "run",
                "name": "Run Echo",
                "agent": "echo",
                "prompt": "hello",
                "kind": { "type": "run_agent" }
            }],
            "edges": []
        });

        let normalized = normalize_workflow_value(value).unwrap();
        assert_eq!(
            normalized.workflow.nodes[0].node_type(),
            WorkflowNodeType::RunAgent
        );
        let result = validate_workflow(normalized.workflow);
        assert!(
            !result.issues.iter().any(|issue| issue.severity == "error"),
            "expected no validation errors, got {:?}",
            result.issues
        );
    }

    #[test]
    fn validates_subflow_call_required_inputs() {
        let mut call = node("call", "Call Double", WorkflowNodeType::Call);
        call.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "double".to_string(),
                exit_node_id: Some("exit".to_string()),
                inputs: Vec::new(),
                max_depth: default_max_call_depth(),
            },
        };
        let mut subflow = workflow(
            vec![
                node("entry", "Entry", WorkflowNodeType::Task),
                node("exit", "Exit", WorkflowNodeType::Task),
            ],
            vec![success_edge("entry_exit", "entry", "exit", None)],
            "entry",
        );
        subflow.variables = vec![WorkflowVariable {
            name: "input".to_string(),
            default: String::new(),
        }];
        let mut parent = workflow(vec![call], vec![], "call");
        parent
            .subflows
            .insert("double".to_string(), Box::new(subflow));

        let result = validate_workflow(parent);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "error"
                && issue
                    .message
                    .contains("does not bind required subflow input \"input\"")
        }));
    }

    #[test]
    fn validates_subflow_body_rules_with_scoped_issues() {
        let mut call = node("call", "Call Broken", WorkflowNodeType::Call);
        call.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "broken".to_string(),
                exit_node_id: Some("exit".to_string()),
                inputs: Vec::new(),
                max_depth: default_max_call_depth(),
            },
        };

        let mut decide = node("decide", "Route", WorkflowNodeType::Decide);
        decide.kind = NodeKind::Decide {
            decide_config: DecideConfig {
                inputs: Vec::new(),
                prompt: "Choose a route".to_string(),
                model: None,
                outcomes: vec!["yes".to_string(), "no".to_string()],
            },
        };
        let mut exit = node("exit", "Exit", WorkflowNodeType::Task);
        exit.agent = Some("claude".to_string());
        exit.prompt = "Finish".to_string();

        let broken_subflow = workflow(
            vec![decide, exit],
            vec![
                WorkflowEdge {
                    id: "branch_yes".to_string(),
                    from: "decide".to_string(),
                    to: "exit".to_string(),
                    outcome: WorkflowEdgeOutcome::Branch,
                    label: Some("yes".to_string()),
                    branch_id: None,
                    condition: None,
                },
                WorkflowEdge {
                    id: "branch_maybe".to_string(),
                    from: "decide".to_string(),
                    to: "missing".to_string(),
                    outcome: WorkflowEdgeOutcome::Branch,
                    label: Some("maybe".to_string()),
                    branch_id: None,
                    condition: None,
                },
            ],
            "decide",
        );
        let mut parent = workflow(vec![call], vec![], "call");
        parent
            .subflows
            .insert("broken".to_string(), Box::new(broken_subflow));

        let result = validate_workflow(parent);

        assert!(
            result.issues.iter().any(|issue| {
                issue.severity == "error"
                    && issue.node_id.as_deref() == Some("decide")
                    && issue.scope.as_deref() == Some("subflow:broken")
                    && issue.message.contains("subflow:broken")
                    && issue
                        .message
                        .contains("does not match an outgoing branch edge label")
            }),
            "expected scoped decide validation error, got {:?}",
            result.issues
        );
        assert!(
            result.issues.iter().any(|issue| {
                issue.severity == "error"
                    && issue.node_id.as_deref() == Some("decide")
                    && issue.scope.as_deref() == Some("subflow:broken")
                    && issue.message.contains("subflow:broken")
                    && issue.message.contains("references unknown target node")
            }),
            "expected scoped dangling edge validation error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn warns_on_subflow_call_cycles() {
        let mut call_beta = node("call-beta", "Call Beta", WorkflowNodeType::Call);
        call_beta.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "beta".to_string(),
                exit_node_id: Some("call-alpha".to_string()),
                inputs: Vec::new(),
                max_depth: 3,
            },
        };
        let alpha = workflow(vec![call_beta], vec![], "call-beta");

        let mut call_alpha = node("call-alpha", "Call Alpha", WorkflowNodeType::Call);
        call_alpha.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "alpha".to_string(),
                exit_node_id: Some("call-beta".to_string()),
                inputs: Vec::new(),
                max_depth: 5,
            },
        };
        let beta = workflow(vec![call_alpha], vec![], "call-alpha");

        let mut parent = workflow(
            vec![node("start", "Start", WorkflowNodeType::Task)],
            vec![],
            "start",
        );
        parent.subflows.insert("alpha".to_string(), Box::new(alpha));
        parent.subflows.insert("beta".to_string(), Box::new(beta));

        let result = validate_workflow(parent);

        let cycle_warnings = result
            .issues
            .iter()
            .filter(|issue| issue.message.contains("Subflow call cycle detected"))
            .collect::<Vec<_>>();
        assert_eq!(cycle_warnings.len(), 1, "got {cycle_warnings:?}");
        assert_eq!(cycle_warnings[0].severity, "warning");
        assert!(
            cycle_warnings[0].message.contains("subflows {alpha, beta}"),
            "got {}",
            cycle_warnings[0].message
        );
    }

    #[test]
    fn rejects_nested_subflow_catalogs_at_ingress() {
        let mut nested = workflow(
            vec![node("inner", "Inner", WorkflowNodeType::Task)],
            vec![],
            "inner",
        );
        nested.subflows.insert(
            "nested".to_string(),
            Box::new(workflow(
                vec![node("leaf", "Leaf", WorkflowNodeType::Task)],
                vec![],
                "leaf",
            )),
        );

        let mut parent = workflow(
            vec![node("start", "Start", WorkflowNodeType::Task)],
            vec![],
            "start",
        );
        parent.subflows.insert("outer".to_string(), Box::new(nested));

        let result = validate_workflow(parent);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "error"
                && issue.message.contains("nested subflow catalog")
                && issue.message.contains("globally scoped at the root")
        }));
    }

    #[test]
    fn rejects_nested_subflow_catalog_used_to_bypass_subflow_count_limit() {
        let mut parent = workflow(
            vec![node("start", "Start", WorkflowNodeType::Task)],
            vec![],
            "start",
        );
        for index in 0..MAX_WORKFLOW_SUBFLOWS {
            let node_id = format!("node-{index}");
            parent.subflows.insert(
                format!("subflow-{index}"),
                Box::new(workflow(
                    vec![node(&node_id, "Step", WorkflowNodeType::Task)],
                    vec![],
                    &node_id,
                )),
            );
        }

        let mut nested_host = workflow(
            vec![node("host", "Host", WorkflowNodeType::Task)],
            vec![],
            "host",
        );
        nested_host.subflows.insert(
            "overflow".to_string(),
            Box::new(workflow(
                vec![node("overflow-node", "Overflow", WorkflowNodeType::Task)],
                vec![],
                "overflow-node",
            )),
        );
        parent
            .subflows
            .insert("nested-host".to_string(), Box::new(nested_host));

        let result = validate_workflow(parent);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "error" && issue.message.contains("nested subflow catalog")
        }));
    }

    #[test]
    fn warns_when_subflow_defines_root_only_run_as_or_limits() {
        let mut subflow = workflow(
            vec![node("step", "Step", WorkflowNodeType::Task)],
            vec![],
            "step",
        );
        subflow.run_as = Some(RunAsConfig {
            user: Some("sandbox".to_string()),
            ..Default::default()
        });
        subflow.limits = WorkflowLimits {
            max_total_steps: 99,
            max_visits_per_node: 9,
        };

        let mut parent = workflow(
            vec![node("start", "Start", WorkflowNodeType::Task)],
            vec![],
            "start",
        );
        parent.subflows.insert("child".to_string(), Box::new(subflow));

        let result = validate_workflow(parent);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "warning"
                && issue.message.contains("Subflow \"child\" defines runAs")
        }));
        assert!(result.issues.iter().any(|issue| {
            issue.severity == "warning"
                && issue.message.contains("Subflow \"child\" defines custom limits")
        }));
    }

    #[test]
    fn does_not_warn_when_subflow_limits_are_canonical() {
        let parent_nodes = vec![node("start", "Start", WorkflowNodeType::Task)];
        let subflow_nodes = vec![node("step", "Step", WorkflowNodeType::Task)];

        let omitted = WorkflowLimits::default();
        let explicit = WorkflowLimits {
            max_total_steps: default_max_total_steps(),
            max_visits_per_node: default_max_visits_per_node(),
        };
        let serde_defaults: WorkflowLimits = serde_json::from_value(json!({})).unwrap();

        for limits in [omitted, explicit, serde_defaults] {
            let mut subflow = workflow(subflow_nodes.clone(), vec![], "step");
            subflow.limits = limits;

            let mut parent = workflow(parent_nodes.clone(), vec![], "start");
            parent.subflows.insert("child".to_string(), Box::new(subflow));

            let result = validate_workflow(parent);

            assert!(
                !result.issues.iter().any(|issue| {
                    issue.severity == "warning"
                        && issue.message.contains("Subflow \"child\" defines custom limits")
                }),
                "canonical subflow limits should not produce a custom-limits warning"
            );
        }
    }

    #[test]
    fn rejects_workflows_exceeding_subflow_count_limit() {
        let mut parent = workflow(
            vec![node("start", "Start", WorkflowNodeType::Task)],
            vec![],
            "start",
        );
        for index in 0..1_025 {
            let node_id = format!("node-{index}");
            parent.subflows.insert(
                format!("subflow-{index}"),
                Box::new(workflow(
                    vec![node(&node_id, "Step", WorkflowNodeType::Task)],
                    vec![],
                    &node_id,
                )),
            );
        }

        let result = validate_workflow(parent);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "error"
                && issue
                    .message
                    .contains("contains 1025 subflows; at most 1024 are allowed")
        }));
    }

    #[test]
    fn rejects_workflows_exceeding_subflow_call_edge_limit() {
        let mut calls = Vec::new();
        for index in 0..4_097 {
            let node_id = format!("call-{index}");
            let mut call = node(&node_id, "Call Target", WorkflowNodeType::Call);
            call.kind = NodeKind::Call {
                subflow_config: SubflowConfig {
                    workflow_name: "target".to_string(),
                    exit_node_id: Some("target".to_string()),
                    inputs: Vec::new(),
                    max_depth: default_max_call_depth(),
                },
            };
            calls.push(call);
        }
        let mut parent = workflow(calls, vec![], "call-0");
        parent.subflows.insert(
            "target".to_string(),
            Box::new(workflow(
                vec![node("target", "Target", WorkflowNodeType::Task)],
                vec![],
                "target",
            )),
        );

        let result = validate_workflow(parent);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "error"
                && issue
                    .message
                    .contains("contains 4097 subflow call edges; at most 4096 are allowed")
        }));
    }

    #[test]
    fn validates_wait_until_requires_marker() {
        let mut wait = node("wait", "Wait", WorkflowNodeType::Wait);
        wait.kind = NodeKind::Wait {
            wait_config: WaitConfig {
                mode: WaitMode::Until,
                ..Default::default()
            },
        };
        let result = validate_workflow(workflow(vec![wait], vec![], "wait"));

        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.severity == "error" && issue.message.contains("marker")),
            "expected marker validation error, got {:?}",
            result.issues
        );

        let mut invalid_regex = node("wait", "Wait", WorkflowNodeType::Wait);
        invalid_regex.kind = NodeKind::Wait {
            wait_config: WaitConfig {
                mode: WaitMode::Until,
                marker: Some("[unclosed".to_string()),
                ..Default::default()
            },
        };
        let invalid_regex_result = validate_workflow(workflow(vec![invalid_regex], vec![], "wait"));
        assert!(
            invalid_regex_result.issues.iter().any(|issue| {
                issue.severity == "error" && issue.message.contains("invalid wait marker regex")
            }),
            "expected invalid regex error, got {:?}",
            invalid_regex_result.issues
        );

        let mut negative_idle = node("wait", "Wait", WorkflowNodeType::Wait);
        negative_idle.kind = NodeKind::Wait {
            wait_config: WaitConfig {
                idle_seconds: Some(-1.0),
                ..Default::default()
            },
        };
        let negative_idle_result = validate_workflow(workflow(vec![negative_idle], vec![], "wait"));
        assert!(
            negative_idle_result.issues.iter().any(|issue| {
                issue.severity == "error"
                    && issue.message.contains("idle_seconds")
                    && issue.message.contains("finite non-negative")
            }),
            "expected negative idle_seconds error, got {:?}",
            negative_idle_result.issues
        );

        let mut nan_ready = node("wait", "Wait", WorkflowNodeType::Wait);
        nan_ready.kind = NodeKind::Wait {
            wait_config: WaitConfig {
                ready_stable_seconds: Some(f64::NAN),
                ..Default::default()
            },
        };
        let nan_ready_result = validate_workflow(workflow(vec![nan_ready], vec![], "wait"));
        assert!(
            nan_ready_result.issues.iter().any(|issue| {
                issue.severity == "error"
                    && issue.message.contains("ready_stable_seconds")
                    && issue.message.contains("finite non-negative")
            }),
            "expected NaN ready_stable_seconds error, got {:?}",
            nan_ready_result.issues
        );
    }

    // -----------------------------------------------------------------------
    // T15: Subflow validation — recursion depth bound + call-cycle warning
    // -----------------------------------------------------------------------

    #[test]
    fn validates_subflow_max_depth_zero_is_error() {
        // A call node with maxDepth=0 must be rejected at validation time.
        let mut call = node("call", "Call Zero Depth", WorkflowNodeType::Call);
        call.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "sub".to_string(),
                exit_node_id: Some("exit".to_string()),
                inputs: Vec::new(),
                max_depth: 0, // invalid
            },
        };
        let sub = workflow(
            vec![
                node("entry", "Entry", WorkflowNodeType::Task),
                node("exit", "Exit", WorkflowNodeType::Task),
            ],
            vec![success_edge("e", "entry", "exit", None)],
            "entry",
        );
        let mut parent = workflow(vec![call], vec![], "call");
        parent.subflows.insert("sub".to_string(), Box::new(sub));

        let result = validate_workflow(parent);

        assert!(
            result.issues.iter().any(|issue| {
                issue.severity == "error" && issue.message.contains("maxDepth must be at least 1")
            }),
            "expected maxDepth=0 error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn recursion_cycle_warning_includes_depth_bound_in_message() {
        // A subflow that directly calls itself should emit a cycle warning that
        // mentions the maxDepth bound so the user knows runtime safety applies.
        let mut self_call = node("call", "Self Call", WorkflowNodeType::Call);
        self_call.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "recurse".to_string(),
                exit_node_id: Some("call".to_string()),
                inputs: Vec::new(),
                max_depth: 5,
            },
        };
        // The "recurse" subflow body IS self_call — single terminal node
        let recurse_body = workflow(vec![self_call], vec![], "call");
        let mut parent = workflow(
            vec![node("start", "Start", WorkflowNodeType::Task)],
            vec![],
            "start",
        );
        parent
            .subflows
            .insert("recurse".to_string(), Box::new(recurse_body));

        let result = validate_workflow(parent);

        let cycle_issue = result.issues.iter().find(|issue| {
            issue.severity == "warning" && issue.message.contains("Subflow call cycle detected")
        });
        assert!(
            cycle_issue.is_some(),
            "expected cycle warning, got {:?}",
            result.issues
        );
        // The warning should mention the maxDepth value (5) so users know recursion is bounded.
        assert!(
            cycle_issue.unwrap().message.contains("5"),
            "cycle warning should mention maxDepth value"
        );
    }

    #[test]
    fn validates_subflow_unknown_workflow_name_is_error() {
        // A call node referencing a workflow not in `subflows` must fail.
        let mut call = node("call", "Call Missing", WorkflowNodeType::Call);
        call.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "no_such_workflow".to_string(),
                exit_node_id: None,
                inputs: Vec::new(),
                max_depth: default_max_call_depth(),
            },
        };
        let parent = workflow(vec![call], vec![], "call");
        let result = validate_workflow(parent);

        assert!(
            result.issues.iter().any(|issue| {
                issue.severity == "error" && issue.message.contains("references unknown subflow")
            }),
            "expected unknown subflow error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn validates_subflow_with_multiple_terminal_nodes_is_error() {
        // A subflow body with two terminal nodes violates single-exit contract.
        let mut call = node("call", "Call Multi-Exit", WorkflowNodeType::Call);
        call.kind = NodeKind::Call {
            subflow_config: SubflowConfig {
                workflow_name: "multi".to_string(),
                exit_node_id: None, // no explicit exitNodeId → auto-detect
                inputs: Vec::new(),
                max_depth: default_max_call_depth(),
            },
        };
        // Two terminal nodes (no outgoing edges)
        let sub = workflow(
            vec![
                node("exit_a", "Exit A", WorkflowNodeType::Task),
                node("exit_b", "Exit B", WorkflowNodeType::Task),
            ],
            vec![], // no edges → both are terminal
            "exit_a",
        );
        let mut parent = workflow(vec![call], vec![], "call");
        parent.subflows.insert("multi".to_string(), Box::new(sub));

        let result = validate_workflow(parent);

        assert!(
            result.issues.iter().any(|issue| {
                issue.severity == "error" && issue.message.contains("exactly one exit node")
            }),
            "expected single-exit validation error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn validates_decide_node_outcomes_must_match_branch_edges() {
        // A Decide node whose outcomes don't match branch edge labels should error.
        let mut decide = node("decide", "Route", WorkflowNodeType::Decide);
        decide.kind = NodeKind::Decide {
            decide_config: DecideConfig {
                inputs: Vec::new(),
                prompt: "Pick a path".to_string(),
                model: None,
                outcomes: vec!["yes".to_string(), "no".to_string()],
            },
        };
        let yes_target = node("yes_node", "Yes", WorkflowNodeType::Task);
        let parent = workflow(
            vec![decide, yes_target],
            vec![WorkflowEdge {
                id: "branch_yes".to_string(),
                from: "decide".to_string(),
                to: "yes_node".to_string(),
                outcome: WorkflowEdgeOutcome::Branch,
                label: Some("yes".to_string()),
                branch_id: None,
                condition: None,
            }],
            "decide",
        );
        // "no" outcome has no matching branch edge → should error
        let result = validate_workflow(parent);

        assert!(
            result.issues.iter().any(|issue| {
                issue.severity == "error"
                    && issue
                        .message
                        .contains("does not match an outgoing branch edge label")
            }),
            "expected outcome→edge mismatch error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn epic_dev_template_parses_and_validates_clean() {
        // Load and validate the bundled epic-dev.json template at the schema level.
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/epic-dev.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("templates/epic-dev.json not found at {:?}", path));
        let value: serde_json::Value =
            serde_json::from_str(&raw).expect("epic-dev.json must be valid JSON");

        let normalized =
            normalize_workflow_value(value).expect("epic-dev.json must parse as a v3 workflow");

        let result = validate_workflow(normalized.workflow);
        let errors: Vec<_> = result
            .issues
            .iter()
            .filter(|issue| issue.severity == "error")
            .collect();
        assert!(
            errors.is_empty(),
            "epic-dev.json has validation errors: {:?}",
            errors
        );
    }

    // -----------------------------------------------------------------------
    // Stage 9: runAs serde round-trip
    // -----------------------------------------------------------------------

    /// A WorkflowV3 with `runAs` serializes/deserializes with camelCase keys
    /// and the fields survive the round-trip intact.
    #[test]
    fn run_as_config_round_trips_with_camel_case_keys() {
        let workflow = WorkflowV3 {
            version: WORKFLOW_SCHEMA_VERSION,
            name: Some("run-as-test".to_string()),
            goal: "test".to_string(),
            cwd: "/tmp".to_string(),
            use_orchestrator: false,
            run_as: Some(RunAsConfig {
                user: Some("agent".to_string()),
                command: None,
                socket: Some("test-socket".to_string()),
            }),
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        };

        let serialized = serde_json::to_value(&workflow).expect("serialization must succeed");

        // Verify camelCase keys are used
        assert!(
            serialized.get("runAs").is_some(),
            "top-level key should be camelCase \"runAs\", got: {:?}",
            serialized.as_object().map(|m| m.keys().collect::<Vec<_>>())
        );
        let run_as_val = &serialized["runAs"];
        assert_eq!(run_as_val["user"], "agent");
        assert_eq!(run_as_val["socket"], "test-socket");
        // command is None → serialized as null (no skip_serializing_if on RunAsConfig fields)
        assert!(
            run_as_val["command"].is_null(),
            "None command should serialize as null, got: {:?}",
            run_as_val["command"]
        );

        // Verify round-trip fidelity
        let deserialized: WorkflowV3 =
            serde_json::from_value(serialized).expect("deserialization must succeed");
        assert_eq!(workflow, deserialized);
        let run_as = deserialized.run_as.as_ref().unwrap();
        assert_eq!(run_as.user.as_deref(), Some("agent"));
        assert_eq!(run_as.socket.as_deref(), Some("test-socket"));
        assert!(run_as.command.is_none());
    }

    /// A WorkflowV3 with runAs.command round-trips correctly.
    #[test]
    fn run_as_command_variant_round_trips() {
        let workflow = WorkflowV3 {
            version: WORKFLOW_SCHEMA_VERSION,
            name: None,
            goal: String::new(),
            cwd: String::new(),
            use_orchestrator: false,
            run_as: Some(RunAsConfig {
                user: None,
                command: Some(vec![
                    "docker".to_string(),
                    "exec".to_string(),
                    "box".to_string(),
                ]),
                socket: None,
            }),
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
            agent_defaults: BTreeMap::new(),
            subflows: BTreeMap::new(),
            ui: None,
        };

        let serialized = serde_json::to_value(&workflow).expect("serialization must succeed");
        let run_as_val = &serialized["runAs"];
        assert_eq!(
            run_as_val["command"],
            serde_json::json!(["docker", "exec", "box"])
        );
        // socket is None → serialized as null (no skip_serializing_if on RunAsConfig fields)
        assert!(
            run_as_val["socket"].is_null(),
            "None socket should serialize as null, got: {:?}",
            run_as_val["socket"]
        );

        let deserialized: WorkflowV3 =
            serde_json::from_value(serialized).expect("deserialization must succeed");
        assert_eq!(workflow, deserialized);
    }

    #[test]
    fn multi_agent_plan_template_parses_and_validates_clean() {
        // Load and validate the bundled multi-agent-plan-implementation.json template.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("templates/multi-agent-plan-implementation.json");
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "multi-agent-plan-implementation.json not found at {:?}",
                path
            )
        });
        let value: serde_json::Value = serde_json::from_str(&raw)
            .expect("multi-agent-plan-implementation.json must be valid JSON");

        let normalized = normalize_workflow_value(value)
            .expect("multi-agent-plan-implementation.json must parse as a v3 workflow");

        let result = validate_workflow(normalized.workflow);
        let errors: Vec<_> = result
            .issues
            .iter()
            .filter(|issue| issue.severity == "error")
            .collect();
        assert!(
            errors.is_empty(),
            "multi-agent-plan-implementation.json has validation errors: {:?}",
            errors
        );
    }

    #[test]
    fn compute_graph_metadata_includes_parallel_batch_body_entry() {
        let batch = WorkflowNode {
            id: "batch".to_string(),
            name: "Batch".to_string(),
            kind: NodeKind::ParallelBatch {
                batch_config: BatchConfig {
                    items_binding: "items".to_string(),
                    max_concurrent: 2,
                    item_var: "item".to_string(),
                    body_entry: "body".to_string(),
                    collector_var: None,
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
        };
        let body = node("body", "Body", WorkflowNodeType::Task);
        let after = node("after", "After", WorkflowNodeType::Task);
        let wf = workflow(
            vec![batch, body, after],
            vec![success_edge("batch_after", "batch", "after", None)],
            "batch",
        );

        let meta = compute_graph_metadata(&wf);
        assert!(meta.reachable_node_ids.contains(&"body".to_string()));
        assert!(!meta.unreachable_node_ids.contains(&"body".to_string()));
    }

    #[test]
    fn validates_loop_continue_requires_loop_exit() {
        let gate = node("gate", "Gate", WorkflowNodeType::Task);
        let result = validate_workflow(workflow(
            vec![gate, node("done", "Done", WorkflowNodeType::Task)],
            vec![WorkflowEdge {
                id: "gate_continue".to_string(),
                from: "gate".to_string(),
                to: "gate".to_string(),
                outcome: WorkflowEdgeOutcome::LoopContinue,
                label: Some("again".to_string()),
                branch_id: None,
                condition: None,
            }],
            "gate",
        ));

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "error"
                && issue
                    .message
                    .contains("loop_continue edge but no loop_exit edge")
        }));
    }
}
