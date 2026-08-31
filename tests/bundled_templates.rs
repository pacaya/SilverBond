//! Guard tests for bundled workflow templates under `templates/`.
//!
//! ISSUE-260830-1925-01 — reject branch edges that depend on absent routing (no condition,
//! non-decide source) so bundled templates cannot ship silently unroutable paths.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use silverbond::model::{
    NodeKind, WorkflowEdgeOutcome, WorkflowNodeType, WorkflowV3, normalize_workflow_value,
};

const UNROUTABLE_PHRASE: &str = "unroutable branch edge";

fn templates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates")
}

fn enumerate_template_files() -> Vec<PathBuf> {
    let entries: Vec<_> = fs::read_dir(templates_dir())
        .expect("read templates directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("read templates directory entries");
    let mut files: Vec<PathBuf> = entries
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension() == Some("json".as_ref()))
        .collect();
    files.sort();
    files
}

fn load_template_workflow(path: &Path) -> WorkflowV3 {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read template {}: {err}", path.display()));
    let value: Value =
        serde_json::from_str(&raw).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));
    normalize_workflow_value(value)
        .unwrap_or_else(|err| panic!("normalize {}: {err}", path.display()))
        .workflow
}

fn node_types_by_id(workflow: &WorkflowV3) -> HashMap<&str, WorkflowNodeType> {
    workflow
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.node_type()))
        .collect()
}

/// Returns diagnostics for branch edges with no condition whose source is not a `decide` node.
fn unroutable_branch_edge_diagnostics(
    workflow: &WorkflowV3,
    location: &str,
) -> Vec<String> {
    let node_types = node_types_by_id(workflow);
    let mut diagnostics = Vec::new();

    for edge in &workflow.edges {
        if edge.outcome != WorkflowEdgeOutcome::Branch || edge.condition.is_some() {
            continue;
        }
        let Some(source_type) = node_types.get(edge.from.as_str()) else {
            diagnostics.push(format!(
                "{location}: {UNROUTABLE_PHRASE} \"{}\" from node \"{}\" (source node not found)",
                edge.id,
                edge.from,
            ));
            continue;
        };
        if *source_type == WorkflowNodeType::Decide {
            continue;
        }
        let source_suffix = format!("(type {})", source_type.as_str());
        diagnostics.push(format!(
            "{location}: {UNROUTABLE_PHRASE} \"{}\" from node \"{}\" {source_suffix}",
            edge.id,
            edge.from,
        ));
    }

    diagnostics
}

fn collect_unroutable_branch_edges(workflow: &WorkflowV3, location: &str) -> Vec<String> {
    let mut diagnostics = unroutable_branch_edge_diagnostics(workflow, location);
    for (subflow_name, subflow) in &workflow.subflows {
        let subflow_location = format!("{location} subflow \"{subflow_name}\"");
        diagnostics.extend(unroutable_branch_edge_diagnostics(subflow, &subflow_location));
    }
    diagnostics
}

fn assert_no_unroutable_branch_edges(workflow: &WorkflowV3, location: &str) {
    let diagnostics = collect_unroutable_branch_edges(workflow, location);
    assert!(
        diagnostics.is_empty(),
        "bundled template has unroutable branch edges:\n{}",
        diagnostics.join("\n")
    );
}

fn first_decide_branch_edge(body: &WorkflowV3) -> Option<String> {
    let node_types = node_types_by_id(body);
    body.edges.iter().find_map(|edge| {
        if edge.outcome != WorkflowEdgeOutcome::Branch || edge.condition.is_some() {
            return None;
        }
        let source_is_decide = node_types.get(edge.from.as_str()) == Some(&WorkflowNodeType::Decide);
        source_is_decide.then(|| edge.id.clone())
    })
}

fn first_non_decide_node_id(workflow: &WorkflowV3) -> String {
    workflow
        .nodes
        .iter()
        .find(|node| node.node_type() != WorkflowNodeType::Decide)
        .map(|node| node.id.clone())
        .expect("workflow must contain a non-decide node for witness construction")
}

fn first_subflow_branch_edge_from_decide(workflow: &WorkflowV3) -> Option<(String, String)> {
    for (subflow_name, subflow) in &workflow.subflows {
        if let Some(edge_id) = first_decide_branch_edge(subflow) {
            return Some((subflow_name.clone(), edge_id));
        }
    }
    None
}

fn repoint_edge(body: &mut WorkflowV3, edge_id: &str, new_source: &str) {
    let edge = body
        .edges
        .iter_mut()
        .find(|edge| edge.id == edge_id)
        .unwrap_or_else(|| panic!("witness edge {edge_id} not found"));
    edge.from = new_source.to_string();
}

