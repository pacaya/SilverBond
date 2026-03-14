use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use include_dir::{Dir, include_dir};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::spawn_blocking;

use crate::{
    model::{NormalizedWorkflow, WorkflowV3, normalize_workflow_value},
    runtime::{
        ExecutionLog, InterruptedRunSummary, LogListItem, PersistedRun,
        RuntimeEvent, RuntimeStatus,
    },
    util::{ensure_dir, now_iso, safe_name},
};

#[derive(Debug, Clone)]
pub struct Database {
    path: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WorkflowStore {
    dir: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TemplateStore {
    dir: Arc<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredWorkflowItem {
    pub name: String,
    pub filename: String,
    pub workflow: WorkflowV3,
    #[serde(default)]
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateItem {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub workflow: WorkflowV3,
    #[serde(default)]
    pub notices: Vec<String>,
    pub template_file: String,
}

static BUNDLED_TEMPLATES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

pub fn seed_bundled_templates(dir: &Path) -> anyhow::Result<()> {
    ensure_dir(dir)?;
    for file in BUNDLED_TEMPLATES_DIR.files() {
        let Some(filename) = file.path().file_name() else {
            continue;
        };
        let target = dir.join(filename);
        if target.exists() {
            continue;
        }
        std::fs::write(target, file.contents())?;
    }
    Ok(())
}

impl Database {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Arc::new(path.into()),
        }
    }

    pub fn path(&self) -> &Path {
        self.path.as_ref().as_path()
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        let path = self.path.clone();
        spawn_blocking(move || -> anyhow::Result<()> {
            if let Some(parent) = path.parent() {
                ensure_dir(parent)?;
            }
            let conn = open_connection(path.as_path())?;
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS runs (
                    run_id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    workflow_name TEXT NOT NULL,
                    current_node_id TEXT,
                    current_node_name TEXT,
                    total_executed INTEGER NOT NULL,
                    started_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    pending_approval_json TEXT,
                    state_json TEXT NOT NULL,
                    workflow_json TEXT NOT NULL,
                    terminal_reason TEXT
                );
                CREATE TABLE IF NOT EXISTS run_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id TEXT NOT NULL,
                    event_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_run_events_run_id_id ON run_events(run_id, id);
                CREATE TABLE IF NOT EXISTS logs (
                    id TEXT PRIMARY KEY,
                    filename TEXT NOT NULL,
                    workflow_name TEXT NOT NULL,
                    goal TEXT NOT NULL,
                    start_time TEXT NOT NULL,
                    end_time TEXT,
                    total_duration TEXT NOT NULL,
                    aborted INTEGER NOT NULL,
                    run_id TEXT,
                    data_json TEXT NOT NULL
                );
            "#,
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn upsert_run(&self, persisted: &PersistedRun) -> anyhow::Result<()> {
        let path = self.path.clone();
        let persisted = persisted.clone();
        spawn_blocking(move || -> anyhow::Result<()> {
            let conn = open_connection(path.as_path())?;
            conn.execute(
                r#"
                INSERT INTO runs (
                    run_id, status, workflow_name, current_node_id, current_node_name, total_executed,
                    started_at, updated_at, pending_approval_json, state_json, workflow_json, terminal_reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(run_id) DO UPDATE SET
                    status = excluded.status,
                    workflow_name = excluded.workflow_name,
                    current_node_id = excluded.current_node_id,
                    current_node_name = excluded.current_node_name,
                    total_executed = excluded.total_executed,
                    started_at = excluded.started_at,
                    updated_at = excluded.updated_at,
                    pending_approval_json = excluded.pending_approval_json,
                    state_json = excluded.state_json,
                    workflow_json = excluded.workflow_json,
                    terminal_reason = excluded.terminal_reason
                "#,
                params![
                    persisted.checkpoint.run_id,
                    status_as_str(&persisted.checkpoint.status),
                    persisted.checkpoint.workflow_name,
                    persisted.checkpoint.current_node_id,
                    persisted.checkpoint.current_node_name,
                    persisted.checkpoint.total_executed,
                    persisted.checkpoint.started_at,
                    persisted.checkpoint.updated_at,
                    persisted
                        .checkpoint
                        .pending_approval
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()?,
                    serde_json::to_string(&persisted.checkpoint)?,
                    serde_json::to_string(&persisted.workflow)?,
                    persisted.checkpoint.execution_log.terminal_reason,
                ],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn get_run(&self, run_id: &str) -> anyhow::Result<Option<PersistedRun>> {
        let path = self.path.clone();
        let run_id = run_id.to_string();
        spawn_blocking(move || -> anyhow::Result<Option<PersistedRun>> {
            let conn = open_connection(path.as_path())?;
            let row = conn
                .query_row(
                    "SELECT state_json, workflow_json FROM runs WHERE run_id = ?1",
                    params![run_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let Some((state_json, workflow_json)) = row else {
                return Ok(None);
            };
            Ok(Some(PersistedRun {
                checkpoint: serde_json::from_str(&state_json)?,
                workflow: serde_json::from_str(&workflow_json)?,
            }))
        })
        .await?
    }

    pub async fn mark_run_status(
        &self,
        run_id: &str,
        status: RuntimeStatus,
        terminal_reason: Option<String>,
    ) -> anyhow::Result<()> {
        let path = self.path.clone();
        let run_id = run_id.to_string();
        let status = status_as_str(&status).to_string();
        spawn_blocking(move || -> anyhow::Result<()> {
            let conn = open_connection(path.as_path())?;
            conn.execute(
                "UPDATE runs SET status = ?2, terminal_reason = ?3, updated_at = ?4 WHERE run_id = ?1",
                params![run_id, status, terminal_reason, now_iso()],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn append_event(&self, run_id: &str, event: &RuntimeEvent) -> anyhow::Result<()> {
        let path = self.path.clone();
        let run_id = run_id.to_string();
        let event = event.clone();
        spawn_blocking(move || -> anyhow::Result<()> {
            let conn = open_connection(path.as_path())?;
            conn.execute(
                "INSERT INTO run_events (run_id, event_json, created_at) VALUES (?1, ?2, ?3)",
                params![run_id, serde_json::to_string(&event)?, now_iso()],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn list_events(&self, run_id: &str) -> anyhow::Result<Vec<RuntimeEvent>> {
        let path = self.path.clone();
        let run_id = run_id.to_string();
        spawn_blocking(move || -> anyhow::Result<Vec<RuntimeEvent>> {
            let conn = open_connection(path.as_path())?;
            let mut stmt = conn
                .prepare("SELECT event_json FROM run_events WHERE run_id = ?1 ORDER BY id ASC")?;
            let rows = stmt.query_map(params![run_id], |row| row.get::<_, String>(0))?;
            let mut events = Vec::new();
            for row in rows {
                events.push(serde_json::from_str(&row?)?);
            }
            Ok(events)
        })
        .await?
    }

    pub async fn list_interrupted_runs(&self) -> anyhow::Result<Vec<InterruptedRunSummary>> {
        let path = self.path.clone();
        spawn_blocking(move || -> anyhow::Result<Vec<InterruptedRunSummary>> {
            let conn = open_connection(path.as_path())?;
            let mut stmt = conn.prepare(
                "SELECT run_id, status, workflow_name, current_node_id, current_node_name, \
                 total_executed, started_at, updated_at, pending_approval_json \
                 FROM runs WHERE status IN ('running', 'paused') ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, u32>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })?;
            let mut runs = Vec::new();
            for row in rows {
                let (run_id, status, workflow_name, current_node_id, current_node_name,
                     total_executed, started_at, updated_at, pending_approval_json) = row?;
                let pending_approval = pending_approval_json
                    .map(|json| serde_json::from_str(&json))
                    .transpose()?;
                runs.push(InterruptedRunSummary {
                    run_id,
                    status: status_from_str(&status),
                    workflow_name,
                    current_node_id,
                    current_node_name,
                    total_executed,
                    started_at,
                    updated_at,
                    pending_approval,
                });
            }
            Ok(runs)
        })
        .await?
    }

    pub async fn save_execution_log(&self, id: &str, log: &ExecutionLog) -> anyhow::Result<()> {
        let path = self.path.clone();
        let id = safe_name(id)?;
        let log_value = serde_json::to_value(log)?;
        spawn_blocking(move || -> anyhow::Result<()> {
            let conn = open_connection(path.as_path())?;
            conn.execute(
                r#"
                INSERT INTO logs (
                    id, filename, workflow_name, goal, start_time, end_time, total_duration, aborted, run_id, data_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(id) DO UPDATE SET
                    filename = excluded.filename,
                    workflow_name = excluded.workflow_name,
                    goal = excluded.goal,
                    start_time = excluded.start_time,
                    end_time = excluded.end_time,
                    total_duration = excluded.total_duration,
                    aborted = excluded.aborted,
                    run_id = excluded.run_id,
                    data_json = excluded.data_json
                "#,
                params![
                    id,
                    format!("{}.json", id),
                    value_str(&log_value, "workflowName"),
                    value_str(&log_value, "goal"),
                    value_str(&log_value, "startTime"),
                    log_value.get("endTime").and_then(Value::as_str),
                    value_str(&log_value, "totalDuration"),
                    if log_value.get("aborted").and_then(Value::as_bool).unwrap_or(false) {
                        1
                    } else {
                        0
                    },
                    log_value.get("runId").and_then(Value::as_str),
                    serde_json::to_string(&log_value)?,
                ],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn list_logs(&self) -> anyhow::Result<Vec<LogListItem>> {
        let path = self.path.clone();
        spawn_blocking(move || -> anyhow::Result<Vec<LogListItem>> {
            let conn = open_connection(path.as_path())?;
            let mut stmt = conn.prepare(
                "SELECT id, filename, workflow_name, goal, start_time, end_time, total_duration, aborted, run_id, data_json FROM logs ORDER BY start_time DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?;
            let mut logs = Vec::new();
            for row in rows {
                let (id, filename, workflow_name, goal, start_time, end_time, total_duration, aborted, run_id, data_json) =
                    row?;
                let data_value: Value = serde_json::from_str(&data_json)?;
                let node_execs = data_value
                    .get("nodeExecutions")
                    .and_then(Value::as_array);
                let node_execution_count = node_execs.map(Vec::len).unwrap_or(0);
                let (total_cost, total_in, total_out, succeeded, failed) =
                    if let Some(execs) = node_execs {
                        let mut cost = 0.0f64;
                        let mut has_cost = false;
                        let mut input = 0u64;
                        let mut output = 0u64;
                        let mut has_tokens = false;
                        let mut ok = 0usize;
                        let mut err = 0usize;
                        for e in execs {
                            if let Some(c) = e.get("costUsd").and_then(Value::as_f64) {
                                cost += c;
                                has_cost = true;
                            }
                            if let Some(t) = e.get("inputTokens").and_then(Value::as_u64) {
                                input += t;
                                has_tokens = true;
                            }
                            if let Some(t) = e.get("outputTokens").and_then(Value::as_u64) {
                                output += t;
                                has_tokens = true;
                            }
                            if e.get("success").and_then(Value::as_bool).unwrap_or(false) {
                                ok += 1;
                            } else {
                                err += 1;
                            }
                        }
                        (
                            if has_cost { Some(cost) } else { None },
                            if has_tokens { Some(input) } else { None },
                            if has_tokens { Some(output) } else { None },
                            ok,
                            err,
                        )
                    } else {
                        (None, None, None, 0, 0)
                    };
                logs.push(LogListItem {
                    id,
                    filename,
                    workflow_name,
                    goal,
                    start_time,
                    end_time,
                    total_duration,
                    node_execution_count,
                    decision_count: data_value
                        .get("decisions")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0),
                    aborted: aborted == 1,
                    run_id,
                    total_cost_usd: total_cost,
                    total_input_tokens: total_in,
                    total_output_tokens: total_out,
                    nodes_succeeded: succeeded,
                    nodes_failed: failed,
                });
            }
            Ok(logs)
        })
        .await?
    }

    pub async fn get_log(&self, id: &str) -> anyhow::Result<Option<Value>> {
        let path = self.path.clone();
        let id = safe_name(id)?;
        spawn_blocking(move || -> anyhow::Result<Option<Value>> {
            let conn = open_connection(path.as_path())?;
            let data_json = conn
                .query_row(
                    "SELECT data_json FROM logs WHERE id = ?1",
                    params![id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(data_json
                .map(|value| serde_json::from_str(&value))
                .transpose()?)
        })
        .await?
    }

    pub async fn delete_log(&self, id: &str) -> anyhow::Result<()> {
        let path = self.path.clone();
        let id = safe_name(id)?;
        spawn_blocking(move || -> anyhow::Result<()> {
            let conn = open_connection(path.as_path())?;
            conn.execute("DELETE FROM logs WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await??;
        Ok(())
    }
}

impl WorkflowStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Arc::new(dir.into()),
        }
    }

    pub fn dir(&self) -> &Path {
        self.dir.as_ref().as_path()
    }

    pub async fn list(&self) -> anyhow::Result<Vec<StoredWorkflowItem>> {
        let mut entries = tokio::fs::read_dir(self.dir()).await?;
        let mut workflows = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_name().to_string_lossy().ends_with(".json") {
                continue;
            }
            let filename = entry.file_name().to_string_lossy().to_string();
            let contents = tokio::fs::read_to_string(entry.path()).await?;
            let value: Value = serde_json::from_str(&contents)?;
            let normalized = normalize_workflow_value(value)?;
            workflows.push(StoredWorkflowItem {
                name: filename.trim_end_matches(".json").to_string(),
                filename,
                workflow: normalized.workflow,
                notices: normalized.notices,
            });
        }
        workflows.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(workflows)
    }

    pub async fn get(&self, name: &str) -> anyhow::Result<Option<StoredWorkflowItem>> {
        let safe = safe_name(name)?;
        let path = self.dir().join(format!("{}.json", safe));
        if !path.exists() {
            return Ok(None);
        }
        let contents = tokio::fs::read_to_string(&path).await?;
        let value: Value = serde_json::from_str(&contents)?;
        let normalized = normalize_workflow_value(value)?;
        Ok(Some(StoredWorkflowItem {
            name: safe.clone(),
            filename: format!("{}.json", safe),
            workflow: normalized.workflow,
            notices: normalized.notices,
        }))
    }

    pub async fn save(&self, name: &str, normalized: NormalizedWorkflow) -> anyhow::Result<String> {
        let safe = safe_name(name)?;
        ensure_dir(self.dir())?;
        let path = self.dir().join(format!("{}.json", safe));
        let data = serde_json::to_vec_pretty(&normalized.workflow)?;
        tokio::fs::write(path, data).await?;
        Ok(safe)
    }

    pub async fn delete(&self, name: &str) -> anyhow::Result<()> {
        let safe = safe_name(name)?;
        let path = self.dir().join(format!("{}.json", safe));
        tokio::fs::remove_file(path).await?;
        Ok(())
    }
}

impl TemplateStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Arc::new(dir.into()),
        }
    }

    pub fn dir(&self) -> &Path {
        self.dir.as_ref().as_path()
    }

    pub async fn list(&self) -> anyhow::Result<Vec<TemplateItem>> {
        if !self.dir().exists() {
            return Ok(Vec::new());
        }
        let mut entries = tokio::fs::read_dir(self.dir()).await?;
        let mut templates = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_name().to_string_lossy().ends_with(".json") {
                continue;
            }
            let filename = entry.file_name().to_string_lossy().to_string();
            let contents = match tokio::fs::read_to_string(entry.path()).await {
                Ok(contents) => contents,
                Err(error) => {
                    tracing::warn!(
                        template_file = %filename,
                        "Skipping template that could not be read: {error}"
                    );
                    continue;
                }
            };
            let value: Value = match serde_json::from_str(&contents) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(
                        template_file = %filename,
                        "Skipping template with invalid JSON: {error}"
                    );
                    continue;
                }
            };
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let normalized = match normalize_workflow_value(value.clone()) {
                Ok(normalized) => normalized,
                Err(error) => {
                    tracing::warn!(
                        template_file = %filename,
                        "Skipping template with invalid workflow schema: {error}"
                    );
                    continue;
                }
            };
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| normalized.workflow.name.clone())
                .unwrap_or_else(|| filename.trim_end_matches(".json").to_string());
            templates.push(TemplateItem {
                name,
                description,
                workflow: normalized.workflow,
                notices: normalized.notices,
                template_file: filename,
            });
        }
        templates.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(templates)
    }
}

fn value_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn open_connection(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

fn status_from_str(s: &str) -> RuntimeStatus {
    match s {
        "running" => RuntimeStatus::Running,
        "paused" => RuntimeStatus::Paused,
        "completed" => RuntimeStatus::Completed,
        "failed" => RuntimeStatus::Failed,
        "aborted" => RuntimeStatus::Aborted,
        "restarted" => RuntimeStatus::Restarted,
        _ => RuntimeStatus::Running,
    }
}

fn status_as_str(status: &RuntimeStatus) -> &'static str {
    match status {
        RuntimeStatus::Running => "running",
        RuntimeStatus::Paused => "paused",
        RuntimeStatus::Completed => "completed",
        RuntimeStatus::Failed => "failed",
        RuntimeStatus::Aborted => "aborted",
        RuntimeStatus::Restarted => "restarted",
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::model::{
        WorkflowEdge, WorkflowEdgeOutcome, WorkflowLimits, WorkflowNode, WorkflowNodeType,
    };

    fn sample_workflow() -> WorkflowV3 {
        WorkflowV3 {
            version: 3,
            name: Some("sample".to_string()),
            goal: "goal".to_string(),
            cwd: String::new(),
            use_orchestrator: false,
            entry_node_id: "n1".to_string(),
            variables: Vec::new(),
            limits: WorkflowLimits {
                max_total_steps: 10,
                max_visits_per_node: 5,
            },
            nodes: vec![WorkflowNode {
                id: "n1".to_string(),
                name: "Node".to_string(),
                node_type: WorkflowNodeType::Task,
                agent: Some("claude".to_string()),
                prompt: "hi".to_string(),
                context_sources: Vec::new(),
                response_format: None,
                output_schema: None,
                retry_count: None,
                retry_delay: None,
                timeout: None,
                skip_condition: None,
                loop_max_iterations: None,
                loop_condition: None,
                split_failure_policy: crate::model::SplitFailurePolicy::BestEffortContinue,
                agent_config: None,
                cwd: None,
                continue_session_from: None,
            }],
            edges: vec![WorkflowEdge {
                id: "e1".to_string(),
                from: "n1".to_string(),
                to: "n1".to_string(),
                outcome: WorkflowEdgeOutcome::LoopContinue,
                label: None,
                branch_id: None,
                condition: None,
            }],
            agent_defaults: std::collections::BTreeMap::new(),
            ui: None,
        }
    }

    #[tokio::test]
    async fn stores_workflow_files() {
        let temp = TempDir::new().unwrap();
        let store = WorkflowStore::new(temp.path());
        let workflow = sample_workflow();
        store
            .save(
                "flow",
                NormalizedWorkflow {
                    workflow: workflow.clone(),
                    notices: Vec::new(),
                },
            )
            .await
            .unwrap();
        let loaded = store.get("flow").await.unwrap().unwrap();
        assert_eq!(loaded.workflow, workflow);
    }

    #[tokio::test]
    async fn lists_templates_and_skips_invalid_files() {
        let temp = TempDir::new().unwrap();
        let store = TemplateStore::new(temp.path());
        let valid = serde_json::json!({
            "version": 3,
            "name": "Valid Template",
            "description": "A valid workflow template",
            "goal": "goal",
            "cwd": "",
            "useOrchestrator": false,
            "entryNodeId": "n1",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [
                {
                    "id": "n1",
                    "name": "Node",
                    "type": "task",
                    "agent": "claude",
                    "prompt": "hi"
                }
            ],
            "edges": []
        });
        tokio::fs::write(
            temp.path().join("valid.json"),
            serde_json::to_vec_pretty(&valid).unwrap(),
        )
        .await
        .unwrap();
        tokio::fs::write(
            temp.path().join("invalid.json"),
            br#"{ "_version": 2, "steps": [] }"#,
        )
        .await
        .unwrap();

        let templates = store.list().await.unwrap();

        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].template_file, "valid.json");
        assert_eq!(templates[0].name, "Valid Template");
        assert_eq!(
            templates[0].description.as_deref(),
            Some("A valid workflow template")
        );
        assert_eq!(templates[0].workflow.version, 3);
    }

    #[tokio::test]
    async fn bundled_templates_are_all_valid() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        let expected = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
            .count();
        let store = TemplateStore::new(dir);

        let templates = store.list().await.unwrap();

        assert_eq!(templates.len(), expected);
        assert!(
            templates
                .iter()
                .all(|template| template.workflow.version == 3)
        );
    }

    #[test]
    fn seeds_bundled_templates_into_empty_directory() {
        let temp = TempDir::new().unwrap();

        seed_bundled_templates(temp.path()).unwrap();

        let seeded = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
            .count();
        assert!(seeded > 0);
    }

    #[tokio::test]
    async fn stores_run_records() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let persisted = PersistedRun {
            checkpoint: crate::runtime::RuntimeCheckpoint {
                run_id: "r1".to_string(),
                status: RuntimeStatus::Running,
                workflow_name: "wf".to_string(),
                current_node_id: Some("n1".to_string()),
                current_node_name: Some("Node".to_string()),
                all_results: Default::default(),
                last_output: String::new(),
                execution_epoch: 1,
                active_cursors: Vec::new(),
                split_families: Default::default(),
                collector_barriers: Default::default(),
                queued_approvals: Vec::new(),
                loop_counters: Default::default(),
                visit_counters: Default::default(),
                total_executed: 0,
                output_hashes: Default::default(),
                last_branch_origin_id: None,
                last_branch_choice: None,
                var_map: Default::default(),
                goal: String::new(),
                cwd: String::new(),
                use_orchestrator: false,
                max_total_steps: 10,
                max_visits_per_node: 5,
                started_at: now_iso(),
                updated_at: now_iso(),
                pending_approval: None,
                execution_log: crate::runtime::ExecutionLog {
                    run_id: "r1".to_string(),
                    workflow_name: "wf".to_string(),
                    goal: String::new(),
                    cwd: String::new(),
                    start_time: now_iso(),
                    end_time: None,
                    use_orchestrator: false,
                    aborted: false,
                    total_duration: "0".to_string(),
                    node_executions: Vec::new(),
                    decisions: Vec::new(),
                    transitions: Vec::new(),
                    terminal_reason: None,
                },
            },
            workflow: sample_workflow(),
        };
        db.upsert_run(&persisted).await.unwrap();
        assert!(db.get_run("r1").await.unwrap().is_some());
    }
}
