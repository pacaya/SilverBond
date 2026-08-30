use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserialize, Deserializer, Visitor, value::MapDeserializer};
use serde_json::Value;
use silverbond::model::{
    BatchConfig, CaptureConfig, DecideConfig, KillConfig, NodeKind, RunAgentConfig, SendConfig,
    SpawnConfig, SubflowConfig, WORKFLOW_SCHEMA_VERSION, WaitConfig, WaitMode, WorkflowEdge,
    WorkflowEdgeOutcome, WorkflowNode, WorkflowNodeType, WorkflowV3, WorkflowVariable,
    ensure_defaults, validate_workflow,
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

    for node in &result.workflow.nodes {
        if matches!(node.kind, NodeKind::Split | NodeKind::Collector)
            && has_task_execution_config(node)
        {
            panic!(
                "example {} node {:?} carries ignored task execution fields",
                node_type.as_str(),
                node.id
            );
        }
    }

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
