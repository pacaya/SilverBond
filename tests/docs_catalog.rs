use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{self, value::MapDeserializer, Deserialize, Deserializer, Visitor};
use serde_json::Value;
use silverbond::model::{
    ensure_defaults, validate_workflow, BatchConfig, CaptureConfig, DecideConfig, KillConfig,
    NodeKind, RunAgentConfig, SendConfig, SpawnConfig, SubflowConfig, WaitConfig, WaitMode,
    WorkflowEdge, WorkflowEdgeOutcome, WorkflowNode, WorkflowNodeType, WorkflowV3,
    WorkflowVariable, WORKFLOW_SCHEMA_VERSION,
};

const REGEN_ENV: &str = "SB_REGEN_DOCS";

const CATALOG_ORDER: &[WorkflowNodeType] = &[
    WorkflowNodeType::Task,
    WorkflowNodeType::Approval,
    WorkflowNodeType::Split,
    WorkflowNodeType::Collector,
    WorkflowNodeType::Decide,
    WorkflowNodeType::ParallelBatch,
    WorkflowNodeType::Subflow,
    WorkflowNodeType::Call,
    WorkflowNodeType::Spawn,
    WorkflowNodeType::Send,
    WorkflowNodeType::Wait,
    WorkflowNodeType::Capture,
    WorkflowNodeType::Kill,
    WorkflowNodeType::RunAgent,
];

fn docs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/workflow-schema.md")
}

fn blank_node(id: &str, name: &str, kind: NodeKind) -> WorkflowNode {
    WorkflowNode {
        id: id.to_string(),
        name: name.to_string(),
        kind,
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
        split_failure_policy: Default::default(),
        cwd: None,
        continue_session_from: None,
    }
}

fn task_node(id: &str, name: &str) -> WorkflowNode {
    let mut node = blank_node(id, name, NodeKind::Task { agent_config: None });
    node.agent = Some("claude".to_string());
    node.prompt = "Complete the step.".to_string();
    node
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

fn branch_edge(id: &str, from: &str, to: &str, label: &str) -> WorkflowEdge {
    WorkflowEdge {
        id: id.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        outcome: WorkflowEdgeOutcome::Branch,
        label: Some(label.to_string()),
        branch_id: None,
        condition: None,
    }
}

fn base_workflow(
    workflow_name: &str,
    nodes: Vec<WorkflowNode>,
    edges: Vec<WorkflowEdge>,
    entry_node_id: &str,
) -> WorkflowV3 {
    WorkflowV3 {
        version: WORKFLOW_SCHEMA_VERSION,
        name: Some(workflow_name.to_string()),
        goal: "Illustrate a node kind".to_string(),
        cwd: String::new(),
        use_orchestrator: false,
        run_as: None,
        entry_node_id: entry_node_id.to_string(),
        variables: Vec::new(),
        limits: Default::default(),
        nodes,
        edges,
        agent_defaults: Default::default(),
        subflows: Default::default(),
        ui: None,
    }
}

fn child_subflow() -> WorkflowV3 {
    base_workflow("child", vec![task_node("exit", "Exit")], Vec::new(), "exit")
}

/// Hand-authored example data on top of the `From<WorkflowNodeType>` skeleton.
fn example_for(node_type: WorkflowNodeType) -> NodeKind {
    match node_type {
        WorkflowNodeType::Task => NodeKind::from(node_type),
        WorkflowNodeType::Approval => NodeKind::from(node_type),
        WorkflowNodeType::Split => NodeKind::from(node_type),
        WorkflowNodeType::Collector => NodeKind::from(node_type),
        WorkflowNodeType::Decide => {
            let NodeKind::Decide { decide_config } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for decide")
            };
            NodeKind::Decide {
                decide_config: DecideConfig {
                    prompt: "Choose a path".to_string(),
                    outcomes: vec!["yes".to_string(), "no".to_string()],
                    ..decide_config
                },
            }
        }
        WorkflowNodeType::ParallelBatch => {
            let NodeKind::ParallelBatch { batch_config } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for parallel_batch")
            };
            NodeKind::ParallelBatch {
                batch_config: BatchConfig {
                    items_binding: "items".to_string(),
                    item_var: "item".to_string(),
                    body_entry: "body".to_string(),
                    ..batch_config
                },
            }
        }
        WorkflowNodeType::Subflow => {
            let NodeKind::Subflow { subflow_config } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for subflow")
            };
            NodeKind::Subflow {
                subflow_config: SubflowConfig {
                    workflow_name: "child".to_string(),
                    ..subflow_config
                },
            }
        }
        WorkflowNodeType::Call => {
            let NodeKind::Call { subflow_config } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for call")
            };
            NodeKind::Call {
                subflow_config: SubflowConfig {
                    workflow_name: "child".to_string(),
                    ..subflow_config
                },
            }
        }
        WorkflowNodeType::Spawn => {
            let NodeKind::Spawn { spawn_config } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for spawn")
            };
            NodeKind::Spawn {
                spawn_config: SpawnConfig {
                    agent: Some("claude".to_string()),
                    session_name: Some("catalog-spawn".to_string()),
                    ..spawn_config
                },
            }
        }
        WorkflowNodeType::Send => {
            let NodeKind::Send { .. } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for send")
            };
            NodeKind::Send {
                send_config: SendConfig {
                    target: Some("catalog-session".to_string()),
                    text: "hello".to_string(),
                    enter: true,
                },
            }
        }
        WorkflowNodeType::Wait => {
            let NodeKind::Wait { wait_config } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for wait")
            };
            NodeKind::Wait {
                wait_config: WaitConfig {
                    target: Some("catalog-session".to_string()),
                    mode: WaitMode::Idle,
                    ..wait_config
                },
            }
        }
        WorkflowNodeType::Capture => {
            let NodeKind::Capture { .. } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for capture")
            };
            NodeKind::Capture {
                capture_config: CaptureConfig {
                    target: Some("catalog-session".to_string()),
                    lines: Some(50),
                    all: false,
                    ansi: true,
                },
            }
        }
        WorkflowNodeType::Kill => {
            let NodeKind::Kill { kill_config } = NodeKind::from(node_type) else {
                unreachable!("From<WorkflowNodeType> skeleton for kill")
            };
            NodeKind::Kill {
                kill_config: KillConfig {
                    target: Some("catalog-session".to_string()),
                    ..kill_config
                },
            }
        }
        WorkflowNodeType::RunAgent => {
            let NodeKind::RunAgent {
                run_agent_config,
                agent_config,
            } = NodeKind::from(node_type)
            else {
                unreachable!("From<WorkflowNodeType> skeleton for run_agent")
            };
            NodeKind::RunAgent {
                run_agent_config: RunAgentConfig {
                    agent: Some("claude".to_string()),
                    prompt: Some("Run a short command".to_string()),
                    ..run_agent_config
                },
                agent_config,
            }
        }
    }
}

fn example_workflow(node_type: WorkflowNodeType) -> (WorkflowV3, &'static str) {
    let example_id = "example";
    match node_type {
        WorkflowNodeType::Task => {
            let mut node = blank_node(example_id, "Task", example_for(node_type));
            node.agent = Some("claude".to_string());
            node.prompt = "Complete the step.".to_string();
            (
                base_workflow("node-catalog-example", vec![node], Vec::new(), example_id),
                example_id,
            )
        }
        WorkflowNodeType::Approval => (
            base_workflow(
                "node-catalog-example",
                vec![blank_node(example_id, "Approval", example_for(node_type))],
                Vec::new(),
                example_id,
            ),
            example_id,
        ),
        WorkflowNodeType::Split => (
            base_workflow(
                "node-catalog-example",
                vec![
                    blank_node(example_id, "Split", example_for(node_type)),
                    task_node("branch_a", "Branch A"),
                    task_node("branch_b", "Branch B"),
                ],
                vec![
                    success_edge("split_a", example_id, "branch_a", None),
                    success_edge("split_b", example_id, "branch_b", None),
                ],
                example_id,
            ),
            example_id,
        ),
        WorkflowNodeType::Collector => (
            base_workflow(
                "node-catalog-example",
                vec![
                    blank_node("split", "Split", NodeKind::Split),
                    task_node("branch_a", "Branch A"),
                    task_node("branch_b", "Branch B"),
                    blank_node(example_id, "Collector", example_for(node_type)),
                    task_node("after", "After"),
                ],
                vec![
                    success_edge("split_a", "split", "branch_a", None),
                    success_edge("split_b", "split", "branch_b", None),
                    success_edge("a_collect", "branch_a", example_id, Some("a")),
                    success_edge("b_collect", "branch_b", example_id, Some("b")),
                    success_edge("collect_after", example_id, "after", None),
                ],
                "split",
            ),
            example_id,
        ),
        WorkflowNodeType::Decide => (
            base_workflow(
                "node-catalog-example",
                vec![
                    blank_node(example_id, "Decide", example_for(node_type)),
                    task_node("yes_node", "Yes"),
                    task_node("no_node", "No"),
                ],
                vec![
                    branch_edge("decide_yes", example_id, "yes_node", "yes"),
                    branch_edge("decide_no", example_id, "no_node", "no"),
                ],
                example_id,
            ),
            example_id,
        ),
        WorkflowNodeType::ParallelBatch => {
            let mut workflow = base_workflow(
                "node-catalog-example",
                vec![
                    blank_node(example_id, "Batch", example_for(node_type)),
                    task_node("body", "Body"),
                ],
                Vec::new(),
                example_id,
            );
            workflow.variables = vec![WorkflowVariable {
                name: "items".to_string(),
                default: r#"["a","b"]"#.to_string(),
            }];
            (workflow, example_id)
        }
        WorkflowNodeType::Subflow | WorkflowNodeType::Call => {
            let display_name = if node_type == WorkflowNodeType::Call {
                "Call"
            } else {
                "Subflow"
            };
            let mut workflow = base_workflow(
                "node-catalog-example",
                vec![blank_node(example_id, display_name, example_for(node_type))],
                Vec::new(),
                example_id,
            );
            workflow
                .subflows
                .insert("child".to_string(), Box::new(child_subflow()));
            (workflow, example_id)
        }
        WorkflowNodeType::Spawn
        | WorkflowNodeType::Send
        | WorkflowNodeType::Wait
        | WorkflowNodeType::Capture
        | WorkflowNodeType::Kill
        | WorkflowNodeType::RunAgent => (
            base_workflow(
                "node-catalog-example",
                vec![blank_node(
                    example_id,
                    node_type.as_str(),
                    example_for(node_type),
                )],
                Vec::new(),
                example_id,
            ),
            example_id,
        ),
    }
}