fn witness_subflow_by_repointing_branch_edge_source(
    workflow: &mut WorkflowV3,
    subflow_name: &str,
    edge_id: &str,
    new_source: &str,
) {
    let subflow = workflow
        .subflows
        .get_mut(subflow_name)
        .unwrap_or_else(|| panic!("witness subflow {subflow_name} not found"));
    repoint_edge(subflow, edge_id, new_source);
}

fn assert_single_unroutable_diagnostic(
    diagnostics: &[String],
    edge_id: &str,
    source_id: &str,
) {
    assert_eq!(diagnostics.len(), 1, "expected one diagnostic, got: {diagnostics:?}");
    assert!(
        diagnostics[0].contains("unroutable branch edge"),
        "diagnostic must contain unroutable branch edge: {}",
        diagnostics[0]
    );
    assert!(
        diagnostics[0].contains(edge_id),
        "diagnostic must name the offending edge: {}",
        diagnostics[0]
    );
    assert!(
        diagnostics[0].contains(source_id),
        "diagnostic must name the source node: {}",
        diagnostics[0]
    );
}

#[test]
fn bundled_templates_have_no_unroutable_branch_edges() {
    let template_files = enumerate_template_files();
    assert!(
        !template_files.is_empty(),
        "expected at least one bundled template under templates/"
    );

    for path in template_files {
        let workflow = load_template_workflow(&path);
        assert_no_unroutable_branch_edges(&workflow, &path.display().to_string());
    }
}

#[test]
fn unroutable_branch_edge_checker_flags_top_level_witness() {
    let path = templates_dir().join("epic-dev.json");
    let mut workflow = load_template_workflow(&path);
    let edge_id = first_decide_branch_edge(&workflow)
        .expect("epic-dev.json must expose a decide-sourced branch edge for witness construction");
    let non_decide_source = first_non_decide_node_id(&workflow);
    repoint_edge(&mut workflow, &edge_id, &non_decide_source);

    let diagnostics = collect_unroutable_branch_edges(&workflow, "witness");
    assert_single_unroutable_diagnostic(&diagnostics, &edge_id, &non_decide_source);
}

#[test]
fn unroutable_branch_edge_checker_flags_subflow_catalog_witness() {
    let path = templates_dir().join("multi-agent-plan-implementation.json");
    let mut workflow = load_template_workflow(&path);
    let (subflow_name, edge_id) = first_subflow_branch_edge_from_decide(&workflow).expect(
        "multi-agent-plan-implementation.json must expose a subflow decide-sourced branch edge",
    );
    let subflow = workflow
        .subflows
        .get(&subflow_name)
        .expect("witness subflow must exist");
    let non_decide_source = first_non_decide_node_id(subflow);
    witness_subflow_by_repointing_branch_edge_source(
        &mut workflow,
        &subflow_name,
        &edge_id,
        &non_decide_source,
    );

    let diagnostics = collect_unroutable_branch_edges(&workflow, "witness");
    assert_single_unroutable_diagnostic(&diagnostics, &edge_id, &non_decide_source);
}

fn paths_to_node(workflow: &WorkflowV3, target: &str) -> Vec<HashSet<String>> {
    fn dfs(
        workflow: &WorkflowV3,
        current: &str,
        target: &str,
        visited: &mut HashSet<String>,
        paths: &mut Vec<HashSet<String>>,
    ) {
        visited.insert(current.to_string());
        if current == target {
            paths.push(visited.clone());
            visited.remove(current);
            return;
        }
        for edge in workflow
            .edges
            .iter()
            .filter(|edge| edge.from == current)
        {
            dfs(workflow, &edge.to, target, visited, paths);
        }
        visited.remove(current);
    }

    let mut visited = HashSet::new();
    let mut paths = Vec::new();
    dfs(
        workflow,
        &workflow.entry_node_id,
        target,
        &mut visited,
        &mut paths,
    );
    paths
}

fn expand_execution_sets_from(
    workflow: &WorkflowV3,
    mut executed: HashSet<String>,
    current: &str,
    results: &mut Vec<HashSet<String>>,
) {
    results.push(executed.clone());
    for edge in workflow
        .edges
        .iter()
        .filter(|edge| edge.from == current)
    {
        match edge.outcome {
            WorkflowEdgeOutcome::Success | WorkflowEdgeOutcome::LoopExit => {
                executed.insert(edge.to.clone());
                expand_execution_sets_from(workflow, executed.clone(), &edge.to, results);
            }
            WorkflowEdgeOutcome::LoopContinue => {
                if !executed.contains(&edge.to) {
                    executed.insert(edge.to.clone());
                    expand_execution_sets_from(workflow, executed.clone(), &edge.to, results);
                }
            }
            WorkflowEdgeOutcome::Branch | WorkflowEdgeOutcome::Reject => {}
        }
    }
}

