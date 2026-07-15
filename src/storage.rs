use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use include_dir::{Dir, include_dir};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::spawn_blocking;

use crate::{
    model::{NormalizedWorkflow, WorkflowV3, normalize_workflow_value},
    runtime::{
        ExecutionLog, InterruptedRunSummary, LogListItem, PersistedRun, RuntimeCheckpoint,
        RuntimeEvent, RuntimeStatus, new_stream_token,
    },
    util::{ensure_dir, now_iso, safe_name},
};

#[derive(Clone)]
pub struct Database {
    path: Arc<PathBuf>,
    connection: Pool<SqliteConnectionManager>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReapableTmuxSession {
    pub(crate) run_id: String,
    pub(crate) session_name: String,
    pub(crate) tmux_invocation: Option<tmux_tools_core::TmuxInvocation>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
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
const INTERRUPTED_RUNS_LIMIT: i64 = 200;

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
        let path = path.into();
        let manager = SqliteConnectionManager::file(&path).with_init(configure_connection);
        let connection = Pool::builder()
            .max_size(8)
            .min_idle(Some(0))
            .connection_timeout(Duration::from_secs(5))
            .build_unchecked(manager);
        Self {
            path: Arc::new(path),
            connection,
        }
    }

    pub fn path(&self) -> &Path {
        self.path.as_ref().as_path()
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        spawn_blocking(move || -> anyhow::Result<()> {
            with_connection(&connection, path.as_path(), |conn| {
                conn.execute_batch(
                    r#"
                CREATE TABLE IF NOT EXISTS runs (
                    run_id TEXT PRIMARY KEY,
                    stream_token TEXT,
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
                    terminal_reason TEXT,
                    tmux_bin TEXT,
                    tmux_socket TEXT,
                    tmux_prefix_json TEXT
                );
                CREATE TABLE IF NOT EXISTS run_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id TEXT NOT NULL,
                    event_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_run_events_run_id_id ON run_events(run_id, id);
                CREATE TABLE IF NOT EXISTS run_tmux_sessions (
                    run_id TEXT NOT NULL,
                    session_name TEXT NOT NULL,
                    PRIMARY KEY (run_id, session_name)
                );
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
                ensure_runs_stream_token_column(conn)?;
                ensure_runs_tmux_invocation_columns(conn)?;
                backfill_legacy_tmux_sessions(conn)?;
                Ok(())
            })
        })
        .await??;
        Ok(())
    }

    pub async fn upsert_run(&self, persisted: &PersistedRun) -> anyhow::Result<()> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let persisted = persisted.clone();
        spawn_blocking(move || -> anyhow::Result<()> {
            with_connection(&connection, path.as_path(), |conn| {
                conn.execute(
                    r#"
                INSERT INTO runs (
                    run_id, stream_token, status, workflow_name, current_node_id, current_node_name, total_executed,
                    started_at, updated_at, pending_approval_json, state_json, workflow_json, terminal_reason,
                    tmux_bin, tmux_socket, tmux_prefix_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                -- workflow_json and stream_token are intentionally frozen at first INSERT and are
                -- NOT updated on conflict; this is an immutable-snapshot invariant, not an oversight.
                -- The only sanctioned write-back to workflow_json is H3's one-time startup upgrade
                -- (which stamps version: 4), not a per-upsert or per-read write-back.
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
                    terminal_reason = excluded.terminal_reason
                "#,
                    params![
                        persisted.checkpoint.run_id,
                        persisted.stream_token,
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
                        persisted
                            .tmux_invocation
                            .as_ref()
                            .map(|invocation| invocation.tmux_bin.as_str()),
                        persisted
                            .tmux_invocation
                            .as_ref()
                            .and_then(|invocation| invocation.socket.as_deref()),
                        persisted
                            .tmux_invocation
                            .as_ref()
                            .map(|invocation| serde_json::to_string(&invocation.prefix))
                            .transpose()?,
                    ],
                )?;
                Ok(())
            })
        })
        .await??;
        Ok(())
    }

    pub async fn update_run_checkpoint(
        &self,
        checkpoint: &RuntimeCheckpoint,
    ) -> anyhow::Result<bool> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let checkpoint = checkpoint.clone();
        spawn_blocking(move || -> anyhow::Result<bool> {
            with_connection(&connection, path.as_path(), |conn| {
                let updated = conn.execute(
                    r#"
                    UPDATE runs SET
                        status = ?2,
                        workflow_name = ?3,
                        current_node_id = ?4,
                        current_node_name = ?5,
                        total_executed = ?6,
                        started_at = ?7,
                        updated_at = ?8,
                        pending_approval_json = ?9,
                        state_json = ?10,
                        terminal_reason = ?11
                    WHERE run_id = ?1
                    "#,
                    params![
                        checkpoint.run_id,
                        status_as_str(&checkpoint.status),
                        checkpoint.workflow_name,
                        checkpoint.current_node_id,
                        checkpoint.current_node_name,
                        checkpoint.total_executed,
                        checkpoint.started_at,
                        checkpoint.updated_at,
                        checkpoint
                            .pending_approval
                            .as_ref()
                            .map(serde_json::to_string)
                            .transpose()?,
                        serde_json::to_string(&checkpoint)?,
                        checkpoint.execution_log.terminal_reason,
                    ],
                )?;
                Ok(updated > 0)
            })
        })
        .await?
    }

