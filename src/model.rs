use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::driver::{AccessMode, AgentConfig, ReasoningLevel, ToolToggles};

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
#[serde(rename_all = "lowercase")]
pub enum WorkflowNodeType {
    Task,
    Approval,
    Split,
    Collector,
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

    let cwd = node
        .cwd
        .clone()
        .unwrap_or_else(|| workflow_cwd.to_string());

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
    let is_legacy = !obj.is_empty()
        && !obj.contains_key("type")
        && obj.values().all(|v| v.is_string());
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
                // Must be a task node
                if source_node.node_type != WorkflowNodeType::Task {
                    issues.push(ValidationIssue {
                        severity: "error".to_string(),
                        node_id: Some(node.id.clone()),
                        message: format!(
                            "\"{}\" continues session from \"{}\" which is not a task node.",
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

    fn workflow(nodes: Vec<WorkflowNode>, edges: Vec<WorkflowEdge>, entry_node_id: &str) -> WorkflowV3 {
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
            issue.message.contains("can only use success edges for split fan-out")
        }));
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.message.contains("fans out to fewer than two branches")));
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

        assert!(result
            .issues
            .iter()
            .any(|issue| issue.message.contains("duplicate collector input key \"dup\"")));
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.message.contains("must have exactly one outbound success edge")));
        assert!(result.issues.iter().any(|issue| {
            issue.message.contains("can only use a success edge after collecting inputs")
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

        assert!(result
            .issues
            .iter()
            .any(|issue| issue.message.contains("split node, so task execution fields are ignored")));
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.message.contains("collector node, so task execution fields are ignored")));
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
        let config =
            resolve_agent_config(&defaults, "/work", "claude", &n, None, false, None);
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
        let config =
            resolve_agent_config(&defaults, "/work", "claude", &n, None, false, None);
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

        let result = validate_workflow(workflow(
            vec![n1],
            vec![],
            "n1",
        ));
        assert!(result.issues.iter().any(|i| {
            i.severity == "error" && i.message.contains("references unknown node \"nonexistent\"")
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
        assert!(result.issues.iter().any(|i| {
            i.severity == "error" && i.message.contains("different agents")
        }));
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
        assert!(result.issues.iter().any(|i| {
            i.severity == "error" && i.message.contains("not a task node")
        }));
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
        assert!(!result.issues.iter().any(|i| {
            i.severity == "error" && i.message.contains("session")
        }));
    }
}
