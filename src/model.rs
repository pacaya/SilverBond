use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::driver::{AccessMode, AgentConfig, ReasoningLevel, ToolToggles};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    let node_base = node.agent_config.as_ref().map(|o| &o.base);

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

    let cwd = node.cwd.clone().unwrap_or_else(|| workflow_cwd.to_string());

    let overrides = node.agent_config.as_ref();

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SendConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub enter: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_stable_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default = "default_run_agent_kill_after")]
    pub kill_after: bool,
}

fn default_run_agent_kill_after() -> bool {
    true
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
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: WorkflowNodeType,
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
    pub agent_config: Option<AgentNodeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continue_session_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decide_config: Option<DecideConfig>,
    #[serde(
        default,
        alias = "parallelBatchConfig",
        skip_serializing_if = "Option::is_none"
    )]
    pub batch_config: Option<BatchConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_config: Option<SpawnConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_config: Option<SendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_config: Option<WaitConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_config: Option<CaptureConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kill_config: Option<KillConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_agent_config: Option<RunAgentConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subflow_config: Option<SubflowConfig>,
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
    /// Run-local catalog of saved workflows referenced by subflow/call nodes.
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
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .context("workflow version is required")?;
    anyhow::ensure!(
        version == 3,
        "Only workflow version 3 is supported. Received version {}.",
        version
    );
    let workflow: WorkflowV3 =
        serde_json::from_value(value).context("failed to deserialize workflow")?;
    Ok(NormalizedWorkflow {
        workflow: ensure_defaults(workflow),
        notices: Vec::new(),
    })
}

pub fn ensure_defaults(mut workflow: WorkflowV3) -> WorkflowV3 {
    workflow.version = 3;
    if workflow.limits.max_total_steps == 0 {
        workflow.limits.max_total_steps = default_max_total_steps();
    }
    if workflow.limits.max_visits_per_node == 0 {
        workflow.limits.max_visits_per_node = default_max_visits_per_node();
    }
    workflow
}