fn harvest_workflow_node_type_tags() -> Vec<&'static str> {
    struct Collector<'a> {
        tags: &'a mut Option<&'static [&'static str]>,
    }

    impl<'de> Deserializer<'de> for Collector<'de> {
        type Error = HarvestError;

        fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(HarvestError)
        }

        fn deserialize_enum<V>(
            self,
            _name: &'static str,
            variants: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            *self.tags = Some(variants);
            Err(HarvestError)
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct identifier ignored_any
        }
    }

    #[derive(Debug)]
    struct HarvestError;

    impl StdError for HarvestError {}

    impl Display for HarvestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "harvest")
        }
    }

    impl de::Error for HarvestError {
        fn custom<T: Display>(_msg: T) -> Self {
            HarvestError
        }
    }

    let mut tags = None;
    let collector = Collector { tags: &mut tags };
    let _ = WorkflowNodeType::deserialize(collector);
    tags.expect("WorkflowNodeType deserialize_enum harvest")
        .to_vec()
}

fn harvest_node_kind_tags() -> Vec<&'static str> {
    #[derive(Debug)]
    struct ExpectedCapture(Option<Vec<&'static str>>);

    impl StdError for ExpectedCapture {}

    impl Display for ExpectedCapture {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "probe")
        }
    }

    impl de::Error for ExpectedCapture {
        fn custom<T: Display>(_msg: T) -> Self {
            ExpectedCapture(None)
        }

        fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
            ExpectedCapture(Some(expected.to_vec()))
        }
    }

    let probe = [("type", "__catalog_probe__")];
    let de = MapDeserializer::new(probe.into_iter());
    let result: Result<NodeKind, ExpectedCapture> = NodeKind::deserialize(de);
    match result {
        Err(ExpectedCapture(Some(expected))) => expected,
        other => panic!("NodeKind harvest probe failed: {:?}", other),
    }
}

fn catalog_wire_tags() -> Vec<&'static str> {
    CATALOG_ORDER.iter().map(|kind| kind.as_str()).collect()
}

fn assert_catalog_length() {
    let catalog_len = CATALOG_ORDER.len();
    let type_len = harvest_workflow_node_type_tags().len();
    let kind_len = harvest_node_kind_tags().len();
    if catalog_len != type_len {
        panic!(
            "catalog length mismatch: CATALOG_ORDER has {catalog_len} entries but WorkflowNodeType harvest has {type_len} variants (this is a length check, not a set-equality check)"
        );
    }
    if catalog_len != kind_len {
        panic!(
            "catalog length mismatch: CATALOG_ORDER has {catalog_len} entries but NodeKind harvest has {kind_len} variants (this is a length check, not a set-equality check)"
        );
    }
    let distinct: BTreeSet<_> = catalog_wire_tags().into_iter().collect();
    if distinct.len() != catalog_len {
        panic!(
            "catalog length mismatch: CATALOG_ORDER has {catalog_len} entries but only {} distinct wire tags (duplicate catalog entry)",
            distinct.len()
        );
    }
}

fn assert_catalog_set_equality() {
    let catalog: BTreeSet<_> = catalog_wire_tags().into_iter().collect();
    let type_tags = harvest_workflow_node_type_tags();
    let kind_tags = harvest_node_kind_tags();

    for tag in &type_tags {
        if !catalog.contains(tag) {
            panic!(
                "wire tag \"{tag}\" is present in the WorkflowNodeType harvest but absent from CATALOG_ORDER"
            );
        }
    }
    for tag in &kind_tags {
        if !catalog.contains(tag) {
            panic!(
                "wire tag \"{tag}\" is present in the NodeKind harvest but absent from CATALOG_ORDER"
            );
        }
    }
    for tag in &catalog {
        if !type_tags.contains(tag) {
            panic!(
                "wire tag \"{tag}\" is present in CATALOG_ORDER but absent from the WorkflowNodeType harvest"
            );
        }
    }
    for tag in &catalog {
        if !kind_tags.contains(tag) {
            panic!(
                "wire tag \"{tag}\" is present in CATALOG_ORDER but absent from the NodeKind harvest"
            );
        }
    }
}

fn assert_catalog_round_trips() {
    for &node_type in CATALOG_ORDER {
        let converted = NodeKind::from(node_type).node_type();
        if converted != node_type {
            panic!(
                "From<WorkflowNodeType> conversion mismatch: keyed to {} but node_type() returned {}",
                node_type.as_str(),
                converted.as_str()
            );
        }
        let kind = example_for(node_type);
        let actual = kind.node_type();
        if actual != node_type {
            panic!(
                "example_for round-trip mismatch: keyed to {} but node_type() returned {}",
                node_type.as_str(),
                actual.as_str()
            );
        }
    }
}

fn assert_workflow_crosses_document_boundary(node_type: WorkflowNodeType, workflow: &WorkflowV3) {
    let serialized = serde_json::to_value(workflow).expect("serialize example workflow");
    let deserialized: WorkflowV3 =
        serde_json::from_value(serialized).expect("strict deserialize example workflow");
    let defaulted = ensure_defaults(deserialized);
    let result = validate_workflow(defaulted);

    let errors: Vec<_> = result
        .issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .collect();
    assert!(
        errors.is_empty(),
        "example workflow for {} validation has errors: {:?}",
        node_type.as_str(),
        errors
    );

    let ignored_warnings: Vec<_> = result
        .issues
        .iter()
        .filter(|issue| {
            issue.severity == "warning"
                && issue
                    .message
                    .contains("so task execution fields are ignored")
        })
        .collect();
    assert!(
        ignored_warnings.is_empty(),
        "example workflow for {} triggers ignored-task-field warnings: {:?}",
        node_type.as_str(),
        ignored_warnings
    );
}

fn format_json_block(value: &Value) -> String {
    let pretty = serde_json::to_string_pretty(value).expect("pretty-print JSON");
    format!("```json\n{pretty}\n```")
}

fn block_span(markdown: &str, block_id: &str) -> (usize, usize) {
    let begin = format!("<!-- BEGIN GENERATED: {block_id} -->");
    let end = format!("<!-- END GENERATED: {block_id} -->");
    let start = markdown
        .find(&begin)
        .unwrap_or_else(|| panic!("missing begin marker for {block_id}"));
    let content_start = start + begin.len();
    let content_end = markdown[content_start..]
        .find(&end)
        .map(|offset| content_start + offset)
        .unwrap_or_else(|| panic!("missing end marker for {block_id}"));
    (content_start, content_end)
}

fn replace_generated_block(markdown: &str, block_id: &str, body: &str) -> String {
    let (content_start, content_end) = block_span(markdown, block_id);
    let mut out = String::new();
    out.push_str(&markdown[..content_start]);
    if body.is_empty() {
        out.push('\n');
    } else {
        out.push('\n');
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(&markdown[content_end..]);
    out
}

fn expected_node_catalog_marker_ids() -> BTreeSet<String> {
    let mut expected = BTreeSet::new();
    for &node_type in CATALOG_ORDER {
        let wire_tag = node_type.as_str();
        expected.insert(format!("node-catalog:{wire_tag}:fragment"));
        expected.insert(format!("node-catalog:{wire_tag}:workflow"));
    }
    expected
}

struct NodeCatalogMarkerInventory {
    openers: Vec<String>,
    closers: Vec<String>,
}

fn inventory_node_catalog_marker_ids(markdown: &str) -> NodeCatalogMarkerInventory {
    let begin_prefix = "<!-- BEGIN GENERATED: ";
    let end_prefix = "<!-- END GENERATED: ";
    let mut openers = Vec::new();
    let mut closers = Vec::new();
    for line in markdown.lines() {
        for (prefix, inventory) in [(begin_prefix, &mut openers), (end_prefix, &mut closers)] {
            let Some(rest) = line.strip_prefix(prefix) else {
                continue;
            };
            let Some(block_id) = rest.strip_suffix(" -->") else {
                continue;
            };
            if block_id.starts_with("node-catalog:") {
                inventory.push(block_id.to_string());
            }
        }
    }
    NodeCatalogMarkerInventory { openers, closers }
}

fn assert_node_catalog_marker_inventory(markdown: &str) {
    let expected = expected_node_catalog_marker_ids();
    let found = inventory_node_catalog_marker_ids(markdown);
    let opener_set: BTreeSet<_> = found.openers.iter().cloned().collect();
    let closer_set: BTreeSet<_> = found.closers.iter().cloned().collect();
    if found.openers.len() != opener_set.len() {
        panic!("duplicate node-catalog marker ids in docs/workflow-schema.md");
    }
    if found.closers.len() != closer_set.len() {
        panic!("duplicate node-catalog closing marker ids in docs/workflow-schema.md");
    }
    for block_id in &opener_set {
        if !expected.contains(block_id) {
            panic!("unexpected node-catalog marker id \"{block_id}\" in docs/workflow-schema.md");
        }
    }
    for block_id in &closer_set {
        if !expected.contains(block_id) {
            panic!(
                "unexpected node-catalog closing marker id \"{block_id}\" in docs/workflow-schema.md"
            );
        }
    }
    for block_id in &expected {
        if !opener_set.contains(block_id) {
            panic!(
                "wire tag marker \"{block_id}\" is expected in docs/workflow-schema.md but absent from the document"
            );
        }
        if !closer_set.contains(block_id) {
            panic!(
                "wire tag closing marker \"{block_id}\" is expected in docs/workflow-schema.md but absent from the document"
            );
        }
    }
}

fn apply_catalog_to_markdown(markdown: &str) -> String {
    assert_node_catalog_marker_inventory(markdown);
    let mut updated = markdown.to_string();
    for &node_type in CATALOG_ORDER {
        let wire_tag = node_type.as_str();
        let (workflow, example_id) = example_workflow(node_type);
        let workflow_value = serde_json::to_value(&workflow).expect("serialize catalog workflow");
        let example_node = workflow
            .nodes
            .iter()
            .find(|node| node.id == example_id)
            .unwrap_or_else(|| panic!("missing example node {example_id} for {wire_tag}"));
        let fragment_value =
            serde_json::to_value(example_node).expect("serialize catalog node fragment");

        updated = replace_generated_block(
            &updated,
            &format!("node-catalog:{wire_tag}:fragment"),
            &format_json_block(&fragment_value),
        );
        updated = replace_generated_block(
            &updated,
            &format!("node-catalog:{wire_tag}:workflow"),
            &format_json_block(&workflow_value),
        );
    }
    updated
}

fn write_catalog(path: &Path, markdown: &str) {
    fs::write(path, markdown).expect("write regenerated workflow schema");
}

fn regenerate_catalog_at(path: &Path, write_to_disk: bool) {
    assert_catalog_set_equality();
    assert_catalog_round_trips();
    assert_catalog_length();

    for &node_type in CATALOG_ORDER {
        let (workflow, _) = example_workflow(node_type);
        assert_workflow_crosses_document_boundary(node_type, &workflow);
    }

    let committed = fs::read_to_string(path).expect("read workflow schema");
    let regenerated = apply_catalog_to_markdown(&committed);

    if write_to_disk {
        write_catalog(path, &regenerated);
        return;
    }

    assert_eq!(
        regenerated, committed,
        "node catalog is stale; run `just regen-docs` to refresh docs/workflow-schema.md"
    );
}

fn perturb_block(markdown: &str, block_id: &str) -> String {
    let (content_start, content_end) = block_span(markdown, block_id);
    let block_body = markdown[content_start..content_end].trim_end();
    let lines: Vec<&str> = block_body.lines().collect();
    if lines.len() < 2 {
        panic!("block {block_id} too short to perturb");
    }
    let truncated = lines[..lines.len() - 1].join("\n");
    let mut out = String::new();
    out.push_str(&markdown[..content_start]);
    out.push('\n');
    out.push_str(&truncated);
    out.push('\n');
    out.push_str(&markdown[content_end..]);
    out
}

fn assert_justfile_names_regen_env() {
    let justfile = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("justfile"))
        .expect("read justfile");
    let expected_command = format!(
        "{REGEN_ENV}=1 cargo test --locked --test docs_catalog regeneration_writes_to_disk -- --ignored --exact"
    );
    assert!(
        justfile.lines().any(|line| line.trim() == expected_command),
        "P3 broke: justfile regen-docs must bind {REGEN_ENV} to the guarded writer entry point"
    );
}