fn execution_sets_for_decide_outcome(
    workflow: &WorkflowV3,
    decide_id: &str,
    outcome_label: &str,
) -> Vec<HashSet<String>> {
    let branch_target = workflow
        .edges
        .iter()
        .find(|edge| {
            edge.from == decide_id
                && edge.outcome == WorkflowEdgeOutcome::Branch
                && edge.label.as_deref() == Some(outcome_label)
        })
        .map(|edge| edge.to.clone())
        .unwrap_or_else(|| {
            panic!(
                "decide node {decide_id} has no branch edge labeled {outcome_label:?}"
            )
        });

    let mut sets = Vec::new();
    for prefix in paths_to_node(workflow, decide_id) {
        let mut executed = prefix;
        executed.insert(decide_id.to_string());
        executed.insert(branch_target.clone());
        expand_execution_sets_from(workflow, executed, &branch_target, &mut sets);
    }
    dedupe_execution_sets(sets)
}

fn dedupe_execution_sets(sets: Vec<HashSet<String>>) -> Vec<HashSet<String>> {
    let mut unique = Vec::new();
    for set in sets {
        if !unique.iter().any(|existing| existing == &set) {
            unique.push(set);
        }
    }
    unique
}

fn context_bindings_by_name(node: &silverbond::model::WorkflowNode) -> HashMap<&str, &str> {
    node.context_sources
        .iter()
        .map(|source| (source.name.as_str(), source.node_id.as_str()))
        .collect()
}

fn context_refs_in_prompt(prompt: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut rest = prompt;
    while let Some(start) = rest.find("{{context:") {
        let after = &rest[start + "{{context:".len()..];
        if let Some(end) = after.find("}}") {
            refs.push(after[..end].to_string());
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    refs
}

fn declared_variable_names(workflow: &WorkflowV3) -> HashSet<String> {
    workflow
        .variables
        .iter()
        .map(|variable| variable.name.clone())
        .collect()
}

fn node_executable_prompt(node: &silverbond::model::WorkflowNode) -> &str {
    match &node.kind {
        NodeKind::Decide { decide_config } => decide_config.prompt.as_str(),
        _ => node.prompt.as_str(),
    }
}

fn simulate_resolved_prompt(
    prompt: &str,
    node: &silverbond::model::WorkflowNode,
    executed: &HashSet<String>,
    declared_vars: &HashSet<String>,
) -> String {
    let mut resolved = prompt.to_string();

    for (name, _) in prompt.split("{{var:").skip(1).filter_map(|segment| {
        segment
            .split("}}")
            .next()
            .map(|name| (name.to_string(), ()))
    }) {
        if declared_vars.contains(&name) {
            resolved = resolved.replace(&format!("{{{{var:{name}}}}}"), "<resolved>");
        }
    }

    for source in &node.context_sources {
        if executed.contains(&source.node_id) {
            resolved = resolved.replace(
                &format!("{{{{context:{}}}}}", source.name),
                "<resolved>",
            );
        }
    }

    if let NodeKind::Decide { decide_config } = &node.kind {
        for input in &decide_config.inputs {
            let source_node = input
                .source
                .strip_prefix("node:")
                .and_then(|rest| rest.strip_suffix(".output"));
            if let Some(node_id) = source_node {
                if executed.contains(node_id) {
                    resolved = resolved.replace(&format!("{{{{{}}}}}", input.name), "<resolved>");
                }
            }
        }
    }

    for node_id in executed {
        resolved = resolved.replace(&format!("{{{{node:{node_id}.output}}}}"), "<resolved>");
        // Matches engine bare output form: {{<node_id>}} (runtime.rs resolve_template_vars).
        resolved = resolved.replace(&format!("{{{{{node_id}}}}}"), "<resolved>");
    }

    for token in [
        "{{previous_output}}",
        "{{all_predecessors}}",
        "{{branch_origin}}",
        "{{branch_choice}}",
    ] {
        resolved = resolved.replace(token, "<resolved>");
    }

    resolved
}

fn has_unresolved_template_tokens(prompt: &str) -> bool {
    prompt.contains("{{") && prompt.contains("}}")
}

fn decide_nodes(workflow: &WorkflowV3) -> Vec<&silverbond::model::WorkflowNode> {
    workflow
        .nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Decide { .. }))
        .collect()
}