pub fn validate_workflow(workflow: WorkflowV3) -> ValidationResult {
    let workflow = ensure_defaults(workflow);
    let graph = workflow.graph();
    let mut issues = Vec::new();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();

    for node in &workflow.nodes {
        if !seen_nodes.insert(node.id.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                message: format!("Duplicate node id \"{}\".", node.id),
            });
        }

        if node.node_type == WorkflowNodeType::Task
            && node.agent.as_deref().unwrap_or("").is_empty()
        {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                message: format!("\"{}\" has no agent assigned.", node.name),
            });
        }

        if node.node_type == WorkflowNodeType::Task && node.prompt.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                message: format!("\"{}\" has an empty prompt.", node.name),
            });
        }

        validate_tmux_node_config(node, &mut issues);

        if node.output_schema.is_some() && node.response_format != Some(ResponseFormat::Json) {
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                message: format!(
                    "\"{}\" has an output schema but responseFormat is not json.",
                    node.name
                ),
            });
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

        if node.node_type == WorkflowNodeType::Decide {
            validate_decide_node_config(node, outgoing, &mut issues);
        }
        if node.node_type == WorkflowNodeType::ParallelBatch {
            validate_batch_node_config(node, &graph, &mut issues);
        }
        if is_subflow_node_type(&node.node_type) {
            validate_subflow_node_config(node, &workflow.subflows, &mut issues);
        }

        if success_edges > 1 {
            if node.node_type != WorkflowNodeType::Split {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!("\"{}\" has more than one success edge.", node.name),
                });
            }
        }
        if reject_edges > 1 {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                message: format!("\"{}\" has more than one reject edge.", node.name),
            });
        }
        if branch_edges > 0 && loop_edges > 0 {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
                message: format!("\"{}\" mixes branch and loop control edges.", node.name),
            });
        }
        if node.node_type == WorkflowNodeType::Approval && branch_edges > 0 {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
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
                message: format!("\"{}\" is a terminal node.", node.name),
            });
        }

        if matches!(
            node.node_type,
            WorkflowNodeType::Split | WorkflowNodeType::Collector
        ) && has_task_execution_config(node)
        {
            issues.push(ValidationIssue {
                severity: "warning".to_string(),
                node_id: Some(node.id.clone()),
                message: format!(
                    "\"{}\" is a {} node, so task execution fields are ignored.",
                    node.name,
                    match node.node_type {
                        WorkflowNodeType::Split => "split",
                        WorkflowNodeType::Collector => "collector",
                        _ => unreachable!(),
                    }
                ),
            });
        }

        if node.node_type == WorkflowNodeType::Split {
            if branch_edges > 0 || loop_edges > 0 || reject_edges > 0 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
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
                    message: format!("\"{}\" has no outbound split edges.", node.name),
                });
            } else if success_edges < 2 {
                issues.push(ValidationIssue {
                    severity: "warning".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!("\"{}\" fans out to fewer than two branches.", node.name),
                });
            }
        }

        if node.node_type == WorkflowNodeType::Collector {
            if inbound.is_empty() {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!("\"{}\" has no inbound branches to collect.", node.name),
                });
            }
            if success_edges != 1 {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
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
                    source_node.node_type,
                    WorkflowNodeType::Task | WorkflowNodeType::RunAgent
                ) {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
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
                    message: format!(
                        "\"{}\" references unknown node \"{}\" for session continuation.",
                        node.name, source_id
                    ),
                });
            }
        }
    }

    if graph
        .node_map
        .get(workflow.entry_node_id.as_str())
        .is_none()
    {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: None,
            message: format!(
                "entryNodeId \"{}\" references a non-existent node.",
                workflow.entry_node_id
            ),
        });
    }

    for edge in &workflow.edges {
        if !seen_edges.insert(edge.id.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(edge.from.clone()),
                message: format!("Duplicate edge id \"{}\".", edge.id),
            });
        }
        if graph.node_map.get(edge.from.as_str()).is_none() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(edge.from.clone()),
                message: format!("Edge \"{}\" references unknown source node.", edge.id),
            });
        }
        if graph.node_map.get(edge.to.as_str()).is_none() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(edge.from.clone()),
                message: format!("Edge \"{}\" references unknown target node.", edge.id),
            });
        }
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

fn validate_tmux_node_config(node: &WorkflowNode, issues: &mut Vec<ValidationIssue>) {
    match &node.node_type {
        WorkflowNodeType::Spawn => {
            let config = node.spawn_config.as_ref();
            let has_agent = config
                .and_then(|cfg| cfg.agent.as_deref())
                .or(node.agent.as_deref())
                .is_some_and(|agent| !agent.trim().is_empty());
            let has_command = config
                .and_then(|cfg| cfg.command.as_deref())
                .is_some_and(|command| !command.trim().is_empty());
            if !has_agent && !has_command {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!("\"{}\" spawn node requires an agent or command.", node.name),
                });
            }
        }
        WorkflowNodeType::Send => {
            let text = node
                .send_config
                .as_ref()
                .map(|cfg| cfg.text.as_str())
                .unwrap_or(node.prompt.as_str());
            if text.trim().is_empty() && node.prompt.trim().is_empty() {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!("\"{}\" send node requires text or prompt.", node.name),
                });
            }
        }
        WorkflowNodeType::Wait => {
            let config = node.wait_config.as_ref();
            if config.is_some_and(|cfg| cfg.mode == WaitMode::Until)
                && config
                    .and_then(|cfg| cfg.marker.as_deref())
                    .is_none_or(|marker| marker.trim().is_empty())
            {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!(
                        "\"{}\" wait node with until mode requires a marker.",
                        node.name
                    ),
                });
            }
        }
        WorkflowNodeType::RunAgent => {
            let has_agent = node
                .run_agent_config
                .as_ref()
                .and_then(|cfg| cfg.agent.as_deref())
                .or(node.agent.as_deref())
                .is_some_and(|agent| !agent.trim().is_empty());
            if !has_agent {
                issues.push(ValidationIssue {
                    severity: "error".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!("\"{}\" run_agent node requires an agent.", node.name),
                });
            }
            let prompt = node
                .run_agent_config
                .as_ref()
                .and_then(|cfg| cfg.prompt.as_deref())
                .unwrap_or(node.prompt.as_str());
            if prompt.trim().is_empty() {
                issues.push(ValidationIssue {
                    severity: "warning".to_string(),
                    node_id: Some(node.id.clone()),
                    message: format!("\"{}\" run_agent node has an empty prompt.", node.name),
                });
            }
        }
        WorkflowNodeType::Task
        | WorkflowNodeType::Approval
        | WorkflowNodeType::Split
        | WorkflowNodeType::Collector
        | WorkflowNodeType::Decide
        | WorkflowNodeType::ParallelBatch
        | WorkflowNodeType::Subflow
        | WorkflowNodeType::Call
        | WorkflowNodeType::Capture
        | WorkflowNodeType::Kill => {}
    }
}