mod workflow_schema_citations {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{OnceLock, RwLock};

    const WORKFLOW_SCHEMA_DOC: &str = "docs/workflow-schema.md";

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Citation {
        raw: String,
        file: String,
        symbols: Vec<String>,
        discriminant: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum CitationProblem {
        Unparseable {
            raw: String,
        },
        MissingFile {
            file: String,
            raw: String,
        },
        MissingSymbol {
            symbol: String,
            raw: String,
        },
        TautologicalDiscriminant {
            discriminant: String,
            raw: String,
        },
        MissingDiscriminant {
            discriminant: String,
            symbol: String,
            raw: String,
        },
        DiscriminantOutsideSymbol {
            discriminant: String,
            symbol: String,
            raw: String,
        },
        FileWithoutSymbol {
            file: String,
            raw: String,
        },
    }

    impl CitationProblem {
        fn message(&self) -> String {
            match self {
                Self::Unparseable { raw } => format!("unparseable citation: {raw}"),
                Self::MissingFile { file, raw } => {
                    format!("missing cited file `{file}` in citation {raw}")
                }
                Self::MissingSymbol { symbol, raw } => {
                    format!("unresolved symbol {symbol} in citation {raw}")
                }
                Self::TautologicalDiscriminant { discriminant, raw } => {
                    format!("discriminant `{discriminant}` repeats a cited symbol ({raw})")
                }
                Self::MissingDiscriminant {
                    discriminant,
                    symbol,
                    raw,
                } => format!("discriminant `{discriminant}` not found in symbol {symbol} ({raw})"),
                Self::DiscriminantOutsideSymbol {
                    discriminant,
                    symbol,
                    raw,
                } => format!("discriminant `{discriminant}` outside symbol {symbol} ({raw})"),
                Self::FileWithoutSymbol { file, raw } => {
                    format!("citation names file `{file}` without a symbol ({raw})")
                }
            }
        }
    }

    #[derive(Clone)]
    struct SymbolSpan {
        name: String,
        start: usize,
        end: usize,
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn strip_generated_blocks(markdown: &str) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < markdown.len() {
            if markdown[i..].starts_with("<!-- BEGIN GENERATED:") {
                let end = markdown[i..]
                    .find("<!-- END GENERATED:")
                    .map(|offset| i + offset)
                    .and_then(|start| markdown[start..].find("-->").map(|close| start + close + 3));
                if let Some(end) = end {
                    let newline_count = markdown[i..end].bytes().filter(|byte| *byte == b'\n').count();
                    for _ in 0..newline_count {
                        out.push('\n');
                    }
                    i = end;
                    continue;
                }
            }
            let ch = markdown[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn read_backtick_token(markdown: &str, start: usize) -> Option<(String, usize)> {
        if !markdown[start..].starts_with('`') {
            return None;
        }
        let mut i = start + 1;
        let mut token = String::new();
        while i < markdown.len() {
            if markdown[i..].starts_with('`') {
                return Some((token, i + 1));
            }
            let ch = markdown[i..].chars().next().unwrap();
            token.push(ch);
            i += ch.len_utf8();
        }
        None
    }

    fn is_source_file(token: &str) -> bool {
        token.starts_with("src/") && token.ends_with(".rs")
    }

    fn parse_backtick_list(raw: &str) -> Option<Vec<String>> {
        let mut rest = raw.trim();
        let mut tokens = Vec::new();
        loop {
            let (token, next) = read_backtick_token(rest, 0)?;
            if token.is_empty() {
                return None;
            }
            tokens.push(token);
            rest = rest[next..].trim_start();
            if rest.is_empty() {
                return Some(tokens);
            }
            rest = rest.strip_prefix(',')?.trim_start();
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum CitationCandidate {
        Parenthesized {
            anchor: usize,
            paren_text: String,
            external_symbol: Option<String>,
        },
        TableCell {
            anchor: usize,
            cell_start: usize,
            cell_end: usize,
            cell_text: String,
        },
        InForm {
            anchor: usize,
            form_text: String,
            symbol: String,
            file: String,
        },
        UnclaimedSource {
            anchor: usize,
            line_text: String,
        },
    }

    fn is_source_candidate(token: &str) -> bool {
        token.starts_with("src/") && token.contains(".rs")
    }

    fn source_mentions(markdown: &str) -> Vec<(usize, usize, String)> {
        let mut mentions = Vec::new();
        let mut i = 0;
        while i < markdown.len() {
            if markdown.as_bytes()[i] == b'`' {
                if let Some((token, end)) = read_backtick_token(markdown, i) {
                    if is_source_candidate(&token) {
                        mentions.push((i, end, token));
                    }
                    i = end;
                    continue;
                }
            }
            let ch = markdown[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
        mentions
    }

    fn matching_paren(markdown: &str, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        let mut i = open;
        while i < markdown.len() {
            if markdown.as_bytes()[i] == b'`' {
                if let Some((_, end)) = read_backtick_token(markdown, i) {
                    i = end;
                    continue;
                }
            }
            match markdown.as_bytes()[i] {
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    struct LineIndex {
        offsets: Vec<usize>,
    }

    impl LineIndex {
        fn new(markdown: &str) -> Self {
            let line_count = markdown.bytes().filter(|byte| *byte == b'\n').count() + 1;
            Self {
                offsets: line_offsets(markdown, line_count),
            }
        }

        fn position_at(&self, pos: usize) -> (usize, usize) {
            let line_idx = self.offsets.partition_point(|offset| *offset <= pos) - 1;
            let line_start = self.offsets[line_idx];
            (line_idx + 1, pos - line_start + 1)
        }
    }

    fn located_raw_at(lines: &LineIndex, pos: usize, slice: &str) -> String {
        let (line, column) = lines.position_at(pos);
        format!("line {line} col {column}: {slice}")
    }

    fn enclosing_line(markdown: &str, pos: usize) -> String {
        let line_start = markdown[..pos].rfind('\n').map_or(0, |index| index + 1);
        let line_end = markdown[pos..]
            .find('\n')
            .map_or(markdown.len(), |index| pos + index);
        markdown[line_start..line_end].trim().to_string()
    }

    fn parenthesized_raw_slice(external_symbol: &Option<String>, paren_text: &str) -> String {
        match external_symbol {
            Some(symbol) => format!("`{symbol}` {paren_text}"),
            None => paren_text.to_string(),
        }
    }

    fn split_table_cell_ranges(line: &str, line_start: usize) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let bytes = line.as_bytes();
        let mut cell_start = 0usize;
        let mut index = 0usize;
        while index < bytes.len() {
            if bytes[index] == b'|' {
                let escaped = index > 0 && bytes[index - 1] == b'\\';
                if !escaped {
                    if cell_start < index {
                        ranges.push((line_start + cell_start, line_start + index));
                    }
                    cell_start = index + 1;
                }
            }
            index += 1;
        }
        if cell_start < line.len() {
            ranges.push((line_start + cell_start, line_start + line.len()));
        }
        ranges
    }

    fn adjacent_preceding_symbol_bounds(markdown: &str, open: usize) -> Option<(usize, usize, String)> {
        let prefix = &markdown[..open];
        let trimmed = prefix.trim_end_matches(char::is_whitespace);
        if !trimmed.ends_with('`') {
            return None;
        }
        let closing_backtick = trimmed.len() - 1;
        let opening_backtick = trimmed[..closing_backtick].rfind('`')?;
        let symbol = trimmed[opening_backtick + 1..closing_backtick].to_string();
        if symbol.is_empty() || symbol.contains('\n') || is_source_candidate(&symbol) {
            return None;
        }
        let symbol_open = opening_backtick;
        let symbol_end = closing_backtick + 1;
        Some((symbol_open, symbol_end, symbol))
    }

    fn adjacent_preceding_symbol(markdown: &str, open: usize) -> Option<String> {
        adjacent_preceding_symbol_bounds(markdown, open)
            .map(|(_, _, symbol)| symbol)
    }

    fn mention_is_explicit_prose(markdown: &str, start: usize, end: usize) -> bool {
        let line_start = markdown[..start].rfind('\n').map_or(0, |pos| pos + 1);
        let line_end = markdown[end..]
            .find('\n')
            .map_or(markdown.len(), |pos| end + pos);
        let before_source = &markdown[line_start..start];
        let before = before_source.trim();
        let after = markdown[end..line_end].trim();
        let prose_in_form = before_source.ends_with(" in ")
            && !before_source[..before_source.len() - 4].ends_with('`');
        let opens_prose_sentence = before.is_empty()
            && after
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase());
        opens_prose_sentence || prose_in_form
    }

    fn discover_citation_candidates(markdown: &str) -> Vec<CitationCandidate> {
        let mentions = source_mentions(markdown);
        let mut claimed = vec![false; mentions.len()];
        let mut candidates = Vec::new();

        let mut parenthesized = Vec::new();
        let mut i = 0;
        while i < markdown.len() {
            if markdown.as_bytes()[i] == b'(' {
                if let Some(close) = matching_paren(markdown, i) {
                    parenthesized.push((i, close));
                }
            }
            let ch = markdown[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
        parenthesized.sort_by_key(|(open, close)| close - open);
        for (open, close) in parenthesized {
            let contained: Vec<usize> = mentions
                .iter()
                .enumerate()
                .filter_map(|(index, (start, end, _))| {
                    (!claimed[index] && open < *start && *end <= close).then_some(index)
                })
                .collect();
            if !contained.is_empty() {
                for index in contained {
                    claimed[index] = true;
                }
                candidates.push(CitationCandidate::Parenthesized {
                    anchor: open,
                    paren_text: markdown[open..=close].to_string(),
                    external_symbol: adjacent_preceding_symbol(markdown, open),
                });
            }
        }

        for (index, (start, end, file)) in mentions.iter().enumerate() {
            if claimed[index] || !is_source_file(file) || !markdown[..*start].ends_with(" in ") {
                continue;
            }
            let in_start = start - 4;
            let before = &markdown[..in_start];
            if !before.ends_with('`') {
                continue;
            }
            let symbol_end = before.len() - 1;
            let Some(symbol_open) = before[..symbol_end].rfind('`') else {
                continue;
            };
            let symbol = &before[symbol_open + 1..symbol_end];
            if symbol.is_empty() || symbol.contains('\n') || is_source_candidate(symbol) {
                continue;
            }
            claimed[index] = true;
            candidates.push(CitationCandidate::InForm {
                anchor: symbol_open,
                form_text: markdown[symbol_open..*end].to_string(),
                symbol: symbol.to_string(),
                file: file.clone(),
            });
        }

        let mut line_start = 0usize;
        for line in markdown.split_inclusive('\n') {
            let line_end = line_start + line.len();
            if line.trim_start().starts_with('|') {
                for (cell_start, cell_end) in split_table_cell_ranges(line, line_start) {
                    let contained: Vec<usize> = mentions
                        .iter()
                        .enumerate()
                        .filter_map(|(index, (start, end, _))| {
                            (!claimed[index] && cell_start <= *start && *end <= cell_end)
                                .then_some(index)
                        })
                        .collect();
                    if !contained.is_empty() {
                        let cell_slice = &markdown[cell_start..cell_end];
                        let anchor = cell_start + cell_slice.len() - cell_slice.trim_start().len();
                        for index in contained {
                            claimed[index] = true;
                        }
                        let cell_text = cell_slice.trim().to_string();
                        candidates.push(CitationCandidate::TableCell {
                            anchor,
                            cell_start,
                            cell_end,
                            cell_text,
                        });
                    }
                }
            }
            line_start = line_end;
        }

        for (index, (start, end, _)) in mentions.iter().enumerate() {
            if !claimed[index] && !mention_is_explicit_prose(markdown, *start, *end) {
                candidates.push(CitationCandidate::UnclaimedSource {
                    anchor: *start,
                    line_text: enclosing_line(markdown, *start),
                });
            }
        }

        candidates
    }

    fn unparseable(raw: String) -> Citation {
        Citation {
            raw,
            file: String::new(),
            symbols: Vec::new(),
            discriminant: None,
        }
    }

    fn parse_token_citation(raw: String, body: &str, external_symbol: Option<String>) -> Citation {
        let Some(tokens) = parse_backtick_list(body) else {
            return unparseable(raw);
        };
        let source_positions: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| is_source_candidate(token).then_some(index))
            .collect();
        if source_positions.len() != 1 {
            return unparseable(raw);
        }
        let file_pos = source_positions[0];
        if !is_source_file(&tokens[file_pos]) || tokens.len() > file_pos + 2 {
            return unparseable(raw);
        }
        let mut symbols = tokens[..file_pos].to_vec();
        if symbols.is_empty() {
            if let Some(symbol) = external_symbol {
                symbols.push(symbol);
            }
        }
        Citation {
            raw,
            file: tokens[file_pos].clone(),
            symbols,
            discriminant: tokens.get(file_pos + 1).cloned(),
        }
    }

    fn parse_citation_candidate(
        markdown: &str,
        candidate: CitationCandidate,
        lines: &LineIndex,
    ) -> Citation {
        match candidate {
            CitationCandidate::Parenthesized {
                anchor,
                paren_text,
                external_symbol,
            } => {
                let body = paren_text[1..paren_text.len() - 1].to_string();
                let report_anchor = if external_symbol.is_some() {
                    adjacent_preceding_symbol_bounds(markdown, anchor)
                        .map(|(open, _, _)| open)
                        .unwrap_or(anchor)
                } else {
                    anchor
                };
                let slice = parenthesized_raw_slice(&external_symbol, &paren_text);
                let raw = located_raw_at(lines, report_anchor, &slice);
                parse_token_citation(raw, &body, external_symbol)
            }
            CitationCandidate::TableCell { anchor, cell_text, .. } => {
                let raw = located_raw_at(lines, anchor, &cell_text);
                parse_token_citation(raw, &cell_text, None)
            }
            CitationCandidate::InForm {
                anchor,
                form_text,
                symbol,
                file,
            } => Citation {
                raw: located_raw_at(lines, anchor, &form_text),
                file,
                symbols: vec![symbol],
                discriminant: None,
            },
            CitationCandidate::UnclaimedSource { anchor, line_text } => {
                unparseable(located_raw_at(lines, anchor, &line_text))
            }
        }
    }

    fn extract_citations(markdown: &str) -> Vec<Citation> {
        let markdown = strip_generated_blocks(markdown);
        let lines = LineIndex::new(&markdown);
        discover_citation_candidates(&markdown)
            .into_iter()
            .map(|candidate| parse_citation_candidate(&markdown, candidate, &lines))
            .collect()
    }

    fn brace_extent(lines: &[&str], open_line: usize, open_col: usize) -> usize {
        let mut depth = 0usize;
        let mut in_string = false;
        let mut in_block_comment = false;
        let mut escape = false;
        let mut line_idx = open_line;
        let mut col = open_col;
        while line_idx < lines.len() {
            let line = lines[line_idx];
            while col < line.len() {
                let bytes = line.as_bytes();
                let ch = bytes[col] as char;
                let next = bytes.get(col + 1).copied();
                if in_block_comment {
                    if ch == '*' && next == Some(b'/') {
                        in_block_comment = false;
                        col += 2;
                    } else {
                        col += 1;
                    }
                    continue;
                }
                if in_string {
                    if escape {
                        escape = false;
                    } else if ch == '\\' {
                        escape = true;
                    } else if ch == '"' {
                        in_string = false;
                    }
                    col += 1;
                    continue;
                }
                if ch == '/' && next == Some(b'/') {
                    break;
                }
                if ch == '/' && next == Some(b'*') {
                    in_block_comment = true;
                    col += 2;
                    continue;
                }
                if ch == '"' {
                    in_string = true;
                } else if ch == '\'' {
                    col += 1;
                    if col < line.len()
                        && ((line.as_bytes()[col] as char).is_ascii_alphanumeric()
                            || line.as_bytes()[col] as char == '_')
                    {
                        while col < line.len() {
                            let next = line.as_bytes()[col] as char;
                            if next.is_ascii_alphanumeric() || next == '_' {
                                col += 1;
                            } else {
                                break;
                            }
                        }
                        if line.as_bytes().get(col) == Some(&b'\'') {
                            col += 1;
                        }
                        continue;
                    }
                    while col < line.len() {
                        if line.as_bytes()[col] == b'\\' {
                            col = (col + 2).min(line.len());
                            continue;
                        }
                        if line.as_bytes()[col] == b'\'' {
                            col += 1;
                            break;
                        }
                        col += 1;
                    }
                    continue;
                } else if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return line_idx;
                    }
                }
                col += 1;
            }
            col = 0;
            line_idx += 1;
        }
        lines.len().saturating_sub(1)
    }

    fn strip_visibility(line: &str) -> &str {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("pub ") {
            return rest.trim_start();
        }
        if let Some(rest) = line.strip_prefix("pub(") {
            if let Some(close) = rest.find(')') {
                return rest[close + 1..].trim_start();
            }
        }
        line
    }

    fn named_item(line: &str, keyword: &str) -> Option<String> {
        let rest = strip_visibility(line).strip_prefix(keyword)?.trim_start();
        let name = rest
            .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '<' | '{' | '(' | ':' | '='))
            .next()
            .unwrap_or("");
        (!name.is_empty()).then(|| name.to_string())
    }

    fn function_name(line: &str) -> Option<String> {
        let mut rest = strip_visibility(line);
        loop {
            let stripped = ["async ", "const ", "unsafe "]
                .into_iter()
                .find_map(|prefix| rest.strip_prefix(prefix));
            if let Some(next) = stripped {
                rest = next.trim_start();
            } else {
                break;
            }
        }
        let rest = rest.strip_prefix("fn ")?;
        let name = rest
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .next()
            .unwrap_or("");
        (!name.is_empty()).then(|| name.to_string())
    }

    fn find_open_brace(lines: &[&str], start: usize, limit: usize) -> Option<(usize, usize)> {
        for (line_index, line) in lines.iter().enumerate().take(limit).skip(start) {
            if let Some(column) = line.find('{') {
                return Some((line_index, column));
            }
            if line.contains(';') {
                return None;
            }
        }
        None
    }

    fn parse_function_item(lines: &[&str], start: usize, limit: usize) -> Option<(String, usize)> {
        let name = function_name(lines.get(start)?.trim())?;
        let (body_line, body_col) = find_open_brace(lines, start, limit)?;
        Some((name, brace_extent(lines, body_line, body_col)))
    }

    fn brace_delta(line: &str) -> isize {
        let mut delta = 0isize;
        let mut in_string = false;
        let mut escape = false;
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let ch = bytes[i] as char;
            if in_string {
                if escape {
                    escape = false;
                } else if ch == '\\' {
                    escape = true;
                } else if ch == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if ch == '/' && bytes.get(i + 1) == Some(&b'/') {
                break;
            }
            if ch == '"' {
                in_string = true;
            } else if ch == '{' {
                delta += 1;
            } else if ch == '}' {
                delta -= 1;
            }
            i += 1;
        }
        delta
    }

    fn cfg_test_item_end(lines: &[&str], cfg_line: usize) -> usize {
        let mut item_start = cfg_line + 1;
        while item_start < lines.len()
            && (lines[item_start].trim().is_empty()
                || lines[item_start].trim_start().starts_with("#["))
        {
            item_start += 1;
        }
        if item_start >= lines.len() {
            return lines.len();
        }
        if let Some((open_line, open_col)) = find_open_brace(lines, item_start, lines.len()) {
            return brace_extent(lines, open_line, open_col) + 1;
        }
        item_start + 1
    }

    fn cfg_test_line_ranges(lines: &[&str]) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut i = 0usize;
        while i < lines.len() {
            if lines[i].trim() == "#[cfg(test)]" {
                let end = cfg_test_item_end(lines, i);
                ranges.push((i, end));
                i = end;
            } else {
                i += 1;
            }
        }
        ranges
    }

    fn line_offsets(text: &str, line_count: usize) -> Vec<usize> {
        let mut offsets = Vec::with_capacity(line_count + 1);
        offsets.push(0);
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                offsets.push(index + 1);
            }
        }
        while offsets.len() <= line_count {
            offsets.push(text.len());
        }
        offsets
    }

    fn symbol_span(
        name: String,
        start_line: usize,
        end_line: usize,
        offsets: &[usize],
    ) -> SymbolSpan {
        SymbolSpan {
            name,
            start: offsets[start_line],
            end: offsets[end_line.min(offsets.len() - 1)],
        }
    }

    fn parse_field_name(line: &str) -> Option<String> {
        let rest = strip_visibility(line);
        let (name, _) = rest.split_once(':')?;
        let name = name.trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return None;
        }
        Some(name.to_string())
    }

    fn impl_type_name(line: &str) -> Option<String> {
        let header = line.trim().strip_prefix("impl ")?.trim();
        let target = header.split(" for ").last()?.trim();
        let name = target
            .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '<' | '{'))
            .next()
            .unwrap_or("");
        (!name.is_empty()).then(|| name.to_string())
    }