fn assert_research_and_summarize_prompt_slots_are_path_safe(workflow: &WorkflowV3) {
    let decide_list = decide_nodes(workflow);
    assert!(
        !decide_list.is_empty(),
        "research-and-summarize.json must contain at least one decide node for path-safety checks"
    );

    let declared_vars = declared_variable_names(workflow);
    let mut asserted_outcome_paths = 0usize;

    for decide_node in decide_list {
        let NodeKind::Decide { decide_config } = &decide_node.kind else {
            continue;
        };
        for outcome in &decide_config.outcomes {
            for executed in execution_sets_for_decide_outcome(workflow, &decide_node.id, outcome) {
                asserted_outcome_paths += 1;
                for node in &workflow.nodes {
                    let prompt = node_executable_prompt(node);
                    if !executed.contains(&node.id) || prompt.is_empty() {
                        continue;
                    }

                    let bindings = context_bindings_by_name(node);
                    for context_name in context_refs_in_prompt(prompt) {
                        let bound_node = bindings.get(context_name.as_str()).unwrap_or_else(|| {
                            panic!(
                                "node {} references {{{{context:{context_name}}}}} without a binding",
                                node.id
                            )
                        });
                        assert!(
                            executed.contains(*bound_node),
                            "node {} binds {{context:{context_name}}} to {bound_node}, \
                             which does not execute on decide outcome {outcome:?} path {executed:?}",
                            node.id
                        );
                    }

                    let resolved = simulate_resolved_prompt(prompt, node, &executed, &declared_vars);
                    assert!(
                        !has_unresolved_template_tokens(&resolved),
                        "node {} on decide outcome {outcome:?} path {executed:?} still has \
                         unresolved template tokens:\n{resolved}",
                        node.id
                    );
                }
            }
        }
    }

    assert!(
        asserted_outcome_paths > 0,
        "path-safety fixture must assert at least one decide outcome execution path"
    );
}

fn path_safety_check_panics(workflow: &WorkflowV3) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_research_and_summarize_prompt_slots_are_path_safe(workflow);
    }))
    .is_err()
}

fn path_safety_check_panic_message(workflow: &WorkflowV3) -> Option<String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_research_and_summarize_prompt_slots_are_path_safe(workflow);
    }))
    .err()
    .and_then(|payload| {
        if let Some(message) = payload.downcast_ref::<&str>() {
            Some(message.to_string())
        } else if let Some(message) = payload.downcast_ref::<String>() {
            Some(message.clone())
        } else {
            None
        }
    })
}

fn mutate_decide_prompt(workflow: &mut WorkflowV3, append: &str) {
    let decide = workflow
        .nodes
        .iter_mut()
        .find(|node| matches!(node.kind, NodeKind::Decide { .. }))
        .expect("witness workflow must contain a decide node");
    let NodeKind::Decide { decide_config } = &mut decide.kind else {
        panic!("witness decide node must have decide kind");
    };
    decide_config.prompt.push_str(append);
}

fn mutate_node_prompt(workflow: &mut WorkflowV3, node_id: &str, append: &str) {
    let node = workflow
        .nodes
        .iter_mut()
        .find(|node| node.id == node_id)
        .unwrap_or_else(|| panic!("witness node {node_id} not found"));
    node.prompt.push_str(append);
}

#[test]
fn research_and_summarize_has_no_conditional_prompt_bindings() {
    let path = templates_dir().join("research-and-summarize.json");
    let workflow = load_template_workflow(&path);
    assert_research_and_summarize_prompt_slots_are_path_safe(&workflow);
}

#[test]
fn path_safety_checker_flags_violating_decide_prompt_context_binding() {
    let path = templates_dir().join("research-and-summarize.json");
    let mut workflow = load_template_workflow(&path);
    mutate_decide_prompt(&mut workflow, " {{context:ghost}}");

    let message = path_safety_check_panic_message(&workflow).expect(
        "expected path-safety checker to reject ghost context refs in decide prompt",
    );
    assert!(
        message.contains("without a binding"),
        "expected P1 context-binding rejection, got: {message}"
    );
}

#[test]
fn path_safety_checker_flags_violating_decide_prompt_residual_tokens() {
    let path = templates_dir().join("research-and-summarize.json");
    let mut workflow = load_template_workflow(&path);
    mutate_decide_prompt(&mut workflow, " {{totally_bogus}}");

    let message = path_safety_check_panic_message(&workflow).expect(
        "expected path-safety checker to reject residual tokens in decide prompt",
    );
    assert!(
        message.contains("unresolved template tokens"),
        "expected P2 residual-token rejection, got: {message}"
    );
}

#[test]
fn path_safety_fixture_requires_decide_node() {
    let path = templates_dir().join("research-and-summarize.json");
    let mut workflow = load_template_workflow(&path);
    workflow
        .nodes
        .retain(|node| !matches!(node.kind, NodeKind::Decide { .. }));

    assert!(
        path_safety_check_panics(&workflow),
        "expected path-safety fixture to fail when the template has no decide node"
    );
}

#[test]
fn path_safety_checker_flags_undeclared_var_tokens() {
    let path = templates_dir().join("research-and-summarize.json");
    let mut workflow = load_template_workflow(&path);
    mutate_node_prompt(&mut workflow, "rs2", " {{var:ghost}}");

    assert!(
        path_safety_check_panics(&workflow),
        "expected path-safety checker to reject undeclared {{var:ghost}} tokens"
    );
}