fn is_subflow_node_type(node_type: &WorkflowNodeType) -> bool {
    matches!(
        node_type,
        WorkflowNodeType::Subflow | WorkflowNodeType::Call
    )
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
                message: format!(
                    "Subflow \"{}\" entryNodeId \"{}\" references a non-existent node.",
                    subflow_name, subflow.entry_node_id
                ),
            });
        }

        for node in &subflow.nodes {
            if is_subflow_node_type(&node.node_type) {
                validate_subflow_node_config(node, &workflow.subflows, issues);
            }
        }
    }
}

fn validate_subflow_node_config(
    node: &WorkflowNode,
    subflows: &BTreeMap<String, Box<WorkflowV3>>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(config) = node.subflow_config.as_ref() else {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            message: format!(
                "\"{}\" {} node requires subflowConfig.",
                node.name,
                node.node_type.as_str()
            ),
        });
        return;
    };

    let workflow_name = config.workflow_name.trim();
    if workflow_name.is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            message: format!(
                "\"{}\" {} node requires subflowConfig.workflowName.",
                node.name,
                node.node_type.as_str()
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

    let mut emitted = BTreeSet::new();
    for start in graph.keys() {
        let mut stack = Vec::new();
        detect_subflow_cycle(start, &graph, &mut stack, &mut emitted, issues);
    }
}

fn collect_subflow_calls(
    workflow_name: &str,
    workflow: &WorkflowV3,
    graph: &mut BTreeMap<String, Vec<(String, String, u32)>>,
) {
    for node in &workflow.nodes {
        if !is_subflow_node_type(&node.node_type) {
            continue;
        }
        let Some(config) = node.subflow_config.as_ref() else {
            continue;
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

fn detect_subflow_cycle(
    current: &str,
    graph: &BTreeMap<String, Vec<(String, String, u32)>>,
    stack: &mut Vec<String>,
    emitted: &mut BTreeSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(edges) = graph.get(current) else {
        return;
    };
    stack.push(current.to_string());
    for (callee, node_id, max_depth) in edges {
        if let Some(position) = stack.iter().position(|name| name == callee) {
            let mut cycle = stack[position..].to_vec();
            cycle.push(callee.clone());
            let signature = cycle.join(" -> ");
            if emitted.insert(signature.clone()) {
                issues.push(ValidationIssue {
                    severity: "warning".to_string(),
                    node_id: Some(node_id.clone()),
                    message: format!(
                        "Subflow call cycle detected: {}. maxDepth ({}) bounds recursion at runtime.",
                        signature, max_depth
                    ),
                });
            }
            continue;
        }
        detect_subflow_cycle(callee, graph, stack, emitted, issues);
    }
    stack.pop();
}

fn validate_decide_node_config(
    node: &WorkflowNode,
    outgoing: &[&WorkflowEdge],
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(config) = &node.decide_config else {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            message: format!("\"{}\" decide node requires decideConfig.", node.name),
        });
        return;
    };

    if config.prompt.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: "warning".to_string(),
            node_id: Some(node.id.clone()),
            message: format!("\"{}\" decide node has an empty prompt.", node.name),
        });
    }

    let mut seen_inputs = BTreeSet::new();
    for input in &config.inputs {
        if input.name.trim().is_empty() || input.source.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
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
                message: format!("\"{}\" decide node has an empty outcome label.", node.name),
            });
            continue;
        }
        if !seen_outcomes.insert(outcome.clone()) {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                node_id: Some(node.id.clone()),
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
    graph: &WorkflowGraph<'_>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(config) = &node.batch_config else {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            message: format!(
                "\"{}\" parallel_batch node requires batchConfig.",
                node.name
            ),
        });
        return;
    };

    if config.items_binding.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
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
            message: format!("\"{}\" parallel_batch node requires itemVar.", node.name),
        });
    }
    if config.body_entry.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
            message: format!("\"{}\" parallel_batch node requires bodyEntry.", node.name),
        });
    } else if graph.node_map.get(config.body_entry.as_str()).is_none() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            node_id: Some(node.id.clone()),
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
        || node.agent_config.is_some()
        || node.cwd.is_some()
        || node.decide_config.is_some()
        || node.batch_config.is_some()
        || node.subflow_config.is_some()
}