    pub async fn get_run(&self, run_id: &str) -> anyhow::Result<Option<PersistedRun>> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        spawn_blocking(move || -> anyhow::Result<Option<PersistedRun>> {
            with_connection(&connection, path.as_path(), |conn| {
                let row = conn
                    .query_row(
                        "SELECT stream_token, state_json, workflow_json, tmux_bin, tmux_socket, tmux_prefix_json FROM runs WHERE run_id = ?1",
                        params![run_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, Option<String>>(4)?,
                                row.get::<_, Option<String>>(5)?,
                            ))
                        },
                    )
                    .optional()?;
                let Some((
                    stream_token,
                    state_json,
                    workflow_json,
                    tmux_bin,
                    tmux_socket,
                    tmux_prefix_json,
                )) = row
                else {
                    return Ok(None);
                };
                let workflow_value: Value = serde_json::from_str(&workflow_json)?;
                // Migrate-on-read normalization for the in-memory PersistedRun only: recomputed on
                // every read and intentionally NOT persisted back (no read-path write-back).
                // Persisting per-read would race with M3's wholesale state_json overwrite and would
                // break the stores_run_records test's immutable-snapshot assertions; the durable
                // repair is H3's one-time startup upgrade (CAS below), not this read path.
                let workflow = normalize_workflow_value(workflow_value)?.workflow;
                let normalized_workflow_json = serde_json::to_string(&workflow)?;
                if normalized_workflow_json != workflow_json {
                    // Compare-and-swap keeps concurrent readers/processes from replacing a
                    // workflow that changed after this read. Once upgraded, subsequent loads
                    // see the canonical JSON and skip the write.
                    conn.execute(
                        "UPDATE runs SET workflow_json = ?1 WHERE run_id = ?2 AND workflow_json = ?3",
                        params![normalized_workflow_json, run_id, workflow_json],
                    )?;
                }
                Ok(Some(PersistedRun {
                    stream_token,
                    tmux_invocation: decode_tmux_invocation(
                        tmux_bin,
                        tmux_socket,
                        tmux_prefix_json,
                    ),
                    checkpoint: serde_json::from_str(&state_json)?,
                    workflow,
                }))
            })
        })
        .await?
    }

    pub async fn store_tmux_invocation_if_missing(
        &self,
        run_id: &str,
        invocation: &tmux_tools_core::TmuxInvocation,
    ) -> anyhow::Result<bool> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        let invocation = invocation.clone();
        spawn_blocking(move || -> anyhow::Result<bool> {
            with_connection(&connection, path.as_path(), |conn| {
                let updated = conn.execute(
                    r#"
                    UPDATE runs SET
                        tmux_bin = ?2,
                        tmux_socket = ?3,
                        tmux_prefix_json = ?4
                    WHERE run_id = ?1
                      AND (tmux_bin IS NULL OR tmux_prefix_json IS NULL)
                    "#,
                    params![
                        run_id,
                        invocation.tmux_bin,
                        invocation.socket,
                        serde_json::to_string(&invocation.prefix)?,
                    ],
                )?;
                Ok(updated > 0)
            })
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
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        let status = status_as_str(&status).to_string();
        spawn_blocking(move || -> anyhow::Result<()> {
            with_connection(&connection, path.as_path(), |conn| {
                conn.execute(
                    "UPDATE runs SET status = ?2, terminal_reason = ?3, updated_at = ?4 WHERE run_id = ?1",
                    params![run_id, status, terminal_reason, now_iso()],
                )?;
                Ok(())
            })
        })
        .await??;
        Ok(())
    }

    pub async fn append_event(&self, run_id: &str, event: &RuntimeEvent) -> anyhow::Result<()> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        let event = event.clone();
        spawn_blocking(move || -> anyhow::Result<()> {
            with_connection(&connection, path.as_path(), |conn| {
                conn.execute(
                    "INSERT INTO run_events (run_id, event_json, created_at) VALUES (?1, ?2, ?3)",
                    params![run_id, serde_json::to_string(&event)?, now_iso()],
                )?;
                Ok(())
            })
        })
        .await??;
        Ok(())
    }

    pub async fn list_events(&self, run_id: &str) -> anyhow::Result<Vec<RuntimeEvent>> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        spawn_blocking(move || -> anyhow::Result<Vec<RuntimeEvent>> {
            with_connection(&connection, path.as_path(), |conn| {
                let mut stmt = conn.prepare(
                    "SELECT event_json FROM run_events WHERE run_id = ?1 ORDER BY id ASC",
                )?;
                let rows = stmt.query_map(params![run_id], |row| row.get::<_, String>(0))?;
                let mut events = Vec::new();
                for row in rows {
                    events.push(serde_json::from_str(&row?)?);
                }
                Ok(events)
            })
        })
        .await?
    }

    pub async fn register_tmux_session(
        &self,
        run_id: &str,
        session_name: &str,
    ) -> anyhow::Result<bool> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        let session_name = session_name.to_string();
        spawn_blocking(move || -> anyhow::Result<bool> {
            with_connection(&connection, path.as_path(), |conn| {
                let inserted = conn.execute(
                    r#"
                    INSERT INTO run_tmux_sessions (run_id, session_name)
                    SELECT ?1, ?2
                    WHERE EXISTS (SELECT 1 FROM runs WHERE run_id = ?1)
                    ON CONFLICT(run_id, session_name) DO NOTHING
                    "#,
                    params![run_id, session_name],
                )?;
                Ok(inserted > 0)
            })
        })
        .await?
    }

    pub async fn remove_tmux_sessions(
        &self,
        run_id: &str,
        session_names: &BTreeSet<String>,
    ) -> anyhow::Result<()> {
        if session_names.is_empty() {
            return Ok(());
        }
        let path = self.path.clone();
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        let session_names = session_names.clone();
        spawn_blocking(move || -> anyhow::Result<()> {
            with_connection(&connection, path.as_path(), |conn| {
                let transaction = conn.unchecked_transaction()?;
                for session_name in session_names {
                    transaction.execute(
                        "DELETE FROM run_tmux_sessions WHERE run_id = ?1 AND session_name = ?2",
                        params![run_id, session_name],
                    )?;
                }
                transaction.commit()?;
                Ok(())
            })
        })
        .await??;
        Ok(())
    }

    pub async fn list_run_tmux_session_names(&self, run_id: &str) -> anyhow::Result<Vec<String>> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let run_id = run_id.to_string();
        spawn_blocking(move || -> anyhow::Result<Vec<String>> {
            with_connection(&connection, path.as_path(), |conn| {
                let mut stmt = conn.prepare(
                    "SELECT session_name FROM run_tmux_sessions \
                     WHERE run_id = ?1 ORDER BY session_name",
                )?;
                let rows = stmt.query_map(params![run_id], |row| row.get::<_, String>(0))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
        })
        .await?
    }

    pub(crate) async fn list_reapable_tmux_sessions(
        &self,
    ) -> anyhow::Result<Vec<ReapableTmuxSession>> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        spawn_blocking(move || -> anyhow::Result<Vec<ReapableTmuxSession>> {
            with_connection(&connection, path.as_path(), |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT
                        runs.run_id,
                        run_tmux_sessions.session_name,
                        runs.tmux_bin,
                        runs.tmux_socket,
                        runs.tmux_prefix_json
                    FROM runs
                    INNER JOIN run_tmux_sessions
                        ON run_tmux_sessions.run_id = runs.run_id
                    WHERE runs.status IN ('completed', 'failed', 'aborted', 'restarted')
                    ORDER BY runs.run_id, run_tmux_sessions.session_name
                    "#,
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                })?;
                let mut sessions = Vec::new();
                for row in rows {
                    let (run_id, session_name, tmux_bin, tmux_socket, tmux_prefix_json) = row?;
                    sessions.push(ReapableTmuxSession {
                        run_id,
                        session_name,
                        tmux_invocation: decode_tmux_invocation(
                            tmux_bin,
                            tmux_socket,
                            tmux_prefix_json,
                        ),
                    });
                }
                Ok(sessions)
            })
        })
        .await?
    }

    pub async fn list_interrupted_runs(&self) -> anyhow::Result<Vec<InterruptedRunSummary>> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        spawn_blocking(move || -> anyhow::Result<Vec<InterruptedRunSummary>> {
            with_connection(&connection, path.as_path(), |conn| {
                let mut stmt = conn.prepare(
                    "SELECT run_id, status, workflow_name, current_node_id, current_node_name, \
                 total_executed, started_at, updated_at, pending_approval_json \
                 FROM runs WHERE status IN ('running', 'paused') ORDER BY updated_at DESC \
                 LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![INTERRUPTED_RUNS_LIMIT], |row| {
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
                    let (
                        run_id,
                        status,
                        workflow_name,
                        current_node_id,
                        current_node_name,
                        total_executed,
                        started_at,
                        updated_at,
                        pending_approval_json,
                    ) = row?;
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
        })
        .await?
    }

    pub async fn save_execution_log(&self, id: &str, log: &ExecutionLog) -> anyhow::Result<()> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let id = safe_name(id)?;
        let log_value = serde_json::to_value(log)?;
        spawn_blocking(move || -> anyhow::Result<()> {
            with_connection(&connection, path.as_path(), |conn| {
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
        })
        .await??;
        Ok(())
    }

    pub async fn list_logs(&self) -> anyhow::Result<Vec<LogListItem>> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        spawn_blocking(move || -> anyhow::Result<Vec<LogListItem>> {
            let rows = with_connection(&connection, path.as_path(), |conn| {
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
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })?;

            let mut logs = Vec::new();
            for row in rows {
                let (id, filename, workflow_name, goal, start_time, end_time, total_duration, aborted, run_id, data_json) =
                    row;
                let data_value: Value = serde_json::from_str(&data_json)?;
                let node_execs = data_value.get("nodeExecutions").and_then(Value::as_array);
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
        let connection = self.connection.clone();
        let id = safe_name(id)?;
        spawn_blocking(move || -> anyhow::Result<Option<Value>> {
            with_connection(&connection, path.as_path(), |conn| {
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
        })
        .await?
    }

    pub async fn delete_log(&self, id: &str) -> anyhow::Result<()> {
        let path = self.path.clone();
        let connection = self.connection.clone();
        let id = safe_name(id)?;
        spawn_blocking(move || -> anyhow::Result<()> {
            with_connection(&connection, path.as_path(), |conn| {
                conn.execute("DELETE FROM logs WHERE id = ?1", params![id])?;
                Ok(())
            })
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

fn with_connection<T>(
    connection: &Pool<SqliteConnectionManager>,
    path: &Path,
    operation: impl FnOnce(&Connection) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        ensure_dir(parent)?;
    }
    let connection = connection.get()?;
    operation(&connection)
}

fn configure_connection(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

fn ensure_runs_stream_token_column(conn: &Connection) -> anyhow::Result<()> {
    let has_stream_token = {
        let mut stmt = conn.prepare("PRAGMA table_info(runs)")?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut found = false;
        for column in columns {
            if column? == "stream_token" {
                found = true;
                break;
            }
        }
        found
    };

    if !has_stream_token {
        conn.execute("ALTER TABLE runs ADD COLUMN stream_token TEXT", [])?;
    }

    let run_ids = {
        let mut stmt = conn
            .prepare("SELECT run_id FROM runs WHERE stream_token IS NULL OR stream_token = ''")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for run_id in run_ids {
        conn.execute(
            "UPDATE runs SET stream_token = ?2 WHERE run_id = ?1",
            params![run_id, new_stream_token()],
        )?;
    }

    Ok(())
}

fn ensure_runs_tmux_invocation_columns(conn: &Connection) -> anyhow::Result<()> {
    let columns = {
        let mut stmt = conn.prepare("PRAGMA table_info(runs)")?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        columns.collect::<Result<BTreeSet<_>, _>>()?
    };

    for (column, sql_type) in [
        ("tmux_bin", "TEXT"),
        ("tmux_socket", "TEXT"),
        ("tmux_prefix_json", "TEXT"),
    ] {
        if !columns.contains(column) {
            conn.execute(
                &format!("ALTER TABLE runs ADD COLUMN {column} {sql_type}"),
                [],
            )?;
        }
    }

    Ok(())
}

fn decode_tmux_invocation(
    tmux_bin: Option<String>,
    socket: Option<String>,
    prefix_json: Option<String>,
) -> Option<tmux_tools_core::TmuxInvocation> {
    let tmux_bin = tmux_bin.filter(|value| !value.trim().is_empty())?;
    let prefix = serde_json::from_str(&prefix_json?).ok()?;
    Some(tmux_tools_core::TmuxInvocation {
        prefix,
        socket,
        tmux_bin,
    })
}

fn backfill_legacy_tmux_sessions(conn: &Connection) -> anyhow::Result<()> {
    let transaction = conn.unchecked_transaction()?;
    let legacy_rows = {
        let mut stmt = transaction.prepare("SELECT run_id, state_json FROM runs")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (run_id, state_json) in legacy_rows {
        let Ok(mut state) = serde_json::from_str::<Value>(&state_json) else {
            continue;
        };
        let Some(state_object) = state.as_object_mut() else {
            continue;
        };
        let Some(legacy_sessions) = state_object.remove("tmuxSessions") else {
            continue;
        };
        if let Some(sessions) = legacy_sessions.as_array() {
            for session_name in sessions.iter().filter_map(Value::as_str) {
                transaction.execute(
                    r#"
                    INSERT INTO run_tmux_sessions (run_id, session_name)
                    VALUES (?1, ?2)
                    ON CONFLICT(run_id, session_name) DO NOTHING
                    "#,
                    params![run_id, session_name],
                )?;
            }
        }
        transaction.execute(
            "UPDATE runs SET state_json = ?2 WHERE run_id = ?1",
            params![run_id, serde_json::to_string(&state)?],
        )?;
    }

    transaction.commit()?;
    Ok(())
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
        WORKFLOW_SCHEMA_VERSION, WorkflowEdge, WorkflowEdgeOutcome, WorkflowLimits, WorkflowNode,
        WorkflowNodeType,
    };

    fn sample_workflow() -> WorkflowV3 {
        WorkflowV3 {
            version: WORKFLOW_SCHEMA_VERSION,
            name: Some("sample".to_string()),
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
            nodes: vec![WorkflowNode {
                id: "n1".to_string(),
                name: "Node".to_string(),
                kind: WorkflowNodeType::Task.into(),
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
            subflows: std::collections::BTreeMap::new(),
            ui: None,
        }
    }

    fn sample_persisted_run(run_id: &str) -> PersistedRun {
        PersistedRun {
            stream_token: "test-stream-token".to_string(),
            tmux_invocation: None,
            checkpoint: crate::runtime::RuntimeCheckpoint {
                run_id: run_id.to_string(),
                status: RuntimeStatus::Running,
                workflow_name: "wf".to_string(),
                current_node_id: Some("n1".to_string()),
                current_node_name: Some("Node".to_string()),
                all_results: Default::default(),
                batch_item_results: Default::default(),
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
                    run_id: run_id.to_string(),
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
            "version": 4,
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
                    "agent": "claude",
                    "prompt": "hi",
                    "kind": { "type": "task" }
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
        assert_eq!(templates[0].workflow.version, 4);
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
                .all(|template| template.workflow.version == 4)
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
        let persisted = sample_persisted_run("r1");
        db.upsert_run(&persisted).await.unwrap();
        let loaded = db.get_run("r1").await.unwrap().unwrap();
        assert_eq!(loaded.workflow, persisted.workflow);
        assert_eq!(loaded.stream_token, persisted.stream_token);

        let mut changed = persisted.clone();
        changed.stream_token = "changed-stream-token".to_string();
        changed.workflow.name = Some("changed".to_string());
        changed.checkpoint.total_executed = 7;
        db.upsert_run(&changed).await.unwrap();
        let loaded = db.get_run("r1").await.unwrap().unwrap();
        assert_eq!(loaded.checkpoint.total_executed, 7);
        assert_eq!(loaded.workflow, persisted.workflow);
        assert_eq!(loaded.stream_token, persisted.stream_token);

        let mut checkpoint = loaded.checkpoint;
        checkpoint.total_executed = 8;
        db.update_run_checkpoint(&checkpoint).await.unwrap();
        let loaded = db.get_run("r1").await.unwrap().unwrap();
        assert_eq!(loaded.checkpoint.total_executed, 8);
        assert_eq!(loaded.workflow, persisted.workflow);
        assert_eq!(loaded.stream_token, persisted.stream_token);
    }

    #[tokio::test]
    async fn stores_resolved_tmux_invocation_with_run() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let mut persisted = sample_persisted_run("run-with-invocation");
        persisted.tmux_invocation = Some(tmux_tools_core::TmuxInvocation {
            prefix: vec!["sudo".to_string(), "-u".to_string(), "agent".to_string()],
            socket: Some("run-socket".to_string()),
            tmux_bin: "/opt/homebrew/bin/tmux".to_string(),
        });

        db.upsert_run(&persisted).await.unwrap();

        let loaded = db.get_run("run-with-invocation").await.unwrap().unwrap();
        assert_eq!(loaded.tmux_invocation, persisted.tmux_invocation);
    }

    #[tokio::test]
    async fn tmux_session_registration_survives_checkpoint_updates() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let mut persisted = sample_persisted_run("terminal-run");
        persisted.checkpoint.status = RuntimeStatus::Completed;
        db.upsert_run(&persisted).await.unwrap();

        crate::runtime::register_tmux_session(&db, "terminal-run", "silverbond-session")
            .await
            .unwrap();
        db.update_run_checkpoint(&persisted.checkpoint)
            .await
            .unwrap();

        let sessions = db.list_reapable_tmux_sessions().await.unwrap();
        assert_eq!(
            sessions,
            vec![ReapableTmuxSession {
                run_id: "terminal-run".to_string(),
                session_name: "silverbond-session".to_string(),
                tmux_invocation: None,
            }]
        );
    }

    #[tokio::test]
    async fn lists_run_tmux_session_names_in_order_for_one_run() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        db.upsert_run(&sample_persisted_run("selected-run"))
            .await
            .unwrap();
        db.upsert_run(&sample_persisted_run("other-run"))
            .await
            .unwrap();
        db.register_tmux_session("selected-run", "silverbond-zeta")
            .await
            .unwrap();
        db.register_tmux_session("selected-run", "silverbond-alpha")
            .await
            .unwrap();
        db.register_tmux_session("other-run", "silverbond-other")
            .await
            .unwrap();

        let sessions = db
            .list_run_tmux_session_names("selected-run")
            .await
            .unwrap();

        assert_eq!(
            sessions,
            vec![
                "silverbond-alpha".to_string(),
                "silverbond-zeta".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn limits_interrupted_run_summaries_to_two_hundred() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        for index in 0..201 {
            db.upsert_run(&sample_persisted_run(&format!("run-{index:03}")))
                .await
                .unwrap();
        }

        let runs = db.list_interrupted_runs().await.unwrap();

        assert_eq!(runs.len(), 200);
    }

    #[tokio::test]
    async fn concurrent_tmux_registrations_accumulate_and_active_runs_are_filtered() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let mut terminal = sample_persisted_run("terminal-run");
        terminal.checkpoint.status = RuntimeStatus::Aborted;
        db.upsert_run(&terminal).await.unwrap();
        db.upsert_run(&sample_persisted_run("active-run"))
            .await
            .unwrap();

        let (first, second, active) = tokio::join!(
            db.register_tmux_session("terminal-run", "silverbond-alpha"),
            db.register_tmux_session("terminal-run", "silverbond-beta"),
            db.register_tmux_session("active-run", "silverbond-active"),
        );
        first.unwrap();
        second.unwrap();
        active.unwrap();

        assert_eq!(
            db.list_reapable_tmux_sessions().await.unwrap(),
            vec![
                ReapableTmuxSession {
                    run_id: "terminal-run".to_string(),
                    session_name: "silverbond-alpha".to_string(),
                    tmux_invocation: None,
                },
                ReapableTmuxSession {
                    run_id: "terminal-run".to_string(),
                    session_name: "silverbond-beta".to_string(),
                    tmux_invocation: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn lists_reapable_tmux_sessions_with_persisted_invocations_in_one_query() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        let invocation = tmux_tools_core::TmuxInvocation {
            prefix: vec!["sudo".to_string(), "-u".to_string(), "agent".to_string()],
            socket: Some("reaper-socket".to_string()),
            tmux_bin: "/custom/tmux".to_string(),
        };
        let mut terminal = sample_persisted_run("terminal-run");
        terminal.checkpoint.status = RuntimeStatus::Failed;
        terminal.tmux_invocation = Some(invocation.clone());
        db.upsert_run(&terminal).await.unwrap();
        db.register_tmux_session("terminal-run", "silverbond-alpha")
            .await
            .unwrap();
        db.register_tmux_session("terminal-run", "silverbond-beta")
            .await
            .unwrap();

        let sessions = db.list_reapable_tmux_sessions().await.unwrap();

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].run_id, "terminal-run");
        assert_eq!(sessions[0].session_name, "silverbond-alpha");
        assert_eq!(sessions[0].tmux_invocation, Some(invocation.clone()));
        assert_eq!(sessions[1].run_id, "terminal-run");
        assert_eq!(sessions[1].session_name, "silverbond-beta");
        assert_eq!(sessions[1].tmux_invocation, Some(invocation));
    }

    #[tokio::test]
    async fn init_backfills_and_retires_legacy_checkpoint_tmux_sessions() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("silverbond.db");
        let db = Database::new(path.clone());
        db.init().await.unwrap();
        let mut persisted = sample_persisted_run("legacy-terminal-run");
        persisted.checkpoint.status = RuntimeStatus::Failed;
        db.upsert_run(&persisted).await.unwrap();
        with_connection(&db.connection, db.path(), |conn| {
            let mut state = serde_json::to_value(&persisted.checkpoint)?;
            state["tmuxSessions"] =
                serde_json::json!(["silverbond-legacy-alpha", "silverbond-legacy-beta"]);
            conn.execute("DROP TABLE run_tmux_sessions", [])?;
            conn.execute(
                "UPDATE runs SET state_json = ?2 WHERE run_id = ?1",
                params!["legacy-terminal-run", serde_json::to_string(&state)?],
            )?;
            Ok(())
        })
        .unwrap();

        let migrated = Database::new(path);
        migrated.init().await.unwrap();

        let sessions = migrated.list_reapable_tmux_sessions().await.unwrap();
        assert_eq!(
            sessions,
            vec![
                ReapableTmuxSession {
                    run_id: "legacy-terminal-run".to_string(),
                    session_name: "silverbond-legacy-alpha".to_string(),
                    tmux_invocation: None,
                },
                ReapableTmuxSession {
                    run_id: "legacy-terminal-run".to_string(),
                    session_name: "silverbond-legacy-beta".to_string(),
                    tmux_invocation: None,
                },
            ]
        );
        let state_json = with_connection(&migrated.connection, migrated.path(), |conn| {
            Ok(conn.query_row(
                "SELECT state_json FROM runs WHERE run_id = ?1",
                params!["legacy-terminal-run"],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap();
        let state: Value = serde_json::from_str(&state_json).unwrap();
        assert!(state.get("tmuxSessions").is_none());
    }

    #[tokio::test]
    async fn writer_progresses_while_a_read_connection_is_checked_out() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        db.upsert_run(&sample_persisted_run("concurrent-run"))
            .await
            .unwrap();

        let connection = db.connection.clone();
        let path = db.path.clone();
        let (read_started_tx, read_started_rx) = std::sync::mpsc::channel();
        let (release_read_tx, release_read_rx) = std::sync::mpsc::channel();
        let held_read = spawn_blocking(move || {
            with_connection(&connection, path.as_path(), |conn| {
                conn.query_row("SELECT COUNT(*) FROM runs", [], |row| row.get::<_, i64>(0))?;
                read_started_tx.send(()).unwrap();
                release_read_rx.recv().unwrap();
                Ok(())
            })
        });
        read_started_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();

        let append_result = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            db.append_event("concurrent-run", &RuntimeEvent::new("concurrent_write")),
        )
        .await;

        release_read_tx.send(()).unwrap();
        held_read.await.unwrap().unwrap();
        assert!(
            append_result.is_ok(),
            "a checked-out read connection must not serialize an unrelated writer"
        );
    }

    #[tokio::test]
    async fn get_run_persists_structurally_migrated_v3_workflow_as_v4() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        db.upsert_run(&sample_persisted_run("legacy-run"))
            .await
            .unwrap();

        let legacy_workflow = serde_json::json!({
            "version": 3,
            "goal": "legacy run",
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
        with_connection(&db.connection, db.path(), |conn| {
            conn.execute(
                "UPDATE runs SET workflow_json = ?2 WHERE run_id = ?1",
                params!["legacy-run", serde_json::to_string(&legacy_workflow)?],
            )?;
            Ok(())
        })
        .unwrap();

        let loaded = db.get_run("legacy-run").await.unwrap().unwrap();
        assert_eq!(loaded.workflow.version, 4);
        assert!(matches!(
            &loaded.workflow.nodes[0].kind,
            crate::model::NodeKind::Decide { decide_config }
                if decide_config.outcomes == vec!["yes".to_string()]
        ));

        let stored_workflow_json = with_connection(&db.connection, db.path(), |conn| {
            Ok(conn.query_row(
                "SELECT workflow_json FROM runs WHERE run_id = ?1",
                params!["legacy-run"],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap();
        let stored_workflow: Value = serde_json::from_str(&stored_workflow_json).unwrap();
        assert_eq!(stored_workflow["version"], 4);
        assert_eq!(stored_workflow["nodes"][0]["kind"]["type"], "decide");
        assert!(stored_workflow["nodes"][0].get("type").is_none());
    }

    #[tokio::test]
    async fn get_run_leaves_canonical_v4_workflow_unchanged() {
        let temp = TempDir::new().unwrap();
        let db = Database::new(temp.path().join("silverbond.db"));
        db.init().await.unwrap();
        db.upsert_run(&sample_persisted_run("current-run"))
            .await
            .unwrap();

        let current_workflow = serde_json::json!({
            "version": 4,
            "goal": "current run",
            "cwd": "/tmp",
            "useOrchestrator": false,
            "entryNodeId": "decide",
            "variables": [],
            "limits": { "maxTotalSteps": 10, "maxVisitsPerNode": 5 },
            "nodes": [{
                "id": "decide",
                "name": "Pick",
                "kind": {
                    "type": "decide",
                    "decideConfig": {
                        "prompt": "Pick one",
                        "outcomes": ["yes"]
                    }
                }
            }],
            "edges": []
        });
        let canonical_json = serde_json::to_string(&current_workflow).unwrap();
        with_connection(&db.connection, db.path(), |conn| {
            conn.execute(
                "UPDATE runs SET workflow_json = ?2 WHERE run_id = ?1",
                params!["current-run", canonical_json.clone()],
            )?;
            Ok(())
        })
        .unwrap();

        let loaded = db.get_run("current-run").await.unwrap().unwrap();
        assert_eq!(loaded.workflow.version, 4);
        assert!(matches!(
            &loaded.workflow.nodes[0].kind,
            crate::model::NodeKind::Decide { decide_config }
                if decide_config.outcomes == vec!["yes".to_string()]
        ));

        let stored_workflow_json = with_connection(&db.connection, db.path(), |conn| {
            Ok(conn.query_row(
                "SELECT workflow_json FROM runs WHERE run_id = ?1",
                params!["current-run"],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap();
        let stored_workflow: Value = serde_json::from_str(&stored_workflow_json).unwrap();
        assert_eq!(stored_workflow["version"], 4);
        assert_eq!(stored_workflow["goal"], "current run");
        assert_eq!(stored_workflow["entryNodeId"], "decide");
        assert_eq!(stored_workflow["nodes"][0]["kind"]["type"], "decide");
        assert_eq!(
            stored_workflow["nodes"][0]["kind"]["decideConfig"]["outcomes"],
            serde_json::json!(["yes"])
        );
        assert!(stored_workflow["nodes"][0].get("type").is_none());
    }
}