    fn index_rust_source(text: &str) -> (Vec<SymbolSpan>, Vec<(usize, usize)>) {
        let lines: Vec<&str> = text.lines().collect();
        let offsets = line_offsets(text, lines.len());
        let cfg_test_lines = cfg_test_line_ranges(&lines);
        let mut spans = Vec::new();
        let mut i = 0usize;
        let mut test_range_index = 0usize;
        while i < lines.len() {
            if let Some((start, end)) = cfg_test_lines.get(test_range_index).copied() {
                if i == start {
                    i = end;
                    test_range_index += 1;
                    continue;
                }
            }
            let stripped = lines[i].trim();
            if let Some(name) = named_item(stripped, "struct ") {
                if let Some((open_line, open_col)) = find_open_brace(&lines, i, lines.len()) {
                    let end = brace_extent(&lines, open_line, open_col);
                    spans.push(symbol_span(name.clone(), i, end + 1, &offsets));
                    let mut depth = 1isize;
                    let mut attribute_depth = 0isize;
                    for j in open_line + 1..end {
                        let field_line = lines[j].trim();
                        if field_line.starts_with("#[") || attribute_depth > 0 {
                            attribute_depth += field_line.matches('[').count() as isize;
                            attribute_depth -= field_line.matches(']').count() as isize;
                        } else if depth == 1 {
                            if let Some(field) = parse_field_name(field_line) {
                                spans.push(symbol_span(
                                    format!("{name}::{field}"),
                                    j,
                                    j + 1,
                                    &offsets,
                                ));
                            }
                        }
                        depth += brace_delta(lines[j]);
                    }
                    i = end + 1;
                    continue;
                }
            }
            if let Some(name) = named_item(stripped, "enum ") {
                if let Some((open_line, open_col)) = find_open_brace(&lines, i, lines.len()) {
                    let end = brace_extent(&lines, open_line, open_col);
                    spans.push(symbol_span(name.clone(), i, end + 1, &offsets));
                    let mut depth = 1isize;
                    let mut attribute_depth = 0isize;
                    for j in open_line + 1..end {
                        let variant_line = lines[j].trim();
                        if variant_line.starts_with("#[") || attribute_depth > 0 {
                            attribute_depth += variant_line.matches('[').count() as isize;
                            attribute_depth -= variant_line.matches(']').count() as isize;
                        } else if depth == 1
                            && !variant_line.is_empty()
                            && !variant_line.starts_with("//")
                        {
                            let variant = variant_line
                                .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                                .next()
                                .unwrap_or("");
                            if !variant.is_empty() {
                                let variant_end = if let Some(column) = lines[j].find('{') {
                                    brace_extent(&lines, j, column) + 1
                                } else {
                                    j + 1
                                };
                                spans.push(symbol_span(
                                    format!("{name}::{variant}"),
                                    j,
                                    variant_end,
                                    &offsets,
                                ));
                            }
                        }
                        depth += brace_delta(lines[j]);
                    }
                    i = end + 1;
                    continue;
                }
            }
            let item = strip_visibility(stripped);
            if let Some(rest) = item
                .strip_prefix("const ")
                .or_else(|| item.strip_prefix("static "))
            {
                let name = rest
                    .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, ':' | '='))
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    spans.push(symbol_span(name.to_string(), i, i + 1, &offsets));
                }
                i += 1;
                continue;
            }
            if let Some(type_name) = impl_type_name(stripped) {
                if let Some((open_line, open_col)) = find_open_brace(&lines, i, lines.len()) {
                    let end = brace_extent(&lines, open_line, open_col);
                    let mut j = open_line + 1;
                    while j < end {
                        if let Some((method, method_end)) = parse_function_item(&lines, j, end) {
                            spans.push(symbol_span(
                                format!("{type_name}::{method}"),
                                j,
                                method_end + 1,
                                &offsets,
                            ));
                            j = method_end + 1;
                        } else {
                            j += 1;
                        }
                    }
                    i = end + 1;
                    continue;
                }
            }
            if let Some((name, end)) = parse_function_item(&lines, i, lines.len()) {
                spans.push(symbol_span(name, i, end + 1, &offsets));
                i = end + 1;
                continue;
            }
            i += 1;
        }
        let test_ranges = cfg_test_lines
            .into_iter()
            .map(|(start, end)| (offsets[start], offsets[end.min(offsets.len() - 1)]))
            .collect();
        (spans, test_ranges)
    }

    struct SourceEntry {
        text: String,
        spans: Vec<SymbolSpan>,
        test_ranges: Vec<(usize, usize)>,
    }

    fn symbol_extent_text(entry: &SourceEntry, span: SymbolSpan) -> &str {
        entry.text.get(span.start..span.end).unwrap_or("")
    }

    fn production_source_contains(entry: &SourceEntry, needle: &str) -> bool {
        let mut cursor = 0usize;
        for (start, end) in &entry.test_ranges {
            if entry.text[cursor..*start].contains(needle) {
                return true;
            }
            cursor = *end;
        }
        entry.text[cursor..].contains(needle)
    }

    fn resolve_symbol(spans: &[SymbolSpan], symbol: &str) -> Option<SymbolSpan> {
        spans.iter().find(|span| span.name == symbol).cloned()
    }

    fn ensure_source_cached(cache: &mut HashMap<String, SourceEntry>, file: &str) {
        if cache.contains_key(file) {
            return;
        }
        let path = repo_root().join(file);
        if !path.is_file() {
            return;
        }
        let text = fs::read_to_string(&path).expect("read cited source file");
        let (spans, test_ranges) = index_rust_source(&text);
        cache.insert(
            file.to_string(),
            SourceEntry {
                text,
                spans,
                test_ranges,
            },
        );
    }

    fn check_citation(
        citation: &Citation,
        source_cache: &HashMap<String, SourceEntry>,
    ) -> Vec<CitationProblem> {
        let mut problems = Vec::new();
        if citation.file.is_empty() {
            problems.push(CitationProblem::Unparseable {
                raw: citation.raw.clone(),
            });
            return problems;
        }
        if citation.symbols.is_empty() {
            problems.push(CitationProblem::FileWithoutSymbol {
                file: citation.file.clone(),
                raw: citation.raw.clone(),
            });
            return problems;
        }

        let path = repo_root().join(&citation.file);
        if !path.is_file() {
            problems.push(CitationProblem::MissingFile {
                file: citation.file.clone(),
                raw: citation.raw.clone(),
            });
            return problems;
        }

        let entry = match source_cache.get(&citation.file) {
            Some(entry) => entry,
            None => {
                problems.push(CitationProblem::MissingFile {
                    file: citation.file.clone(),
                    raw: citation.raw.clone(),
                });
                return problems;
            }
        };

        for symbol in &citation.symbols {
            if resolve_symbol(&entry.spans, symbol).is_none() {
                problems.push(CitationProblem::MissingSymbol {
                    symbol: symbol.clone(),
                    raw: citation.raw.clone(),
                });
            }
        }
        if problems
            .iter()
            .any(|problem| matches!(problem, CitationProblem::MissingSymbol { .. }))
        {
            return problems;
        }

        if let Some(discriminant) = &citation.discriminant {
            if citation.symbols.contains(discriminant) {
                problems.push(CitationProblem::TautologicalDiscriminant {
                    discriminant: discriminant.clone(),
                    raw: citation.raw.clone(),
                });
                return problems;
            }
            let symbol = citation.symbols.last().expect("symbol checked above");
            let span = resolve_symbol(&entry.spans, symbol).expect("symbol exists");
            let extent = symbol_extent_text(entry, span);
            if extent.contains(discriminant) {
                return problems;
            }
            if production_source_contains(entry, discriminant) {
                problems.push(CitationProblem::DiscriminantOutsideSymbol {
                    discriminant: discriminant.clone(),
                    symbol: symbol.clone(),
                    raw: citation.raw.clone(),
                });
            } else {
                problems.push(CitationProblem::MissingDiscriminant {
                    discriminant: discriminant.clone(),
                    symbol: symbol.clone(),
                    raw: citation.raw.clone(),
                });
            }
        }

        problems
    }

    fn shared_source_cache() -> &'static RwLock<HashMap<String, SourceEntry>> {
        static CACHE: OnceLock<RwLock<HashMap<String, SourceEntry>>> = OnceLock::new();
        CACHE.get_or_init(|| RwLock::new(HashMap::new()))
    }

    pub fn check_workflow_schema_citations(markdown: &str) -> Vec<String> {
        let citations = extract_citations(markdown);
        {
            let mut source_cache = shared_source_cache()
                .write()
                .expect("workflow schema citation source cache");
            for citation in &citations {
                if !citation.file.is_empty() {
                    ensure_source_cached(&mut source_cache, &citation.file);
                }
            }
        }
        let source_cache = shared_source_cache()
            .read()
            .expect("workflow schema citation source cache");
        let mut problems = Vec::new();
        for citation in citations {
            for problem in check_citation(&citation, &source_cache) {
                problems.push(problem.message());
            }
        }
        problems.sort();
        problems
    }

    pub fn assert_workflow_schema_citations_fresh() {
        let markdown = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema for citation guard");
        let problems = check_workflow_schema_citations(&markdown);
        if !problems.is_empty() {
            panic!(
                "workflow schema citations are stale or broken:\n{}",
                problems.join("\n")
            );
        }
    }

    fn fixture_markdown(perturb: impl FnOnce(String) -> String) -> String {
        let committed = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema");
        perturb(committed)
    }

    fn located_raw_in(markdown: &str, slice: &str) -> String {
        let stripped = strip_generated_blocks(markdown);
        let lines = LineIndex::new(&stripped);
        let pos = stripped
            .rfind(slice)
            .or_else(|| stripped.find(slice))
            .unwrap_or_else(|| panic!("expected markdown to contain citation slice: {slice}"));
        located_raw_at(&lines, pos, slice)
    }

    fn located_raw_after(markdown: &str, context: &str, slice: &str) -> String {
        let stripped = strip_generated_blocks(markdown);
        let lines = LineIndex::new(&stripped);
        let context_pos = stripped
            .find(context)
            .unwrap_or_else(|| panic!("expected markdown to contain context: {context}"));
        let slice_pos = stripped[context_pos..]
            .find(slice)
            .map(|offset| context_pos + offset)
            .unwrap_or_else(|| panic!("expected context to contain citation slice: {slice}"));
        located_raw_at(&lines, slice_pos, slice)
    }

    fn citation_body_matches_tokens(body: &str, tokens: &[&str]) -> bool {
        let Some(parsed) = parse_backtick_list(body) else {
            return false;
        };
        parsed.len() == tokens.len()
            && parsed
                .iter()
                .zip(tokens.iter())
                .all(|(parsed, expected)| parsed == *expected)
    }

    fn find_citation_with_tokens(text: &str, tokens: &[&str]) -> Option<(usize, usize)> {
        let mut search_from = 0usize;
        while search_from < text.len() {
            let Some(relative_open) = text[search_from..].find('(') else {
                break;
            };
            let open = search_from + relative_open;
            let Some(close) = matching_paren(text, open) else {
                search_from = open + 1;
                continue;
            };
            if citation_body_matches_tokens(&text[open + 1..close], tokens) {
                return Some((open, close));
            }
            search_from = open + 1;
        }
        None
    }

    fn replace_citation_tokens(text: &str, from: &[&str], to: &[&str]) -> Option<String> {
        assert_eq!(from.len(), to.len());
        let (open, close) = find_citation_with_tokens(text, from)?;
        let new_body = to
            .iter()
            .map(|token| format!("`{token}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..open]);
        out.push('(');
        out.push_str(&new_body);
        out.push(')');
        out.push_str(&text[close + 1..]);
        Some(out)
    }

    fn located_parenthesized_raw(
        markdown: &str,
        context: &str,
        external_symbol: Option<&str>,
        paren_text: &str,
    ) -> String {
        let stripped = strip_generated_blocks(markdown);
        let lines = LineIndex::new(&stripped);
        let context_pos = stripped
            .find(context)
            .unwrap_or_else(|| panic!("expected markdown to contain context: {context}"));
        let paren_pos = stripped[context_pos..]
            .find(paren_text)
            .map(|offset| context_pos + offset)
            .unwrap_or_else(|| panic!("expected context to contain paren text: {paren_text}"));
        let anchor = if external_symbol.is_some() {
            adjacent_preceding_symbol_bounds(&stripped, paren_pos)
                .map(|(open, _, _)| open)
                .unwrap_or(paren_pos)
        } else {
            paren_pos
        };
        let slice = parenthesized_raw_slice(
            &external_symbol.map(str::to_string),
            paren_text,
        );
        located_raw_at(&lines, anchor, &slice)
    }

    fn assert_problems(markdown: &str, expected: &[&str]) {
        let expected = expected
            .iter()
            .map(|problem| (*problem).to_string())
            .collect::<Vec<_>>();
        assert_eq!(check_workflow_schema_citations(markdown), expected);
    }

    #[test]
    fn workflow_schema_citations_resolve() {
        assert_workflow_schema_citations_fresh();
    }

    #[test]
    fn citation_guard_reports_missing_symbol_in_validation_source_cell() {
        let markdown = fixture_markdown(|text| {
            text.replace(
                "`validate_graph_body`, `src/model.rs`, `Duplicate node id` |",
                "`validate_graph_bodyQQQ`, `src/model.rs`, `Duplicate node id` |",
            )
        });
        let raw = extract_citations(&markdown)
            .into_iter()
            .find(|citation| citation.symbols.iter().any(|symbol| symbol == "validate_graph_bodyQQQ"))
            .map(|citation| citation.raw)
            .expect("corrupted validation catalog citation");
        assert_eq!(
            check_workflow_schema_citations(&markdown),
            vec![format!(
                "unresolved symbol validate_graph_bodyQQQ in citation {raw}"
            )]
        );
    }

    #[test]
    fn citation_guard_reports_missing_symbol_in_source_line() {
        let markdown = fixture_markdown(|text| {
            text.replace(
                "*Source: `WorkflowNode` (`WorkflowNode`, `src/model.rs`).*",
                "*Source: `WorkflowNode` (`WorkflowNodeQQQ`, `src/model.rs`).*",
            )
        });
        let context = "*Source: `WorkflowNode` (`WorkflowNodeQQQ`, `src/model.rs`).*";
        let citation = "(`WorkflowNodeQQQ`, `src/model.rs`)";
        assert_eq!(
            check_workflow_schema_citations(&markdown),
            vec![format!(
                "unresolved symbol WorkflowNodeQQQ in citation {}",
                located_parenthesized_raw(&markdown, context, Some("WorkflowNode"), citation)
            )]
        );
    }

    #[test]
    fn citation_guard_reports_bad_citation_separator() {
        let markdown = fixture_markdown(|text| {
            format!("{text}\nMalformed citation (`validate_graph_body` and `src/model.rs`).\n")
        });
        let citation = "(`validate_graph_body` and `src/model.rs`)";
        assert_eq!(
            check_workflow_schema_citations(&markdown),
            vec![format!("unparseable citation: {}", located_raw_in(&markdown, citation))]
        );
    }

    #[test]
    fn citation_guard_reports_trailing_citation_tokens() {
        let markdown = fixture_markdown(|text| {
            format!(
                "{text}\nMalformed citation (`validate_graph_body`, `src/model.rs`, `Duplicate node id`, `trailing`).\n"
            )
        });
        let citation =
            "(`validate_graph_body`, `src/model.rs`, `Duplicate node id`, `trailing`)";
        assert_problems(
            &markdown,
            &[&format!(
                "unparseable citation: {}",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_does_not_borrow_an_unrelated_preceding_symbol() {
        let markdown = fixture_markdown(|text| {
            format!("{text}\n`validate_graph_body` is unrelated prose before (`src/model.rs`).\n")
        });
        let context = "`validate_graph_body` is unrelated prose before (`src/model.rs`).";
        let citation = "(`src/model.rs`)";
        assert_problems(
            &markdown,
            &[&format!(
                "citation names file `src/model.rs` without a symbol ({})",
                located_raw_after(&markdown, context, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_does_not_parse_prose_between_symbol_and_file() {
        let markdown = fixture_markdown(|text| {
            format!(
                "{text}\n`validate_graph_body` in the migration path described in `src/model.rs`.\n"
            )
        });
        assert_problems(&markdown, &[]);
    }

    #[test]
    fn citation_discovery_claims_every_source_mention() {
        let markdown = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema");
        let markdown = strip_generated_blocks(&markdown);
        let mentions = source_mentions(&markdown);
        assert_eq!(
            mentions.len(),
            307,
            "source mention count changed; update discovery expectations if intentional"
        );
        let unclaimed = discover_citation_candidates(&markdown)
            .into_iter()
            .filter(|candidate| matches!(candidate, CitationCandidate::UnclaimedSource { .. }))
            .count();
        assert_eq!(unclaimed, 0);
    }

    #[test]
    fn citation_discovery_classifies_expected_prose_mentions() {
        let markdown = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema");
        let markdown = strip_generated_blocks(&markdown);
        let mentions = source_mentions(&markdown);
        let prose_count = mentions
            .iter()
            .filter(|(start, end, _)| mention_is_explicit_prose(&markdown, *start, *end))
            .count();
        assert_eq!(prose_count, 1);
    }

    /// P2: distinct citation sites must not share a located raw identifier, so the same
    /// perturbation class cannot produce byte-identical diagnostics for two sites.
    #[test]
    fn citation_raw_identifiers_are_unique() {
        let markdown = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema");
        let markdown = strip_generated_blocks(&markdown);
        let raws = extract_citations(&markdown)
            .into_iter()
            .map(|citation| citation.raw)
            .collect::<Vec<_>>();
        let unique = raws.iter().collect::<BTreeSet<_>>();
        assert_eq!(
            unique.len(),
            raws.len(),
            "distinct citation sites must not share a located raw identifier"
        );
    }

    #[test]
    fn citation_reported_lines_match_committed_file() {
        fn committed_line_of(committed: &str, needle: &str) -> usize {
            let pos = committed
                .find(needle)
                .unwrap_or_else(|| panic!("committed file missing citation needle: {needle}"));
            committed[..pos].bytes().filter(|byte| *byte == b'\n').count() + 1
        }

        fn committed_column_of(committed: &str, needle: &str) -> usize {
            let pos = committed
                .find(needle)
                .unwrap_or_else(|| panic!("committed file missing citation needle: {needle}"));
            let line_start = committed[..pos].rfind('\n').map_or(0, |index| index + 1);
            pos - line_start + 1
        }

        fn reported_line_from_problems(problems: &[String]) -> usize {
            let diagnostic = problems
                .first()
                .expect("expected at least one citation problem");
            let after_line = diagnostic
                .split("line ")
                .nth(1)
                .expect("diagnostic must include line number");
            after_line
                .split_whitespace()
                .next()
                .expect("line number token")
                .parse()
                .expect("numeric line number")
        }

        fn reported_column_from_problems(problems: &[String]) -> usize {
            let diagnostic = problems
                .first()
                .expect("expected at least one citation problem");
            let after_col = diagnostic
                .split("col ")
                .nth(1)
                .expect("diagnostic must include column number");
            after_col
                .split_whitespace()
                .next()
                .expect("column number token")
                .trim_end_matches(':')
                .parse()
                .expect("numeric column number")
        }

        fn committed_column_of_on_line(
            committed: &str,
            line_needle: &str,
            column_needle: &str,
        ) -> usize {
            let line_pos = committed
                .find(line_needle)
                .unwrap_or_else(|| panic!("committed file missing line needle: {line_needle}"));
            let line_start = committed[..line_pos].rfind('\n').map_or(0, |index| index + 1);
            let line_end = committed[line_start..]
                .find('\n')
                .map_or(committed.len(), |index| line_start + index);
            let line = &committed[line_start..line_end];
            let column_pos = line
                .find(column_needle)
                .unwrap_or_else(|| panic!("line missing column needle: {column_needle}"));
            column_pos + 1
        }

        let committed = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema");

        let above_needle = "(`WorkflowNodeType::as_str`, `src/model.rs`)";
        let above_expected = committed_line_of(&committed, above_needle);
        let above_markdown = fixture_markdown(|text| {
            replace_citation_tokens(
                &text,
                &["WorkflowNodeType::as_str", "src/model.rs"],
                &["WorkflowNodeType::as_strZZZ", "src/model.rs"],
            )
            .expect("citation above generated block")
        });
        let above_problems = check_workflow_schema_citations(&above_markdown);
        assert_eq!(
            reported_line_from_problems(&above_problems),
            above_expected,
            "line number must match committed file above generated blocks"
        );

        let below_needle = "(`WorkflowNode::split_failure_policy`, `src/model.rs`)";
        let below_expected = committed_line_of(&committed, below_needle);
        let below_markdown = fixture_markdown(|text| {
            replace_citation_tokens(
                &text,
                &["WorkflowNode::split_failure_policy", "src/model.rs"],
                &["WorkflowNode::split_failure_policyZZZ", "src/model.rs"],
            )
            .expect("citation below generated block")
        });
        let below_problems = check_workflow_schema_citations(&below_markdown);
        assert_eq!(
            reported_line_from_problems(&below_problems),
            below_expected,
            "line number must match committed file below generated blocks"
        );

        let external_symbol_line_needle =
            "*Source: `WorkflowNode` (`WorkflowNode`, `src/model.rs`).*";
        let external_symbol_column_needle =
            "`WorkflowNode` (`WorkflowNode`, `src/model.rs`)";
        let external_symbol_expected_line =
            committed_line_of(&committed, external_symbol_line_needle);
        let external_symbol_expected_column =
            committed_column_of(&committed, external_symbol_column_needle);
        let external_symbol_markdown = fixture_markdown(|text| {
            text.replace(
                "*Source: `WorkflowNode` (`WorkflowNode`, `src/model.rs`).*",
                "*Source: `WorkflowNode` (`WorkflowNodeQQQ`, `src/model.rs`).*",
            )
        });
        let external_symbol_problems = check_workflow_schema_citations(&external_symbol_markdown);
        assert_eq!(
            reported_line_from_problems(&external_symbol_problems),
            external_symbol_expected_line,
            "externally-symbolled citation line must match committed file"
        );
        assert_eq!(
            reported_column_from_problems(&external_symbol_problems),
            external_symbol_expected_column,
            "externally-symbolled citation column must match committed file"
        );

        let table_cell_line_needle = "| error | Duplicate node id | `validate_graph_body`, `src/model.rs`, `Duplicate node id` |";
        let table_cell_column_needle =
            "`validate_graph_body`, `src/model.rs`, `Duplicate node id`";
        let table_cell_expected_line = committed_line_of(&committed, table_cell_line_needle);
        let table_cell_expected_column = committed_column_of_on_line(
            &committed,
            table_cell_line_needle,
            table_cell_column_needle,
        );
        let table_cell_markdown = fixture_markdown(|text| {
            text.replace(
                "`validate_graph_body`, `src/model.rs`, `Duplicate node id` |",
                "`validate_graph_bodyQQQ`, `src/model.rs`, `Duplicate node id` |",
            )
        });
        let table_cell_problems = check_workflow_schema_citations(&table_cell_markdown);
        assert_eq!(
            reported_line_from_problems(&table_cell_problems),
            table_cell_expected_line,
            "table-cell citation line must match committed file"
        );
        assert_eq!(
            reported_column_from_problems(&table_cell_problems),
            table_cell_expected_column,
            "table-cell citation column must match committed file"
        );
    }

    #[test]
    fn validation_catalog_rows_have_distinct_branch_discriminants() {
        let markdown = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema");
        let catalog = markdown
            .split_once("## Validation catalog")
            .expect("validation catalog heading")
            .1
            .split_once("## Regenerating this document")
            .expect("regeneration heading after validation catalog")
            .0;
        let mut identities = BTreeSet::new();
        let mut row_count = 0usize;
        for line in catalog
            .lines()
            .filter(|line| line.starts_with("| error |") || line.starts_with("| warning |"))
        {
            row_count += 1;
            let source = line
                .rsplit('|')
                .nth(1)
                .expect("validation row source cell")
                .trim();
            let tokens = parse_backtick_list(source)
                .unwrap_or_else(|| panic!("validation Source cell is not a citation: {source}"));
            let file_pos = tokens
                .iter()
                .position(|token| is_source_file(token))
                .unwrap_or_else(|| panic!("validation Source cell has no source file: {source}"));
            assert_eq!(
                tokens.len(),
                file_pos + 2,
                "validation Source cell needs exactly one branch discriminant: {source}"
            );
            let identity = format!(
                "{} :: {}",
                tokens[..file_pos].join(", "),
                tokens[file_pos + 1]
            );
            assert!(
                identities.insert(identity.clone()),
                "validation Source cells reuse branch identity {identity}"
            );
        }
        assert_eq!(
            row_count, 100,
            "validation catalog row count changed unexpectedly"
        );
    }

    #[test]
    fn citation_guard_rejects_fake_enum_variants_from_attributes_and_fields() {
        let markdown = fixture_markdown(|text| {
            format!(
                "{text}\nFake variants (`NodeKind::default`, `NodeKind::rename`, `NodeKind::skip_serializing_if`, `NodeKind::decide_config`, `NodeKind::batch_config`, `NodeKind::subflow_config`, `src/model.rs`).\n"
            )
        });
        let citation = "(`NodeKind::default`, `NodeKind::rename`, `NodeKind::skip_serializing_if`, `NodeKind::decide_config`, `NodeKind::batch_config`, `NodeKind::subflow_config`, `src/model.rs`)";
        let located = located_raw_in(&markdown, citation);
        assert_problems(
            &markdown,
            &[
                &format!("unresolved symbol NodeKind::batch_config in citation {located}"),
                &format!("unresolved symbol NodeKind::decide_config in citation {located}"),
                &format!("unresolved symbol NodeKind::default in citation {located}"),
                &format!("unresolved symbol NodeKind::rename in citation {located}"),
                &format!("unresolved symbol NodeKind::skip_serializing_if in citation {located}"),
                &format!("unresolved symbol NodeKind::subflow_config in citation {located}"),
            ],
        );
    }

    #[test]
    fn citation_guard_resolves_pub_crate_functions_and_static_items() {
        let markdown = fixture_markdown(|text| {
            format!(
                "{text}\nPrivate functions (`max_node_retry_attempts`, `validate_workflow_input_bounds`, `src/model.rs`). Private static (`PUBLIC_DIR`, `src/frontend.rs`).\n"
            )
        });
        assert_problems(&markdown, &[]);
    }

    #[test]
    fn citation_guard_rejects_test_only_symbols() {
        let markdown = fixture_markdown(|text| {
            format!(
                "{text}\nTest-only symbol (`compute_graph_metadata_includes_parallel_batch_body_entry`, `src/model.rs`).\n"
            )
        });
        let citation = "(`compute_graph_metadata_includes_parallel_batch_body_entry`, `src/model.rs`)";
        assert_problems(
            &markdown,
            &[&format!(
                "unresolved symbol compute_graph_metadata_includes_parallel_batch_body_entry in citation {}",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_excludes_test_text_from_discriminant_classification() {
        let markdown = fixture_markdown(|text| {
            format!(
                "{text}\nTest-only text (`compute_graph_metadata`, `src/model.rs`, `compute_graph_metadata_includes_parallel_batch_body_entry`).\n"
            )
        });
        let citation = "(`compute_graph_metadata`, `src/model.rs`, `compute_graph_metadata_includes_parallel_batch_body_entry`)";
        assert_problems(
            &markdown,
            &[&format!(
                "discriminant `compute_graph_metadata_includes_parallel_batch_body_entry` not found in symbol compute_graph_metadata ({})",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn rust_index_brace_extent_ignores_line_comment_braces() {
        let lines = ["fn cited() {", "    // }", "    let value = 1;", "}"];
        assert_eq!(brace_extent(&lines, 0, 11), 3);
    }

    #[test]
    fn citation_guard_reports_missing_symbol() {
        let markdown = fixture_markdown(|text| {
            text.replace(
                "(`validate_graph_body`, `src/model.rs`, `Duplicate edge id`)",
                "(`validate_graph_bodyZZZ`, `src/model.rs`, `Duplicate edge id`)",
            )
        });
        let citation = "(`validate_graph_bodyZZZ`, `src/model.rs`, `Duplicate edge id`)";
        assert_problems(
            &markdown,
            &[&format!(
                "unresolved symbol validate_graph_bodyZZZ in citation {}",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_reports_missing_discriminant() {
        let markdown = fixture_markdown(|text| {
            text.replace(
                "(`validate_graph_body`, `src/model.rs`, `Duplicate edge id`)",
                "(`validate_graph_body`, `src/model.rs`, `ZZZ-not-in-source-discriminant`)",
            )
        });
        let citation = "(`validate_graph_body`, `src/model.rs`, `ZZZ-not-in-source-discriminant`)";
        assert_problems(
            &markdown,
            &[&format!(
                "discriminant `ZZZ-not-in-source-discriminant` not found in symbol validate_graph_body ({})",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_reports_discriminant_outside_symbol_extent() {
        let markdown = fixture_markdown(|text| {
            text.replacen(
                "(`validate_graph_body`, `src/model.rs`, `Duplicate edge id`)",
                "(`validate_graph_body`, `src/model.rs`, `workflow version is required`)",
                1,
            )
        });
        let citation = "(`validate_graph_body`, `src/model.rs`, `workflow version is required`)";
        assert_problems(
            &markdown,
            &[&format!(
                "discriminant `workflow version is required` outside symbol validate_graph_body ({})",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_reports_enum_variant_and_multi_symbol_citations() {
        let committed = fs::read_to_string(repo_root().join(WORKFLOW_SCHEMA_DOC))
            .expect("read workflow schema");
        let normalized = committed.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains("(`NodeKind::Approval`, `src/model.rs`)"),
            "fixture requires an enum-variant citation in the committed document"
        );
        assert!(
            committed.contains(
                "(`MAX_WORKFLOW_SUBFLOWS`, `MAX_SUBFLOW_CALL_EDGES`, `MAX_WORKFLOW_NODES`"
            ),
            "fixture requires a multi-symbol citation in the committed document"
        );

        let enum_variant = fixture_markdown(|text| {
            replace_citation_tokens(
                &text,
                &["NodeKind::Approval", "src/model.rs"],
                &["NodeKind::ApprovalZZZ", "src/model.rs"],
            )
            .expect("fixture requires an enum-variant citation in the committed document")
        });
        let enum_citation = "(`NodeKind::ApprovalZZZ`, `src/model.rs`)";
        assert_problems(
            &enum_variant,
            &[&format!(
                "unresolved symbol NodeKind::ApprovalZZZ in citation {}",
                located_raw_in(&enum_variant, enum_citation)
            )],
        );

        let multi_symbol = fixture_markdown(|text| {
            replace_citation_tokens(
                &text,
                &[
                    "MAX_WORKFLOW_SUBFLOWS",
                    "MAX_SUBFLOW_CALL_EDGES",
                    "MAX_WORKFLOW_NODES",
                    "MAX_WORKFLOW_EDGES",
                    "MAX_NODE_RETRY_COUNT",
                    "src/model.rs",
                ],
                &[
                    "MAX_WORKFLOW_SUBFLOWS",
                    "MAX_SUBFLOW_CALL_EDGES_ZZZ",
                    "MAX_WORKFLOW_NODES",
                    "MAX_WORKFLOW_EDGES",
                    "MAX_NODE_RETRY_COUNT",
                    "src/model.rs",
                ],
            )
            .expect("fixture requires a multi-symbol citation in the committed document")
        });
        let multi_citation = "(`MAX_WORKFLOW_SUBFLOWS`, `MAX_SUBFLOW_CALL_EDGES_ZZZ`, `MAX_WORKFLOW_NODES`, `MAX_WORKFLOW_EDGES`, `MAX_NODE_RETRY_COUNT`, `src/model.rs`)";
        assert_problems(
            &multi_symbol,
            &[&format!(
                "unresolved symbol MAX_SUBFLOW_CALL_EDGES_ZZZ in citation {}",
                located_raw_in(&multi_symbol, multi_citation)
            )],
        );
    }

    #[test]
    fn citation_guard_reports_unparseable_citations() {
        let markdown = fixture_markdown(|text| {
            format!("{text}\n\nMalformed citation (`src/model.rs:123`).\n")
        });
        let citation = "(`src/model.rs:123`)";
        assert_problems(
            &markdown,
            &[&format!(
                "unparseable citation: {}",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_reports_file_without_symbol() {
        let resolvable = fixture_markdown(|text| text);
        assert!(
            resolvable.contains("`normalize_workflow_value` (`src/model.rs`)"),
            "fixture requires an immediate position-free citation"
        );
        assert_problems(&resolvable, &[]);

        let markdown = fixture_markdown(|text| {
            text.replacen(
                "`normalize_workflow_value` (`src/model.rs`)",
                "(`src/model.rs`)",
                1,
            )
        });
        let context = "Ingest runs (`src/model.rs`), which calls";
        let citation = "(`src/model.rs`)";
        assert_problems(
            &markdown,
            &[&format!(
                "citation names file `src/model.rs` without a symbol ({})",
                located_raw_after(&markdown, context, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_reports_missing_file_distinctly() {
        let markdown = fixture_markdown(|text| {
            format!("{text}\nMissing file (`validate_graph_body`, `src/not-a-real-file.rs`).\n")
        });
        let citation = "(`validate_graph_body`, `src/not-a-real-file.rs`)";
        assert_problems(
            &markdown,
            &[&format!(
                "missing cited file `src/not-a-real-file.rs` in citation {}",
                located_raw_in(&markdown, citation)
            )],
        );
    }

    #[test]
    fn citation_guard_rejects_symbol_name_as_discriminant() {
        let markdown = fixture_markdown(|text| {
            format!(
                "{text}\nTautological citation (`validate_graph_body`, `src/model.rs`, `validate_graph_body`).\n"
            )
        });
        let citation = "(`validate_graph_body`, `src/model.rs`, `validate_graph_body`)";
        assert_problems(
            &markdown,
            &[&format!(
                "discriminant `validate_graph_body` repeats a cited symbol ({})",
                located_raw_in(&markdown, citation)
            )],
        );
    }
}

#[test]
fn node_catalog_generator() {
    regenerate_catalog_at(&docs_path(), false);
}

#[test]
fn regeneration_repairs_perturbed_block() {
    let committed = fs::read_to_string(docs_path()).expect("read workflow schema");
    let perturbed = perturb_block(&committed, "node-catalog:task:workflow");
    let once = apply_catalog_to_markdown(&perturbed);
    let twice = apply_catalog_to_markdown(&once);
    assert_eq!(
        once, twice,
        "catalog regeneration must be idempotent on perturbed markdown"
    );
    assert_eq!(once, committed);
}

#[test]
fn regen_recipe_is_bound_to_guarded_writer() {
    assert_justfile_names_regen_env();
}

#[test]
fn explicit_write_repairs_copied_tree_after_validation() {
    let committed = fs::read_to_string(docs_path()).expect("read workflow schema");
    let expected = apply_catalog_to_markdown(&committed);
    let perturbed = perturb_block(&committed, "node-catalog:task:workflow");
    let temp_dir = tempfile::tempdir().expect("temp directory");
    let copy_path = temp_dir.path().join("workflow-schema.md");
    fs::write(&copy_path, &perturbed).expect("write perturbed workflow schema");

    regenerate_catalog_at(&copy_path, true);

    assert_eq!(
        fs::read_to_string(&copy_path).expect("read repaired workflow schema"),
        expected,
        "P3/P5 broke: the explicit write control did not validate and repair the copied tree"
    );
}

#[test]
#[should_panic(expected = "unexpected node-catalog closing marker id")]
fn marker_inventory_rejects_unexpected_closing_marker() {
    let committed = fs::read_to_string(docs_path()).expect("read workflow schema");
    let malformed = format!("{committed}\n<!-- END GENERATED: node-catalog:retired:fragment -->\n");

    assert_node_catalog_marker_inventory(&malformed);
}

#[test]
fn freshness_check_is_compare_only_by_construction() {
    let committed = fs::read_to_string(docs_path()).expect("read workflow schema");
    let perturbed = perturb_block(&committed, "node-catalog:task:workflow");
    let temp_dir = tempfile::tempdir().expect("temp directory");
    let copy_path = temp_dir.path().join("workflow-schema.md");
    fs::write(&copy_path, &perturbed).expect("write perturbed workflow schema");

    let comparison = std::panic::catch_unwind(|| regenerate_catalog_at(&copy_path, false));
    let payload = comparison
        .expect_err("P2 broke: the freshness check must compare stale markdown, not write it");
    let panic_message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("P2 broke: freshness check must panic with a string message");
    assert!(
        panic_message.contains("node catalog is stale"),
        "P2 broke: freshness check must panic with the stale-catalog assertion, got: {panic_message}"
    );
    assert_eq!(
        fs::read_to_string(&copy_path).expect("read compared workflow schema"),
        perturbed,
        "P1 broke: a non-writer test changed workflow-schema.md under ambient regeneration state"
    );
}

#[test]
#[ignore = "writes docs/workflow-schema.md when SB_REGEN_DOCS=1"]
fn regeneration_writes_to_disk() {
    if std::env::var(REGEN_ENV).as_deref() != Ok("1") {
        return;
    }

    regenerate_catalog_at(&docs_path(), true);
}