pub fn compute_graph_metadata(workflow: &WorkflowV3) -> GraphMetadata {
    let graph = workflow.graph();
    let mut reachable = BTreeSet::new();
    let mut queue = VecDeque::from([workflow.entry_node_id.clone()]);
    while let Some(node_id) = queue.pop_front() {
        if !reachable.insert(node_id.clone()) {
            continue;
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
            node_type,
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
            decide_config: None,
            batch_config: None,
            spawn_config: None,
            send_config: None,
            wait_config: None,
            capture_config: None,
            kill_config: None,
            run_agent_config: None,
            subflow_config: None,
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
            version: 3,
            name: Some("test".to_string()),
            goal: "goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
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
            version: 3,
            name: None,
            goal: String::new(),
            cwd: String::new(),
            use_orchestrator: false,
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
        n.agent_config = Some(AgentNodeConfig {
            base: AgentDefaults {
                model: Some("sonnet".to_string()),
                ..Default::default()
            },
            ..Default::default()
        });
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
            "version": 3,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "n1",
                "name": "Step 1",
                "type": "task",
                "agent": "claude",
                "prompt": "hello"
            }],
            "edges": []
        });
        let result = normalize_workflow_value(json);
        assert!(result.is_ok());
        let w = result.unwrap().workflow;
        assert!(w.agent_defaults.is_empty());
        assert!(w.nodes[0].agent_config.is_none());
        assert!(w.nodes[0].cwd.is_none());
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
            "version": 3,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "n1",
                "name": "Step 1",
                "type": "task",
                "agent": "claude",
                "prompt": "hello",
                "responseFormat": "json",
                "outputSchema": {"name": "string", "score": "number"}
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
            "version": 3,
            "goal": "test",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "n1",
                "name": "Step 1",
                "type": "task",
                "agent": "claude",
                "prompt": "hello",
                "responseFormat": "json",
                "outputSchema": full_schema
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
            "version": 3,
            "goal": "Run one agent",
            "entryNodeId": "run",
            "nodes": [{
                "id": "run",
                "name": "Run Echo",
                "type": "run_agent",
                "agent": "echo",
                "prompt": "hello"
            }],
            "edges": []
        });

        let normalized = normalize_workflow_value(value).unwrap();
        assert_eq!(
            normalized.workflow.nodes[0].node_type,
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
        call.subflow_config = Some(SubflowConfig {
            workflow_name: "double".to_string(),
            exit_node_id: Some("exit".to_string()),
            inputs: Vec::new(),
            max_depth: default_max_call_depth(),
        });
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
    fn warns_on_subflow_call_cycles() {
        let mut call = node("call", "Recursive Call", WorkflowNodeType::Call);
        call.subflow_config = Some(SubflowConfig {
            workflow_name: "recursive".to_string(),
            exit_node_id: Some("call".to_string()),
            inputs: Vec::new(),
            max_depth: 3,
        });
        let recursive = workflow(vec![call], vec![], "call");
        let mut parent = workflow(
            vec![node("start", "Start", WorkflowNodeType::Task)],
            vec![],
            "start",
        );
        parent
            .subflows
            .insert("recursive".to_string(), Box::new(recursive));

        let result = validate_workflow(parent);

        assert!(result.issues.iter().any(|issue| {
            issue.severity == "warning" && issue.message.contains("Subflow call cycle detected")
        }));
    }

    #[test]
    fn validates_wait_until_requires_marker() {
        let mut wait = node("wait", "Wait", WorkflowNodeType::Wait);
        wait.wait_config = Some(WaitConfig {
            mode: WaitMode::Until,
            ..Default::default()
        });
        let result = validate_workflow(workflow(vec![wait], vec![], "wait"));

        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.severity == "error" && issue.message.contains("marker")),
            "expected marker validation error, got {:?}",
            result.issues
        );
    }

    // -----------------------------------------------------------------------
    // T15: Subflow validation — recursion depth bound + call-cycle warning
    // -----------------------------------------------------------------------

    #[test]
    fn validates_subflow_max_depth_zero_is_error() {
        // A call node with maxDepth=0 must be rejected at validation time.
        let mut call = node("call", "Call Zero Depth", WorkflowNodeType::Call);
        call.subflow_config = Some(SubflowConfig {
            workflow_name: "sub".to_string(),
            exit_node_id: Some("exit".to_string()),
            inputs: Vec::new(),
            max_depth: 0, // invalid
        });
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
        self_call.subflow_config = Some(SubflowConfig {
            workflow_name: "recurse".to_string(),
            exit_node_id: Some("call".to_string()),
            inputs: Vec::new(),
            max_depth: 5,
        });
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
        call.subflow_config = Some(SubflowConfig {
            workflow_name: "no_such_workflow".to_string(),
            exit_node_id: None,
            inputs: Vec::new(),
            max_depth: default_max_call_depth(),
        });
        let parent = workflow(vec![call], vec![], "call");
        let result = validate_workflow(parent);

        assert!(
            result.issues.iter().any(|issue| {
                issue.severity == "error"
                    && issue.message.contains("references unknown subflow")
            }),
            "expected unknown subflow error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn validates_subflow_with_multiple_terminal_nodes_is_error() {
        // A subflow body with two terminal nodes violates single-exit contract.
        let mut call = node("call", "Call Multi-Exit", WorkflowNodeType::Call);
        call.subflow_config = Some(SubflowConfig {
            workflow_name: "multi".to_string(),
            exit_node_id: None, // no explicit exitNodeId → auto-detect
            inputs: Vec::new(),
            max_depth: default_max_call_depth(),
        });
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
                issue.severity == "error"
                    && issue.message.contains("exactly one exit node")
            }),
            "expected single-exit validation error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn validates_decide_node_outcomes_must_match_branch_edges() {
        // A Decide node whose outcomes don't match branch edge labels should error.
        let mut decide = node("decide", "Route", WorkflowNodeType::Decide);
        decide.decide_config = Some(DecideConfig {
            inputs: Vec::new(),
            prompt: "Pick a path".to_string(),
            model: None,
            outcomes: vec!["yes".to_string(), "no".to_string()],
        });
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
                    && issue.message.contains("does not match an outgoing branch edge label")
            }),
            "expected outcome→edge mismatch error, got {:?}",
            result.issues
        );
    }

    #[test]
    fn epic_dev_template_parses_and_validates_clean() {
        // Load and validate the bundled epic-dev.json template at the schema level.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("templates/epic-dev.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("templates/epic-dev.json not found at {:?}", path));
        let value: serde_json::Value =
            serde_json::from_str(&raw).expect("epic-dev.json must be valid JSON");

        let normalized = normalize_workflow_value(value)
            .expect("epic-dev.json must parse as a v3 workflow");

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

    #[test]
    fn multi_agent_plan_template_parses_and_validates_clean() {
        // Load and validate the bundled multi-agent-plan-implementation.json template.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("templates/multi-agent-plan-implementation.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("multi-agent-plan-implementation.json not found at {:?}", path));
        let value: serde_json::Value =
            serde_json::from_str(&raw).expect("multi-agent-plan-implementation.json must be valid JSON");

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
}
