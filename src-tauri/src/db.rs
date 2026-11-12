use crate::markets::{Holding, MarketOrder, PortfolioTrade, Watchlist};
use crate::types::{
    AgentRunMetrics, BenchmarkResult, ConversationSummary, HkuRequestUsage, ImageInput, PlanStatus,
    PlanStep, RefactorPlan, SecurityAuditRecord, SecurityEvent, SessionEventRecord, StoredMessage,
    ToolArgumentRepair, ToolCall, UsageModelBreakdown, UsageRecord, UsageStats, UsageSummary,
    UsageTotals, VerificationResult, Workspace, WorkspaceChangeSet, WorkspaceMetadata,
};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub type Db = Arc<Mutex<Connection>>;
const SCHEMA_VERSION: i64 = 8;

pub fn init_db(path: &Path) -> Result<Db, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    backup_before_migration(path)?;
    let connection = Connection::open(path).map_err(|e| e.to_string())?;
    connection
        .execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                parent_id TEXT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                model TEXT,
                profile TEXT,
                usage_json TEXT,
                reasoning TEXT,
                attachments_json TEXT,
                active INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS message_edits (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                message_id TEXT NOT NULL,
                original_content TEXT NOT NULL,
                edited_content TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS summaries (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                mime_type TEXT,
                extracted_text TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS benchmark_runs (
                id TEXT PRIMARY KEY,
                model TEXT NOT NULL,
                context_length INTEGER NOT NULL,
                ttft_ms INTEGER,
                tokens_per_second REAL,
                total_ms INTEGER,
                input_tokens INTEGER,
                output_tokens INTEGER,
                memory_before INTEGER,
                memory_during INTEGER,
                memory_after INTEGER,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                root_path TEXT NOT NULL UNIQUE,
                pinned INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tool_calls (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                message_id TEXT,
                tool_name TEXT NOT NULL,
                arguments_json TEXT NOT NULL,
                result TEXT,
                ok INTEGER,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS security_events (
                id TEXT PRIMARY KEY,
                conversation_id TEXT,
                tool_name TEXT NOT NULL,
                action TEXT NOT NULL,
                decision TEXT NOT NULL,
                reason TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS context_snapshots (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                summary TEXT NOT NULL,
                retained_messages INTEGER NOT NULL,
                compacted_messages INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session_approvals (
                id TEXT PRIMARY KEY,
                tool_name TEXT NOT NULL,
                workspace_root TEXT,
                command_class TEXT,
                created_at TEXT NOT NULL,
                expires_at TEXT
            );
            CREATE TABLE IF NOT EXISTS resource_snapshots (
                id TEXT PRIMARY KEY,
                benchmark_id TEXT,
                phase TEXT NOT NULL,
                telemetry_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session_events (
                id TEXT PRIMARY KEY,
                conversation_id TEXT,
                turn_id TEXT,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS session_events_conversation_idx
                ON session_events(conversation_id, created_at);
            CREATE TABLE IF NOT EXISTS refactor_plans (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS plan_steps (
                id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL REFERENCES refactor_plans(id) ON DELETE CASCADE,
                description TEXT NOT NULL,
                paths_json TEXT NOT NULL,
                risk TEXT NOT NULL,
                verification_json TEXT NOT NULL,
                status TEXT NOT NULL,
                position INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspace_metadata (
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                kind TEXT NOT NULL,
                bytes INTEGER NOT NULL,
                modified_at TEXT NOT NULL,
                ignored INTEGER NOT NULL DEFAULT 0,
                git_status TEXT,
                PRIMARY KEY(workspace_id, path)
            );
            CREATE TABLE IF NOT EXISTS workspace_project_index (
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                modified_ns INTEGER NOT NULL,
                bytes INTEGER NOT NULL,
                summary_json TEXT NOT NULL,
                PRIMARY KEY(workspace_id, path)
            );
            CREATE TABLE IF NOT EXISTS workspace_change_sets (
                id TEXT PRIMARY KEY,
                conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
                turn_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS verification_runs (
                id TEXT PRIMARY KEY,
                conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
                turn_id TEXT,
                command TEXT NOT NULL,
                status TEXT NOT NULL,
                output TEXT,
                duration_ms INTEGER,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS context_measurements (
                id TEXT PRIMARY KEY,
                conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
                configured_length INTEGER NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS resource_samples (
                id TEXT PRIMARY KEY,
                benchmark_id TEXT,
                conversation_id TEXT,
                phase TEXT NOT NULL,
                telemetry_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS usage_records (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                surface TEXT NOT NULL DEFAULT 'unassigned',
                conversation_id TEXT,
                turn_id TEXT NOT NULL,
                input_tokens INTEGER,
                cached_input_tokens INTEGER,
                cache_miss_tokens INTEGER,
                cache_write_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                ttft_ms INTEGER,
                total_ms INTEGER,
                tokens_per_second REAL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS usage_records_created_idx
                ON usage_records(created_at DESC);
            CREATE INDEX IF NOT EXISTS usage_records_model_idx
                ON usage_records(provider_id, model, created_at DESC);
            CREATE TABLE IF NOT EXISTS hku_request_attempts (
                id TEXT PRIMARY KEY,
                model TEXT NOT NULL,
                status TEXT NOT NULL,
                http_status INTEGER,
                created_at TEXT NOT NULL,
                finished_at TEXT
            );
            CREATE INDEX IF NOT EXISTS hku_request_attempts_model_idx
                ON hku_request_attempts(model, created_at DESC);
            CREATE TABLE IF NOT EXISTS tool_argument_repairs (
                id TEXT PRIMARY KEY,
                conversation_id TEXT,
                turn_id TEXT,
                tool_name TEXT NOT NULL,
                original_text TEXT NOT NULL,
                normalized_json TEXT NOT NULL,
                reason TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS market_watchlists (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                symbols_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS market_holdings (
                id TEXT PRIMARY KEY,
                account TEXT NOT NULL,
                symbol TEXT NOT NULL,
                shares REAL NOT NULL,
                average_cost REAL NOT NULL,
                currency TEXT NOT NULL,
                source TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(account, symbol)
            );
            CREATE TABLE IF NOT EXISTS market_alerts (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                kind TEXT NOT NULL,
                threshold REAL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                triggered_at TEXT
            );
            CREATE TABLE IF NOT EXISTS market_orders (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                quantity REAL NOT NULL,
                order_type TEXT NOT NULL,
                limit_price REAL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                preview_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS market_trades (
                id TEXT PRIMARY KEY,
                account TEXT NOT NULL,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                quantity REAL NOT NULL,
                price REAL NOT NULL,
                commission REAL NOT NULL DEFAULT 0,
                trade_date TEXT NOT NULL,
                notes TEXT NOT NULL DEFAULT '',
                source TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS market_trades_lookup_idx
                ON market_trades(account, symbol, trade_date);
            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                message_id UNINDEXED,
                conversation_id UNINDEXED,
                title,
                content
            );
            "#,
        )
        .map_err(|e| e.to_string())?;
    ensure_column(&connection, "messages", "reasoning", "TEXT")?;
    ensure_column(&connection, "messages", "attachments_json", "TEXT")?;
    ensure_column(
        &connection,
        "messages",
        "active",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(&connection, "conversations", "workspace_id", "TEXT")?;
    ensure_column(
        &connection,
        "conversations",
        "archived",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &connection,
        "workspaces",
        "pinned",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &connection,
        "workspaces",
        "archived",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &connection,
        "usage_records",
        "surface",
        "TEXT NOT NULL DEFAULT 'unassigned'",
    )?;
    ensure_column(&connection, "tool_argument_repairs", "turn_id", "TEXT")?;
    connection
        .execute_batch("CREATE INDEX IF NOT EXISTS tool_argument_repairs_turn_idx ON tool_argument_repairs(turn_id);")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(Mutex::new(connection)))
}

fn backup_before_migration(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let current_version = Connection::open(path)
        .and_then(|connection| connection.query_row("PRAGMA user_version", [], |row| row.get(0)))
        .unwrap_or(0_i64);
    if current_version >= SCHEMA_VERSION {
        return Ok(());
    }
    let backup = path.with_extension(format!("sqlite3.pre-v{}-backup", SCHEMA_VERSION));
    if !backup.exists() {
        std::fs::copy(path, &backup).map_err(|error| {
            format!(
                "could not create database migration backup at {}: {error}",
                backup.display()
            )
        })?;
    }
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .any(|name| name == column);
    if !exists {
        connection
            .execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn ensure_conversation(db: &Db) -> Result<ConversationSummary, String> {
    let existing = list_conversations(db)?;
    if let Some(first) = existing.into_iter().next() {
        return Ok(first);
    }
    create_conversation(db, Some("New local chat".to_string()))
}

pub fn create_conversation(db: &Db, title: Option<String>) -> Result<ConversationSummary, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let title = title.unwrap_or_else(|| "New chat".to_string());
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO conversations(id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![id, title, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(ConversationSummary {
        id,
        title,
        updated_at: now,
        pinned: false,
        archived: false,
        workspace_id: None,
    })
}

pub fn list_conversations(db: &Db) -> Result<Vec<ConversationSummary>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, title, updated_at, pinned, archived, workspace_id FROM conversations ORDER BY archived ASC, pinned DESC, updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
                pinned: row.get::<_, i64>(3)? != 0,
                archived: row.get::<_, i64>(4)? != 0,
                workspace_id: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn set_conversation_pinned(db: &Db, conversation_id: &str, pinned: bool) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE conversations SET pinned = ?1, updated_at = ?2 WHERE id = ?3",
            params![pinned as i64, Utc::now().to_rfc3339(), conversation_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_conversation_archived(
    db: &Db,
    conversation_id: &str,
    archived: bool,
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE conversations SET archived = ?1, updated_at = ?2 WHERE id = ?3",
            params![archived as i64, Utc::now().to_rfc3339(), conversation_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_conversation(db: &Db, conversation_id: &str) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "DELETE FROM messages_fts WHERE conversation_id = ?1",
            [conversation_id],
        )
        .map_err(|e| e.to_string())?;
    transaction
        .execute("DELETE FROM conversations WHERE id = ?1", [conversation_id])
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())
}

pub fn list_messages(db: &Db, conversation_id: &str) -> Result<Vec<StoredMessage>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, conversation_id, parent_id, role, content, created_at, model, profile, usage_json, reasoning, attachments_json
             FROM messages WHERE conversation_id = ?1 AND active = 1 ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([conversation_id], |row| {
            let usage_json: Option<String> = row.get(8)?;
            let usage = usage_json
                .as_deref()
                .and_then(|json| serde_json::from_str::<UsageStats>(json).ok());
            let images_json: Option<String> = row.get(10)?;
            let images = images_json
                .as_deref()
                .and_then(|json| serde_json::from_str(json).ok())
                .unwrap_or_default();
            Ok(StoredMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                parent_id: row.get(2)?,
                role: row.get(3)?,
                content: row.get(4)?,
                images,
                created_at: row.get(5)?,
                model: row.get(6)?,
                profile: row.get(7)?,
                usage,
                reasoning: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn save_message(
    db: &Db,
    conversation_id: &str,
    parent_id: Option<&str>,
    role: &str,
    content: &str,
    model: Option<&str>,
    profile: Option<&str>,
    usage: Option<&UsageStats>,
) -> Result<StoredMessage, String> {
    save_message_with_reasoning(
        db,
        conversation_id,
        parent_id,
        role,
        content,
        model,
        profile,
        usage,
        None,
    )
}

pub fn save_message_with_reasoning(
    db: &Db,
    conversation_id: &str,
    parent_id: Option<&str>,
    role: &str,
    content: &str,
    model: Option<&str>,
    profile: Option<&str>,
    usage: Option<&UsageStats>,
    reasoning: Option<&str>,
) -> Result<StoredMessage, String> {
    save_message_with_reasoning_and_attachments(
        db,
        conversation_id,
        parent_id,
        role,
        content,
        model,
        profile,
        usage,
        reasoning,
        None,
    )
}

pub fn save_message_with_attachments(
    db: &Db,
    conversation_id: &str,
    parent_id: Option<&str>,
    role: &str,
    content: &str,
    model: Option<&str>,
    profile: Option<&str>,
    usage: Option<&UsageStats>,
    images: Option<&[ImageInput]>,
) -> Result<StoredMessage, String> {
    save_message_with_reasoning_and_attachments(
        db,
        conversation_id,
        parent_id,
        role,
        content,
        model,
        profile,
        usage,
        None,
        images,
    )
}

pub fn save_message_with_reasoning_and_attachments(
    db: &Db,
    conversation_id: &str,
    parent_id: Option<&str>,
    role: &str,
    content: &str,
    model: Option<&str>,
    profile: Option<&str>,
    usage: Option<&UsageStats>,
    reasoning: Option<&str>,
    images: Option<&[ImageInput]>,
) -> Result<StoredMessage, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let usage_json = usage.and_then(|value| serde_json::to_string(value).ok());
    let attachments_json = images
        .filter(|values| !values.is_empty())
        .and_then(|values| serde_json::to_string(values).ok());
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO messages(id, conversation_id, parent_id, role, content, created_at, model, profile, usage_json, reasoning, attachments_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![id, conversation_id, parent_id, role, content, now, model, profile, usage_json, reasoning, attachments_json],
        )
        .map_err(|e| e.to_string())?;
    connection
        .execute(
            "UPDATE conversations SET updated_at = ?1, title = CASE WHEN title = 'New local chat' AND ?2 != '' THEN substr(?2, 1, 80) ELSE title END WHERE id = ?3",
            params![now, content, conversation_id],
        )
        .map_err(|e| e.to_string())?;
    let title: String = connection
        .query_row(
            "SELECT title FROM conversations WHERE id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "New chat".to_string());
    let _ = connection.execute(
        "INSERT INTO messages_fts(message_id, conversation_id, title, content) VALUES (?1, ?2, ?3, ?4)",
        params![id, conversation_id, title, content],
    );
    Ok(StoredMessage {
        id,
        conversation_id: conversation_id.to_string(),
        parent_id: parent_id.map(str::to_string),
        role: role.to_string(),
        content: content.to_string(),
        images: images.map(|values| values.to_vec()).unwrap_or_default(),
        created_at: now,
        model: model.map(str::to_string),
        profile: profile.map(str::to_string),
        usage: usage.cloned(),
        reasoning: reasoning.map(str::to_string),
    })
}

/// Rewrite a user message in the active conversation branch while preserving
/// the original text and the superseded tail for local recovery/inspection.
pub fn edit_message(
    db: &Db,
    conversation_id: &str,
    message_id: &str,
    content: &str,
) -> Result<StoredMessage, String> {
    edit_message_with_attachments(db, conversation_id, message_id, content, None)
}

pub fn edit_message_with_attachments(
    db: &Db,
    conversation_id: &str,
    message_id: &str,
    content: &str,
    images: Option<&[ImageInput]>,
) -> Result<StoredMessage, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() && images.map(|values| values.is_empty()).unwrap_or(true) {
        return Err("message cannot be empty".into());
    }
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let original: StoredMessage = transaction
        .query_row(
            "SELECT id, conversation_id, parent_id, role, content, created_at, model, profile, usage_json, reasoning, attachments_json
             FROM messages WHERE id = ?1 AND conversation_id = ?2 AND active = 1",
            params![message_id, conversation_id],
            |row| {
                let usage_json: Option<String> = row.get(8)?;
                let usage = usage_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<UsageStats>(json).ok());
                let images_json: Option<String> = row.get(10)?;
                let images = images_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str(json).ok())
                    .unwrap_or_default();
                Ok(StoredMessage {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    role: row.get(3)?,
                    content: row.get(4)?,
                    images,
                    created_at: row.get(5)?,
                    model: row.get(6)?,
                    profile: row.get(7)?,
                    usage,
                    reasoning: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "message is no longer in the active conversation branch".to_string())?;
    if original.role != "user" {
        return Err("only user messages can be edited".into());
    }
    let edited_at = Utc::now().to_rfc3339();
    transaction
        .execute(
            "INSERT INTO message_edits(id, conversation_id, message_id, original_content, edited_content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                conversation_id,
                message_id,
                original.content,
                trimmed,
                edited_at
            ],
        )
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "UPDATE messages SET active = 0
             WHERE conversation_id = ?1 AND active = 1
               AND rowid > (SELECT rowid FROM messages WHERE id = ?2)",
            params![conversation_id, message_id],
        )
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "UPDATE messages SET content = ?1, usage_json = NULL, reasoning = NULL, attachments_json = ?2 WHERE id = ?3 AND conversation_id = ?4",
            params![
                trimmed,
                images
                    .filter(|values| !values.is_empty())
                    .and_then(|values| serde_json::to_string(values).ok()),
                message_id,
                conversation_id
            ],
        )
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "UPDATE conversations SET updated_at = ?1, title = CASE WHEN title = 'New local chat' THEN substr(?2, 1, 80) ELSE title END WHERE id = ?3",
            params![edited_at, trimmed, conversation_id],
        )
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;

    let mut updated = original;
    updated.content = trimmed.to_string();
    updated.images = images.map(|values| values.to_vec()).unwrap_or_default();
    updated.usage = None;
    updated.reasoning = None;
    Ok(updated)
}

pub fn list_workspaces(db: &Db) -> Result<Vec<Workspace>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection.prepare("SELECT id, name, root_path, pinned, archived, created_at, updated_at FROM workspaces ORDER BY archived ASC, pinned DESC, updated_at DESC").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let root_path: String = row.get(2)?;
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                exists: Path::new(&root_path).is_dir(),
                root_path,
                pinned: row.get::<_, i64>(3)? != 0,
                archived: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn get_workspace(db: &Db, workspace_id: &str) -> Result<Option<Workspace>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .query_row(
            "SELECT id, name, root_path, pinned, archived, created_at, updated_at FROM workspaces WHERE id = ?1",
            [workspace_id],
            |row| {
                let root_path: String = row.get(2)?;
                Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                exists: Path::new(&root_path).is_dir(),
                root_path,
                pinned: row.get::<_, i64>(3)? != 0,
                archived: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
}

pub fn add_workspace(db: &Db, name: &str, root_path: &str) -> Result<Workspace, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection.execute("INSERT INTO workspaces(id, name, root_path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(root_path) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at", params![id, name, root_path, now]).map_err(|e| e.to_string())?;
    let workspace = connection.query_row("SELECT id, name, root_path, pinned, archived, created_at, updated_at FROM workspaces WHERE root_path = ?1", [root_path], |row| Ok(Workspace { id: row.get(0)?, name: row.get(1)?, root_path: row.get(2)?, exists: true, pinned: row.get::<_, i64>(3)? != 0, archived: row.get::<_, i64>(4)? != 0, created_at: row.get(5)?, updated_at: row.get(6)? })).map_err(|e| e.to_string())?;
    Ok(workspace)
}

pub fn remove_workspace(db: &Db, workspace_id: &str) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE conversations SET workspace_id = NULL WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|e| e.to_string())?;
    connection
        .execute("DELETE FROM workspaces WHERE id = ?1", [workspace_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_workspace_pinned(db: &Db, workspace_id: &str, pinned: bool) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE workspaces SET pinned = ?1, updated_at = ?2 WHERE id = ?3",
            params![pinned as i64, Utc::now().to_rfc3339(), workspace_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_workspace_archived(db: &Db, workspace_id: &str, archived: bool) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE workspaces SET archived = ?1, updated_at = ?2 WHERE id = ?3",
            params![archived as i64, Utc::now().to_rfc3339(), workspace_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn rename_workspace(db: &Db, workspace_id: &str, name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("workspace name cannot be empty".into());
    }
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE workspaces SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![trimmed, Utc::now().to_rfc3339(), workspace_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_conversation_workspace(
    db: &Db,
    conversation_id: &str,
    workspace_id: Option<&str>,
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE conversations SET workspace_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![workspace_id, Utc::now().to_rfc3339(), conversation_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_context_snapshot(
    db: &Db,
    conversation_id: &str,
    summary: &str,
    retained: usize,
    compacted: usize,
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection.execute("INSERT INTO context_snapshots(id, conversation_id, summary, retained_messages, compacted_messages, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![Uuid::new_v4().to_string(), conversation_id, summary, retained as i64, compacted as i64, Utc::now().to_rfc3339()]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_session_event(
    db: &Db,
    conversation_id: Option<&str>,
    turn_id: Option<&str>,
    event_type: &str,
    payload: &Value,
) -> Result<SessionEventRecord, String> {
    let event = SessionEventRecord {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation_id.map(str::to_string),
        turn_id: turn_id.map(str::to_string),
        event_type: event_type.to_string(),
        payload: payload.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO session_events(id, conversation_id, turn_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![event.id, event.conversation_id, event.turn_id, event.event_type, serde_json::to_string(&event.payload).map_err(|e| e.to_string())?, event.created_at],
        )
        .map_err(|e| e.to_string())?;
    Ok(event)
}

pub fn list_session_events(
    db: &Db,
    conversation_id: Option<&str>,
    limit: usize,
) -> Result<Vec<SessionEventRecord>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, conversation_id, turn_id, event_type, payload_json, created_at FROM session_events WHERE (?1 IS NULL OR conversation_id = ?1) ORDER BY created_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![conversation_id, limit.min(2_000) as i64], |row| {
            let payload: String = row.get(4)?;
            Ok(SessionEventRecord {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                turn_id: row.get(2)?,
                event_type: row.get(3)?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn save_tool_argument_repair(
    db: &Db,
    conversation_id: Option<&str>,
    turn_id: &str,
    repair: &ToolArgumentRepair,
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO tool_argument_repairs(id, conversation_id, turn_id, tool_name, original_text, normalized_json, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![Uuid::new_v4().to_string(), conversation_id, turn_id, repair.tool_name, repair.original, serde_json::to_string(&repair.normalized).map_err(|e| e.to_string())?, repair.reason, Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_tool_call(
    db: &Db,
    conversation_id: Option<&str>,
    call: &ToolCall,
    result: Option<&str>,
    ok: Option<bool>,
) -> Result<(), String> {
    let Some(conversation_id) = conversation_id else {
        return Ok(());
    };
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let id = if call.id.trim().is_empty() {
        Uuid::new_v4().to_string()
    } else {
        call.id.clone()
    };
    connection
        .execute(
            "INSERT INTO tool_calls(id, conversation_id, message_id, tool_name, arguments_json, result, ok, created_at) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(id) DO UPDATE SET result = excluded.result, ok = excluded.ok",
            params![
                id,
                conversation_id,
                call.name,
                serde_json::to_string(&call.arguments).map_err(|e| e.to_string())?,
                result,
                ok.map(|value| value as i64),
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_workspace_change_set(
    db: &Db,
    conversation_id: Option<&str>,
    change_set: &WorkspaceChangeSet,
) -> Result<WorkspaceChangeSet, String> {
    let mut connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let existing: Option<(String, String)> = transaction
        .query_row(
            "SELECT id, payload_json FROM workspace_change_sets WHERE turn_id = ?1 ORDER BY created_at LIMIT 1",
            [&change_set.turn_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (id, mut aggregate) = match existing {
        Some((id, payload)) => (
            id,
            serde_json::from_str::<WorkspaceChangeSet>(&payload).unwrap_or_default(),
        ),
        None => (Uuid::new_v4().to_string(), WorkspaceChangeSet::default()),
    };
    aggregate.turn_id = change_set.turn_id.clone();
    aggregate
        .verification
        .extend(change_set.verification.clone());
    for incoming in &change_set.files {
        if let Some(existing) = aggregate
            .files
            .iter_mut()
            .find(|file| file.path == incoming.path)
        {
            existing.added_lines = existing.added_lines.saturating_add(incoming.added_lines);
            existing.removed_lines = existing
                .removed_lines
                .saturating_add(incoming.removed_lines);
            existing.bytes_after = incoming.bytes_after;
            if existing.mode != "created" {
                existing.mode = incoming.mode.clone();
            }
            if existing.recovery_snapshot.is_none() {
                existing.recovery_snapshot = incoming.recovery_snapshot.clone();
            }
            if let Some(next_diff) = &incoming.diff_preview {
                let combined = match existing.diff_preview.take() {
                    Some(previous) if !previous.is_empty() => {
                        format!("{previous}\n… later edit …\n{next_diff}")
                    }
                    _ => next_diff.clone(),
                };
                existing.diff_preview = Some(combined.chars().take(12_000).collect());
            }
        } else {
            aggregate.files.push(incoming.clone());
        }
    }
    let mut validation_statement = transaction
        .prepare("SELECT command, status, COALESCE(output, ''), COALESCE(duration_ms, 0), created_at FROM verification_runs WHERE turn_id = ?1 ORDER BY created_at")
        .map_err(|error| error.to_string())?;
    aggregate.validation_results = validation_statement
        .query_map([&change_set.turn_id], |row| {
            Ok(VerificationResult {
                command: row.get(0)?,
                status: row.get(1)?,
                output: row.get(2)?,
                duration_ms: row.get::<_, i64>(3)?.max(0) as u64,
                created_at: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(validation_statement);
    aggregate
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    aggregate.added_lines = aggregate.files.iter().map(|file| file.added_lines).sum();
    aggregate.removed_lines = aggregate.files.iter().map(|file| file.removed_lines).sum();
    aggregate.recovery_available = aggregate
        .files
        .iter()
        .all(|file| file.recovery_snapshot.is_some());
    let payload = serde_json::to_string(&aggregate).map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO workspace_change_sets(id, conversation_id, turn_id, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json",
            params![id, conversation_id, change_set.turn_id, payload, Utc::now().to_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(aggregate)
}

/// Returns cached project summaries and their source metadata. Source contents
/// are never stored in this table.
pub fn load_project_index(
    db: &Db,
    workspace_id: &str,
) -> Result<std::collections::HashMap<String, (u64, u64, String)>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare("SELECT path, modified_ns, bytes, summary_json FROM workspace_project_index WHERE workspace_id = ?1")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
                row.get::<_, i64>(2)?.max(0) as u64,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    rows.map(|row| {
        let (path, modified_ns, bytes, summary) = row.map_err(|error| error.to_string())?;
        Ok((path, (modified_ns, bytes, summary)))
    })
    .collect()
}

/// Atomically replaces the current workspace map, pruning removed or newly
/// ignored paths while retaining only path metadata and compact summaries.
pub fn replace_project_index(
    db: &Db,
    workspace_id: &str,
    entries: &[(String, u64, u64, String)],
) -> Result<(), String> {
    let mut connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM workspace_project_index WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    {
        let mut insert = transaction
            .prepare("INSERT INTO workspace_project_index(workspace_id, path, modified_ns, bytes, summary_json) VALUES (?1, ?2, ?3, ?4, ?5)")
            .map_err(|error| error.to_string())?;
        for (path, modified_ns, bytes, summary) in entries {
            insert
                .execute(params![
                    workspace_id,
                    path,
                    *modified_ns as i64,
                    *bytes as i64,
                    summary
                ])
                .map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())
}

pub fn save_project_index_entry(
    db: &Db,
    workspace_id: &str,
    path: &str,
    modified_ns: u64,
    bytes: u64,
    summary: &str,
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO workspace_project_index(workspace_id, path, modified_ns, bytes, summary_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(workspace_id, path) DO UPDATE SET modified_ns = excluded.modified_ns, bytes = excluded.bytes, summary_json = excluded.summary_json",
            params![workspace_id, path, modified_ns as i64, bytes as i64, summary],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn remove_project_index_entry(db: &Db, workspace_id: &str, path: &str) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "DELETE FROM workspace_project_index WHERE workspace_id = ?1 AND path = ?2",
            params![workspace_id, path],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn save_verification_result(
    db: &Db,
    conversation_id: Option<&str>,
    turn_id: &str,
    result: &VerificationResult,
) -> Result<(), String> {
    let mut connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO verification_runs(id, conversation_id, turn_id, command, status, output, duration_ms, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                conversation_id,
                turn_id,
                result.command,
                result.status,
                result.output,
                result.duration_ms as i64,
                result.created_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    let existing: Option<(String, String)> = transaction
        .query_row(
            "SELECT id, payload_json FROM workspace_change_sets WHERE turn_id = ?1 ORDER BY created_at LIMIT 1",
            [turn_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((id, payload)) = existing {
        if let Ok(mut change_set) = serde_json::from_str::<WorkspaceChangeSet>(&payload) {
            change_set.validation_results.push(result.clone());
            transaction
                .execute(
                    "UPDATE workspace_change_sets SET payload_json = ?2 WHERE id = ?1",
                    params![
                        id,
                        serde_json::to_string(&change_set).map_err(|error| error.to_string())?
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())
}

pub fn agent_run_metrics(db: &Db, turn_id: &str) -> Result<AgentRunMetrics, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let (input_tokens, output_tokens): (Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT SUM(input_tokens), SUM(output_tokens) FROM usage_records WHERE turn_id = ?1",
            [turn_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let repair_attempts: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tool_argument_repairs WHERE turn_id = ?1",
            [turn_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let passed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM verification_runs WHERE turn_id = ?1 AND status = 'passed'",
            [turn_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let failed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM verification_runs WHERE turn_id = ?1 AND status IN ('failed', 'cancelled')",
            [turn_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload_json FROM workspace_change_sets WHERE turn_id = ?1 ORDER BY created_at LIMIT 1",
            [turn_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let changed_files = payload
        .as_deref()
        .and_then(|json| serde_json::from_str::<WorkspaceChangeSet>(json).ok())
        .map(|change_set| change_set.files.len() as u64)
        .unwrap_or_default();
    Ok(AgentRunMetrics {
        input_tokens: input_tokens.unwrap_or_default().max(0) as u64,
        output_tokens: output_tokens.unwrap_or_default().max(0) as u64,
        repair_attempts: repair_attempts.max(0) as u64,
        changed_files,
        checks_passed: passed.max(0) as u64,
        checks_failed: failed.max(0) as u64,
    })
}

pub fn save_context_measurement(
    db: &Db,
    conversation_id: Option<&str>,
    configured_length: u64,
    inspection: &crate::types::ContextInspection,
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO context_measurements(id, conversation_id, configured_length, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Uuid::new_v4().to_string(),
                conversation_id,
                configured_length as i64,
                serde_json::to_string(inspection).map_err(|e| e.to_string())?,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn plan_status_name(status: &PlanStatus) -> &'static str {
    match status {
        PlanStatus::Draft => "Draft",
        PlanStatus::AwaitingApproval => "AwaitingApproval",
        PlanStatus::Approved => "Approved",
        PlanStatus::Executing => "Executing",
        PlanStatus::Completed => "Completed",
        PlanStatus::Rejected => "Rejected",
        PlanStatus::Failed => "Failed",
        PlanStatus::Cancelled => "Cancelled",
    }
}

fn parse_plan_status(value: &str) -> PlanStatus {
    match value {
        "AwaitingApproval" => PlanStatus::AwaitingApproval,
        "Approved" => PlanStatus::Approved,
        "Executing" => PlanStatus::Executing,
        "Completed" => PlanStatus::Completed,
        "Rejected" => PlanStatus::Rejected,
        "Failed" => PlanStatus::Failed,
        "Cancelled" => PlanStatus::Cancelled,
        _ => PlanStatus::Draft,
    }
}

fn todo_status_name(status: &crate::types::TodoStatus) -> &'static str {
    match status {
        crate::types::TodoStatus::Pending => "Pending",
        crate::types::TodoStatus::InProgress => "InProgress",
        crate::types::TodoStatus::Completed => "Completed",
    }
}

fn parse_todo_status(value: &str) -> crate::types::TodoStatus {
    match value {
        "InProgress" => crate::types::TodoStatus::InProgress,
        "Completed" => crate::types::TodoStatus::Completed,
        _ => crate::types::TodoStatus::Pending,
    }
}

pub fn save_refactor_plan(db: &Db, plan: &RefactorPlan) -> Result<RefactorPlan, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "INSERT INTO refactor_plans(id, conversation_id, title, summary, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(id) DO UPDATE SET title = excluded.title, summary = excluded.summary, status = excluded.status, updated_at = excluded.updated_at",
            params![plan.id, plan.conversation_id, plan.title, plan.summary, plan_status_name(&plan.status), plan.created_at, plan.updated_at],
        )
        .map_err(|e| e.to_string())?;
    transaction
        .execute("DELETE FROM plan_steps WHERE plan_id = ?1", [&plan.id])
        .map_err(|e| e.to_string())?;
    for (position, step) in plan.steps.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO plan_steps(id, plan_id, description, paths_json, risk, verification_json, status, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![step.id, plan.id, step.description, serde_json::to_string(&step.paths).map_err(|e| e.to_string())?, step.risk, serde_json::to_string(&step.verification).map_err(|e| e.to_string())?, todo_status_name(&step.status), position as i64],
            )
            .map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(plan.clone())
}

pub fn get_refactor_plan(db: &Db, conversation_id: &str) -> Result<Option<RefactorPlan>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut plan = connection
        .query_row(
            "SELECT id, conversation_id, title, summary, status, created_at, updated_at FROM refactor_plans WHERE conversation_id = ?1 ORDER BY updated_at DESC LIMIT 1",
            [conversation_id],
            |row| Ok(RefactorPlan { id: row.get(0)?, conversation_id: row.get(1)?, title: row.get(2)?, summary: row.get(3)?, steps: Vec::new(), status: parse_plan_status(&row.get::<_, String>(4)?), created_at: row.get(5)?, updated_at: row.get(6)? }),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(ref mut plan_value) = plan else {
        return Ok(None);
    };
    let mut statement = connection
        .prepare("SELECT id, description, paths_json, risk, verification_json, status FROM plan_steps WHERE plan_id = ?1 ORDER BY position")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([&plan_value.id], |row| {
            let paths: String = row.get(2)?;
            let verification: String = row.get(4)?;
            Ok(PlanStep {
                id: row.get(0)?,
                description: row.get(1)?,
                paths: serde_json::from_str(&paths).unwrap_or_default(),
                risk: row.get(3)?,
                verification: serde_json::from_str(&verification).unwrap_or_default(),
                status: parse_todo_status(&row.get::<_, String>(5)?),
            })
        })
        .map_err(|e| e.to_string())?;
    plan_value.steps = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(plan)
}

pub fn set_refactor_plan_status(db: &Db, plan_id: &str, status: PlanStatus) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE refactor_plans SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![plan_status_name(&status), Utc::now().to_rfc3339(), plan_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Recover plans left in Executing by a process restart or crash. The caller
/// records the durable recovery event and the user can explicitly retry.
pub fn recover_interrupted_refactor_plans(db: &Db) -> Result<Vec<(String, String)>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare("SELECT id, conversation_id FROM refactor_plans WHERE status = 'Executing'")
        .map_err(|e| e.to_string())?;
    let plans = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<(String, String)>, _>>()
        .map_err(|e| e.to_string())?;
    if plans.is_empty() {
        return Ok(plans);
    }
    connection
        .execute(
            "UPDATE refactor_plans SET status = 'Failed', updated_at = ?1 WHERE status = 'Executing'",
            [Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
    Ok(plans)
}

pub fn replace_workspace_metadata(
    db: &Db,
    workspace_id: &str,
    entries: &[WorkspaceMetadata],
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "DELETE FROM workspace_metadata WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|e| e.to_string())?;
    for entry in entries {
        transaction.execute("INSERT INTO workspace_metadata(workspace_id, path, kind, bytes, modified_at, ignored, git_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![workspace_id, entry.path, entry.kind, entry.bytes as i64, entry.modified_at, entry.ignored as i64, entry.git_status]).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

pub fn list_workspace_metadata(
    db: &Db,
    workspace_id: &str,
    limit: usize,
) -> Result<Vec<WorkspaceMetadata>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection.prepare("SELECT path, kind, bytes, modified_at, ignored, git_status FROM workspace_metadata WHERE workspace_id = ?1 ORDER BY path LIMIT ?2").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![workspace_id, limit.min(50_000) as i64], |row| {
            Ok(WorkspaceMetadata {
                path: row.get(0)?,
                kind: row.get(1)?,
                bytes: row.get::<_, i64>(2)? as u64,
                modified_at: row.get(3)?,
                ignored: row.get::<_, i64>(4)? != 0,
                git_status: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn save_security_event(db: &Db, event: &SecurityEvent) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection.execute(
        "INSERT INTO security_events(id, conversation_id, tool_name, action, decision, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![Uuid::new_v4().to_string(), event.conversation_id, event.tool, event.action, event.decision, event.reason, Utc::now().to_rfc3339()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_security_events(db: &Db, limit: usize) -> Result<Vec<SecurityAuditRecord>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection.prepare(
        "SELECT id, conversation_id, tool_name, action, decision, reason, created_at FROM security_events ORDER BY created_at DESC LIMIT ?1",
    ).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([limit.min(500) as i64], |row| {
            Ok(SecurityAuditRecord {
                id: row.get(0)?,
                event: SecurityEvent {
                    conversation_id: row.get(1)?,
                    tool: row.get(2)?,
                    action: row.get(3)?,
                    decision: row.get(4)?,
                    reason: row.get(5)?,
                },
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn clear_session_approvals(db: &Db) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute("DELETE FROM session_approvals", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_benchmark(db: &Db, result: &BenchmarkResult) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection.execute(
        "INSERT INTO benchmark_runs(id, model, context_length, ttft_ms, tokens_per_second, total_ms, input_tokens, output_tokens, memory_before, memory_during, memory_after, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![Uuid::new_v4().to_string(), result.model, result.context_length as i64, result.ttft_ms as i64, result.tokens_per_second, result.total_ms as i64, result.input_tokens.map(|value| value as i64), result.output_tokens.map(|value| value as i64), result.memory_before as i64, result.memory_during.map(|value| value as i64), result.memory_after as i64, Utc::now().to_rfc3339()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
pub fn save_usage_record(
    db: &Db,
    provider_id: &str,
    model: &str,
    conversation_id: Option<&str>,
    turn_id: &str,
    usage: &UsageStats,
) -> Result<(), String> {
    save_usage_record_with_surface(
        db,
        provider_id,
        model,
        "unassigned",
        conversation_id,
        turn_id,
        usage,
    )
}

pub fn save_usage_record_with_surface(
    db: &Db,
    provider_id: &str,
    model: &str,
    surface: &str,
    conversation_id: Option<&str>,
    turn_id: &str,
    usage: &UsageStats,
) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection.execute(
        "INSERT INTO usage_records(id, provider_id, model, surface, conversation_id, turn_id, input_tokens, cached_input_tokens, cache_miss_tokens, cache_write_tokens, output_tokens, total_tokens, ttft_ms, total_ms, tokens_per_second, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            Uuid::new_v4().to_string(), provider_id, model,
            match surface { "ide" => "ide", "assistant" => "assistant", "other" => "other", _ => "unassigned" },
            conversation_id, turn_id,
            usage.input_tokens.map(|value| value as i64),
            usage.cached_input_tokens.map(|value| value as i64),
            usage.cache_miss_tokens.map(|value| value as i64),
            usage.cache_write_tokens.map(|value| value as i64),
            usage.output_tokens.map(|value| value as i64),
            usage.total_tokens.map(|value| value as i64),
            usage.ttft_ms.map(|value| value as i64),
            usage.total_ms.map(|value| value as i64),
            usage.tokens_per_second,
            Utc::now().to_rfc3339(),
        ],
    ).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn start_hku_request_attempt(db: &Db, id: &str, model: &str) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO hku_request_attempts(id, model, status, created_at) VALUES (?1, ?2, 'started', ?3)",
            params![id, model, Utc::now().to_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn finish_hku_request_attempt(
    db: &Db,
    id: &str,
    status: &str,
    http_status: Option<u16>,
) -> Result<(), String> {
    if !matches!(status, "succeeded" | "failed" | "cancelled") {
        return Err("invalid HKU request-attempt status".into());
    }
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "UPDATE hku_request_attempts SET status = ?1, http_status = ?2, finished_at = ?3 WHERE id = ?4",
            params![status, http_status.map(i64::from), Utc::now().to_rfc3339(), id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn get_hku_request_usage(connection: &Connection) -> Result<Vec<HkuRequestUsage>, String> {
    let now = Utc::now();
    let minute_ago = (now - Duration::minutes(1)).to_rfc3339();
    let day_ago = (now - Duration::hours(24)).to_rfc3339();
    let week_ago = (now - Duration::days(7)).to_rfc3339();
    let mut statement = connection
        .prepare(
            "SELECT model, SUM(CASE WHEN created_at >= ?1 THEN 1 ELSE 0 END), SUM(CASE WHEN created_at >= ?2 THEN 1 ELSE 0 END), SUM(CASE WHEN created_at >= ?3 THEN 1 ELSE 0 END), SUM(CASE WHEN created_at >= ?2 AND status = 'failed' THEN 1 ELSE 0 END) FROM hku_request_attempts GROUP BY model ORDER BY model",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![minute_ago, day_ago, week_ago], |row| {
            Ok(HkuRequestUsage {
                model: row.get(0)?,
                last_minute: row.get::<_, i64>(1)?.max(0) as u64,
                last_day: row.get::<_, i64>(2)?.max(0) as u64,
                last_week: row.get::<_, i64>(3)?.max(0) as u64,
                failed_last_day: row.get::<_, i64>(4)?.max(0) as u64,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub fn get_usage_summary(db: &Db) -> Result<UsageSummary, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let totals = query_usage_totals(&connection, None, None)?;
    let by_model = query_usage_breakdowns(&connection, None)?;
    let month_start = (Utc::now() - Duration::days(30)).to_rfc3339();
    let month_by_model = query_usage_breakdowns(&connection, Some(&month_start))?;
    let mut recent_statement = connection.prepare(
        "SELECT id, provider_id, model, surface, conversation_id, turn_id, input_tokens, cached_input_tokens, cache_miss_tokens, cache_write_tokens, output_tokens, total_tokens, ttft_ms, total_ms, tokens_per_second, created_at FROM usage_records ORDER BY created_at DESC LIMIT 30",
    ).map_err(|error| error.to_string())?;
    let recent = recent_statement
        .query_map([], |row| {
            let optional_u64 = |index| -> rusqlite::Result<Option<u64>> {
                Ok(row
                    .get::<_, Option<i64>>(index)?
                    .map(|value| value.max(0) as u64))
            };
            Ok(UsageRecord {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                model: row.get(2)?,
                surface: row.get(3)?,
                conversation_id: row.get(4)?,
                turn_id: row.get(5)?,
                usage: UsageStats {
                    input_tokens: optional_u64(6)?,
                    cached_input_tokens: optional_u64(7)?,
                    cache_miss_tokens: optional_u64(8)?,
                    cache_write_tokens: optional_u64(9)?,
                    output_tokens: optional_u64(10)?,
                    total_tokens: optional_u64(11)?,
                    ttft_ms: optional_u64(12)?,
                    total_ms: optional_u64(13)?,
                    tokens_per_second: row.get(14)?,
                },
                created_at: row.get(15)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(UsageSummary {
        totals,
        by_model,
        month_by_model,
        recent,
        hku_requests: get_hku_request_usage(&connection)?,
    })
}

fn query_usage_breakdowns(
    connection: &Connection,
    since: Option<&str>,
) -> Result<Vec<UsageModelBreakdown>, String> {
    let mut statement = connection
        .prepare("SELECT surface, provider_id, model, COUNT(*), SUM(input_tokens), SUM(output_tokens), SUM(total_tokens), SUM(cached_input_tokens), SUM(cache_miss_tokens), SUM(cache_write_tokens), SUM(CASE WHEN cached_input_tokens IS NOT NULL OR cache_miss_tokens IS NOT NULL OR cache_write_tokens IS NOT NULL THEN 1 ELSE 0 END), SUM(CASE WHEN input_tokens IS NOT NULL THEN 1 ELSE 0 END), SUM(CASE WHEN output_tokens IS NOT NULL THEN 1 ELSE 0 END), SUM(CASE WHEN total_tokens IS NOT NULL THEN 1 ELSE 0 END) FROM usage_records WHERE (?1 IS NULL OR created_at >= ?1) GROUP BY surface, provider_id, model ORDER BY MAX(created_at) DESC")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![since], |row| {
            let cache_reported: i64 = row.get(10)?;
            let input_reported: i64 = row.get(11)?;
            let output_reported: i64 = row.get(12)?;
            let total_reported: i64 = row.get(13)?;
            let input: Option<i64> = row.get(4)?;
            let output: Option<i64> = row.get(5)?;
            let total: Option<i64> = row.get(6)?;
            let cached: Option<i64> = row.get(7)?;
            let missed: Option<i64> = row.get(8)?;
            let written: Option<i64> = row.get(9)?;
            Ok(UsageModelBreakdown {
                surface: row.get(0)?,
                provider_id: row.get(1)?,
                model: row.get(2)?,
                totals: UsageTotals {
                    requests: row.get::<_, i64>(3)?.max(0) as u64,
                    input_tokens: (input_reported > 0).then(|| input.unwrap_or(0).max(0) as u64),
                    output_tokens: (output_reported > 0).then(|| output.unwrap_or(0).max(0) as u64),
                    total_tokens: (total_reported > 0).then(|| total.unwrap_or(0).max(0) as u64),
                    input_metrics_reported_requests: input_reported.max(0) as u64,
                    output_metrics_reported_requests: output_reported.max(0) as u64,
                    total_metrics_reported_requests: total_reported.max(0) as u64,
                    cached_input_tokens: (cache_reported > 0)
                        .then(|| cached.unwrap_or(0).max(0) as u64),
                    cache_miss_tokens: (cache_reported > 0)
                        .then(|| missed.unwrap_or(0).max(0) as u64),
                    cache_write_tokens: (cache_reported > 0)
                        .then(|| written.unwrap_or(0).max(0) as u64),
                    cache_metrics_reported_requests: cache_reported.max(0) as u64,
                },
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn query_usage_totals(
    connection: &Connection,
    provider_id: Option<&str>,
    model: Option<&str>,
) -> Result<UsageTotals, String> {
    let (requests, input, output, total, cached, missed, written, cache_reported, input_reported, output_reported, total_reported): (i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>, i64, i64, i64, i64) = connection.query_row(
        "SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens), SUM(total_tokens), SUM(cached_input_tokens), SUM(cache_miss_tokens), SUM(cache_write_tokens), COALESCE(SUM(CASE WHEN cached_input_tokens IS NOT NULL OR cache_miss_tokens IS NOT NULL OR cache_write_tokens IS NOT NULL THEN 1 ELSE 0 END), 0), COALESCE(SUM(CASE WHEN input_tokens IS NOT NULL THEN 1 ELSE 0 END), 0), COALESCE(SUM(CASE WHEN output_tokens IS NOT NULL THEN 1 ELSE 0 END), 0), COALESCE(SUM(CASE WHEN total_tokens IS NOT NULL THEN 1 ELSE 0 END), 0) FROM usage_records WHERE (?1 IS NULL OR provider_id = ?1) AND (?2 IS NULL OR model = ?2)",
        params![provider_id, model],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?)),
    ).map_err(|error| error.to_string())?;
    Ok(UsageTotals {
        requests: requests.max(0) as u64,
        input_tokens: (input_reported > 0).then(|| input.unwrap_or(0).max(0) as u64),
        cached_input_tokens: (cache_reported > 0).then(|| cached.unwrap_or(0).max(0) as u64),
        cache_miss_tokens: (cache_reported > 0).then(|| missed.unwrap_or(0).max(0) as u64),
        cache_write_tokens: (cache_reported > 0).then(|| written.unwrap_or(0).max(0) as u64),
        output_tokens: (output_reported > 0).then(|| output.unwrap_or(0).max(0) as u64),
        total_tokens: (total_reported > 0).then(|| total.unwrap_or(0).max(0) as u64),
        input_metrics_reported_requests: input_reported.max(0) as u64,
        output_metrics_reported_requests: output_reported.max(0) as u64,
        total_metrics_reported_requests: total_reported.max(0) as u64,
        cache_metrics_reported_requests: cache_reported.max(0) as u64,
    })
}

pub fn set_setting(db: &Db, key: &str, value: &Value) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    connection
        .execute(
            "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
            params![key, serde_json::to_string(value).map_err(|e| e.to_string())?, Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_settings(db: &Db) -> Result<Value, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare("SELECT key, value_json FROM settings")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })
        .map_err(|e| e.to_string())?;
    let mut object = serde_json::Map::new();
    for row in rows {
        let (key, value) = row.map_err(|e| e.to_string())?;
        if let Ok(parsed) = serde_json::from_str(&value) {
            object.insert(key, parsed);
        }
    }
    Ok(Value::Object(object))
}

pub fn list_market_watchlists(db: &Db) -> Result<Vec<Watchlist>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, symbols_json, updated_at FROM market_watchlists ORDER BY name ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let symbols_json: String = row.get(2)?;
            Ok(Watchlist {
                id: row.get(0)?,
                name: row.get(1)?,
                symbols: serde_json::from_str(&symbols_json).unwrap_or_default(),
                updated_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn save_market_watchlist(db: &Db, watchlist: &Watchlist) -> Result<Watchlist, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let updated_at = Utc::now().to_rfc3339();
    connection
        .execute(
            "INSERT INTO market_watchlists(id, name, symbols_json, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, symbols_json = excluded.symbols_json, updated_at = excluded.updated_at",
            params![
                watchlist.id,
                watchlist.name,
                serde_json::to_string(&watchlist.symbols).map_err(|e| e.to_string())?,
                updated_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(Watchlist {
        updated_at,
        ..watchlist.clone()
    })
}

pub fn ensure_market_watchlist(db: &Db) -> Result<Vec<Watchlist>, String> {
    let existing = list_market_watchlists(db)?;
    if existing.is_empty() {
        save_market_watchlist(
            db,
            &Watchlist {
                id: Uuid::new_v4().to_string(),
                name: "Main".into(),
                symbols: Vec::new(),
                updated_at: Utc::now().to_rfc3339(),
            },
        )?;
    }
    list_market_watchlists(db)
}

pub fn list_market_holdings(db: &Db) -> Result<Vec<Holding>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare("SELECT id, account, symbol, shares, average_cost, currency, source, updated_at FROM market_holdings ORDER BY account, symbol")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Holding {
                id: row.get(0)?,
                account: row.get(1)?,
                symbol: row.get(2)?,
                shares: row.get(3)?,
                average_cost: row.get(4)?,
                currency: row.get(5)?,
                source: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn save_market_holding(db: &Db, holding: &Holding) -> Result<Holding, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let updated_at = Utc::now().to_rfc3339();
    connection
        .execute(
            "INSERT INTO market_holdings(id, account, symbol, shares, average_cost, currency, source, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(account, symbol) DO UPDATE SET shares = excluded.shares, average_cost = excluded.average_cost, currency = excluded.currency, source = excluded.source, updated_at = excluded.updated_at",
            params![
                holding.id,
                holding.account,
                holding.symbol,
                holding.shares,
                holding.average_cost,
                holding.currency,
                holding.source,
                updated_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(Holding {
        updated_at,
        ..holding.clone()
    })
}

pub fn save_market_order(
    db: &Db,
    preview: &crate::markets::OrderPreview,
) -> Result<String, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let id = Uuid::new_v4().to_string();
    connection
        .execute(
            "INSERT INTO market_orders(id, symbol, side, quantity, order_type, limit_price, mode, status, preview_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                preview.symbol,
                preview.side,
                preview.quantity,
                preview.order_type,
                preview.limit_price,
                preview.trading_mode,
                "PAPER_SUBMITTED",
                serde_json::to_string(preview).map_err(|e| e.to_string())?,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn list_market_orders(db: &Db) -> Result<Vec<MarketOrder>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, symbol, side, quantity, order_type, limit_price, mode, status, created_at FROM market_orders ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(MarketOrder {
                id: row.get(0)?,
                symbol: row.get(1)?,
                side: row.get(2)?,
                quantity: row.get(3)?,
                order_type: row.get(4)?,
                limit_price: row.get(5)?,
                mode: row.get(6)?,
                status: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn list_market_trades(db: &Db) -> Result<Vec<PortfolioTrade>, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, account, symbol, side, quantity, price, commission, trade_date, notes, source, created_at
             FROM market_trades ORDER BY trade_date DESC, created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(PortfolioTrade {
                id: row.get(0)?,
                account: row.get(1)?,
                symbol: row.get(2)?,
                side: row.get(3)?,
                quantity: row.get(4)?,
                price: row.get(5)?,
                commission: row.get(6)?,
                trade_date: row.get(7)?,
                notes: row.get(8)?,
                source: row.get(9)?,
                created_at: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn save_market_trade(db: &Db, trade: &PortfolioTrade) -> Result<PortfolioTrade, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let created_at = Utc::now().to_rfc3339();
    connection
        .execute(
            "INSERT INTO market_trades(id, account, symbol, side, quantity, price, commission, trade_date, notes, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                trade.id,
                trade.account,
                trade.symbol,
                trade.side,
                trade.quantity,
                trade.price,
                trade.commission,
                trade.trade_date,
                trade.notes,
                trade.source,
                created_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(PortfolioTrade {
        created_at,
        ..trade.clone()
    })
}

pub fn update_market_trade(db: &Db, trade: &PortfolioTrade) -> Result<PortfolioTrade, String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let updated = connection
        .execute(
            "UPDATE market_trades
             SET account = ?2, symbol = ?3, side = ?4, quantity = ?5, price = ?6,
                 commission = ?7, trade_date = ?8, notes = ?9, source = ?10
             WHERE id = ?1",
            params![
                trade.id,
                trade.account,
                trade.symbol,
                trade.side,
                trade.quantity,
                trade.price,
                trade.commission,
                trade.trade_date,
                trade.notes,
                trade.source,
            ],
        )
        .map_err(|e| e.to_string())?;
    if updated == 0 {
        return Err("Transaction was not found in the local ledger.".into());
    }
    let created_at = connection
        .query_row(
            "SELECT created_at FROM market_trades WHERE id = ?1",
            params![trade.id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(PortfolioTrade {
        created_at,
        ..trade.clone()
    })
}

pub fn delete_market_trade(db: &Db, id: &str) -> Result<(), String> {
    let connection = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let deleted = connection
        .execute("DELETE FROM market_trades WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if deleted == 0 {
        return Err("Transaction was not found in the local ledger.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_and_round_trip_messages() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Test".into())).expect("conversation");
        save_message(
            &db,
            &conversation.id,
            None,
            "user",
            "hello",
            None,
            None,
            None,
        )
        .expect("message");
        let messages = list_messages(&db, &conversation.id).expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn usage_ledger_aggregates_by_model_without_fabricating_cache_metrics() {
        let db = init_db(Path::new(":memory:")).expect("db");
        save_usage_record(
            &db,
            "hku",
            "gpt-5-mini",
            None,
            "turn-1",
            &UsageStats {
                input_tokens: Some(100),
                output_tokens: Some(20),
                total_tokens: Some(120),
                ..Default::default()
            },
        )
        .expect("first usage row");
        save_usage_record(
            &db,
            "hku",
            "gpt-5-mini",
            None,
            "turn-2",
            &UsageStats {
                input_tokens: Some(80),
                cached_input_tokens: Some(30),
                cache_miss_tokens: Some(50),
                output_tokens: Some(10),
                total_tokens: Some(90),
                ..Default::default()
            },
        )
        .expect("second usage row");

        let summary = get_usage_summary(&db).expect("summary");
        assert_eq!(summary.totals.requests, 2);
        assert_eq!(summary.totals.input_tokens, Some(180));
        assert_eq!(summary.totals.output_tokens, Some(30));
        assert_eq!(summary.totals.cached_input_tokens, Some(30));
        assert_eq!(summary.totals.cache_miss_tokens, Some(50));
        assert_eq!(summary.totals.cache_metrics_reported_requests, 1);
        assert_eq!(summary.by_model.len(), 1);
        assert_eq!(summary.by_model[0].model, "gpt-5-mini");
        assert_eq!(summary.recent.len(), 2);
        assert!(summary
            .recent
            .iter()
            .all(|record| record.conversation_id.is_none()));
    }

    #[test]
    fn usage_ledger_keeps_unreported_token_counts_unknown() {
        let db = init_db(Path::new(":memory:")).expect("db");
        save_usage_record(
            &db,
            "hku",
            "gemini-test",
            None,
            "turn-1",
            &UsageStats::default(),
        )
        .expect("usage row");
        let summary = get_usage_summary(&db).expect("summary");
        assert_eq!(summary.totals.requests, 1);
        assert_eq!(summary.totals.input_tokens, None);
        assert_eq!(summary.totals.output_tokens, None);
        assert_eq!(summary.totals.total_tokens, None);
        assert_eq!(summary.by_model[0].totals.input_tokens, None);
    }

    #[test]
    fn hku_request_tracker_counts_attempts_in_rolling_windows_without_payloads() {
        let db = init_db(Path::new(":memory:")).expect("db");
        start_hku_request_attempt(&db, "attempt-1", "claude-opus-4.6").expect("start");
        finish_hku_request_attempt(&db, "attempt-1", "succeeded", Some(200)).expect("finish");
        start_hku_request_attempt(&db, "attempt-2", "claude-opus-4.6").expect("start");
        finish_hku_request_attempt(&db, "attempt-2", "failed", Some(500)).expect("finish");

        let summary = get_usage_summary(&db).expect("summary");
        assert_eq!(summary.hku_requests.len(), 1);
        assert_eq!(summary.hku_requests[0].model, "claude-opus-4.6");
        assert_eq!(summary.hku_requests[0].last_minute, 2);
        assert_eq!(summary.hku_requests[0].last_day, 2);
        assert_eq!(summary.hku_requests[0].last_week, 2);
        assert_eq!(summary.hku_requests[0].failed_last_day, 1);
        // Request tracking stores only model/timestamps/status, not prompt content.
        let connection = db.lock().expect("connection");
        let columns: Vec<String> = connection
            .prepare("PRAGMA table_info(hku_request_attempts)")
            .expect("schema")
            .query_map([], |row| row.get(1))
            .expect("columns")
            .filter_map(Result::ok)
            .collect();
        assert!(!columns.iter().any(|column| column.contains("prompt")));
    }

    #[test]
    fn market_trade_round_trip_preserves_date_and_commission() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let saved = save_market_trade(
            &db,
            &PortfolioTrade {
                id: "trade-1".into(),
                account: "Example account".into(),
                symbol: "HK.12345".into(),
                side: "BUY".into(),
                quantity: 10.0,
                price: 12.34,
                commission: 0.1,
                trade_date: "2024-01-02".into(),
                notes: "Example trade".into(),
                source: "manual".into(),
                created_at: "".into(),
            },
        )
        .expect("trade");
        let trades = list_market_trades(&db).expect("trades");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].trade_date, "2024-01-02");
        assert_eq!(trades[0].commission, 0.1);
        assert_eq!(saved.id, trades[0].id);
        let mut edited = saved.clone();
        edited.quantity = 11.0;
        edited.price = 15.67;
        edited.commission = 0.2;
        edited.trade_date = "2024-01-03".into();
        edited.notes = "Updated example trade".into();
        let updated = update_market_trade(&db, &edited).expect("update trade");
        assert_eq!(updated.id, saved.id);
        assert_eq!(updated.created_at, saved.created_at);
        let edited_trades = list_market_trades(&db).expect("edited trades");
        assert_eq!(edited_trades.len(), 1);
        assert_eq!(edited_trades[0].quantity, 11.0);
        assert_eq!(edited_trades[0].price, 15.67);
        assert_eq!(edited_trades[0].commission, 0.2);
        assert_eq!(edited_trades[0].trade_date, "2024-01-03");
        assert_eq!(edited_trades[0].notes, "Updated example trade");
        delete_market_trade(&db, &saved.id).expect("delete trade");
        assert!(list_market_trades(&db)
            .expect("trades after delete")
            .is_empty());
    }

    #[test]
    fn editing_a_message_preserves_the_old_tail_and_switches_active_branch() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Test".into())).expect("conversation");
        let first = save_message(
            &db,
            &conversation.id,
            None,
            "user",
            "old question",
            None,
            None,
            None,
        )
        .expect("first message");
        save_message(
            &db,
            &conversation.id,
            None,
            "assistant",
            "old answer",
            None,
            None,
            None,
        )
        .expect("old answer");

        let edited = edit_message(&db, &conversation.id, &first.id, "new question").expect("edit");
        assert_eq!(edited.content, "new question");
        let active = list_messages(&db, &conversation.id).expect("active messages");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].content, "new question");

        let connection = db.lock().expect("lock");
        let raw_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
                [&conversation.id],
                |row| row.get(0),
            )
            .expect("raw count");
        let edit_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM message_edits", [], |row| row.get(0))
            .expect("edit count");
        assert_eq!(raw_count, 2);
        assert_eq!(edit_count, 1);
    }

    #[test]
    fn message_images_round_trip_without_leaving_the_database() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Images".into())).expect("conversation");
        save_message_with_attachments(
            &db,
            &conversation.id,
            None,
            "user",
            "what is this?",
            None,
            None,
            None,
            Some(&[ImageInput {
                name: "sample.png".into(),
                mime_type: "image/png".into(),
                data_url: "data:image/png;base64,AA==".into(),
                size_bytes: 1,
            }]),
        )
        .expect("image message");
        let messages = list_messages(&db, &conversation.id).expect("messages");
        assert_eq!(messages[0].images[0].name, "sample.png");
        assert_eq!(messages[0].images[0].mime_type, "image/png");
    }

    #[test]
    fn chats_and_workspaces_support_pin_archive_and_delete() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Lifecycle".into())).expect("chat");
        set_conversation_pinned(&db, &conversation.id, true).expect("pin chat");
        set_conversation_archived(&db, &conversation.id, true).expect("archive chat");
        let listed = list_conversations(&db).expect("list chats");
        assert!(listed[0].pinned);
        assert!(listed[0].archived);

        let workspace = add_workspace(&db, "Project", "/tmp").expect("workspace");
        set_workspace_pinned(&db, &workspace.id, true).expect("pin workspace");
        set_workspace_archived(&db, &workspace.id, true).expect("archive workspace");
        rename_workspace(&db, &workspace.id, "Renamed").expect("rename workspace");
        let listed_workspaces = list_workspaces(&db).expect("list workspaces");
        assert!(listed_workspaces[0].pinned);
        assert!(listed_workspaces[0].archived);
        assert_eq!(listed_workspaces[0].name, "Renamed");

        delete_conversation(&db, &conversation.id).expect("delete chat");
        remove_workspace(&db, &workspace.id).expect("delete workspace");
        assert!(list_conversations(&db).expect("remaining chats").is_empty());
        assert!(list_workspaces(&db)
            .expect("remaining workspaces")
            .is_empty());
    }

    #[test]
    fn conversation_summaries_keep_their_workspace_association() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Independent chat".into())).expect("chat");
        assert_eq!(conversation.workspace_id, None);

        let workspace = add_workspace(&db, "Project", "/tmp/project").expect("workspace");
        set_conversation_workspace(&db, &conversation.id, Some(&workspace.id)).expect("associate");
        let listed = list_conversations(&db).expect("list");
        assert_eq!(
            listed[0].workspace_id.as_deref(),
            Some(workspace.id.as_str())
        );

        set_conversation_workspace(&db, &conversation.id, None).expect("detach");
        let listed = list_conversations(&db).expect("list detached chat");
        assert_eq!(listed[0].workspace_id, None);
    }

    #[test]
    fn plans_events_and_workspace_metadata_round_trip() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Refactor".into())).expect("chat");
        let plan = RefactorPlan {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            title: "Update app".into(),
            summary: "Coordinate two files and verify the build.".into(),
            steps: vec![PlanStep {
                id: "step-1".into(),
                description: "Update the entry point".into(),
                paths: vec!["src/main.rs".into()],
                risk: "low".into(),
                verification: vec!["cargo test".into()],
                status: crate::types::TodoStatus::Pending,
            }],
            status: PlanStatus::AwaitingApproval,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        save_refactor_plan(&db, &plan).expect("plan");
        let loaded = get_refactor_plan(&db, &conversation.id)
            .expect("load plan")
            .expect("plan exists");
        assert_eq!(loaded.title, plan.title);
        assert_eq!(loaded.steps[0].verification, vec!["cargo test"]);

        save_session_event(
            &db,
            Some(&conversation.id),
            Some("turn-1"),
            "plan_updated",
            &serde_json::to_value(&plan).expect("json"),
        )
        .expect("event");
        assert_eq!(
            list_session_events(&db, Some(&conversation.id), 10)
                .expect("events")
                .len(),
            1
        );

        let workspace = add_workspace(&db, "Project", "/tmp").expect("workspace");
        replace_workspace_metadata(
            &db,
            &workspace.id,
            &[WorkspaceMetadata {
                path: "src/main.rs".into(),
                kind: "file".into(),
                bytes: 10,
                modified_at: "1".into(),
                ignored: false,
                git_status: None,
            }],
        )
        .expect("metadata");
        assert_eq!(
            list_workspace_metadata(&db, &workspace.id, 10).expect("metadata list")[0].path,
            "src/main.rs"
        );
    }

    #[test]
    fn interrupted_executing_plans_are_recovered_as_failed() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Interrupted".into())).expect("chat");
        let plan = RefactorPlan {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            title: "Interrupted plan".into(),
            summary: "A plan that was active during a restart.".into(),
            steps: Vec::new(),
            status: PlanStatus::Executing,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        save_refactor_plan(&db, &plan).expect("plan");
        let recovered = recover_interrupted_refactor_plans(&db).expect("recover");
        assert_eq!(recovered, vec![(plan.id.clone(), conversation.id.clone())]);
        assert_eq!(
            get_refactor_plan(&db, &conversation.id)
                .expect("load")
                .expect("plan")
                .status,
            PlanStatus::Failed
        );
    }

    #[test]
    fn offline_eval_merges_multi_file_edits_and_persists_validation() {
        let db = init_db(Path::new(":memory:")).expect("db");
        let conversation = create_conversation(&db, Some("Offline refactor".into())).expect("chat");
        let first = WorkspaceChangeSet {
            turn_id: "offline-run".into(),
            files: vec![crate::types::WorkspaceFileChange {
                path: "src/notes.ts".into(),
                mode: "modified".into(),
                added_lines: 2,
                removed_lines: 1,
                bytes_before: 100,
                bytes_after: 120,
                recovery_snapshot: Some("snap-1".into()),
                diff_preview: Some("-old\n+new".into()),
                ..Default::default()
            }],
            recovery_available: true,
            ..Default::default()
        };
        save_workspace_change_set(&db, Some(&conversation.id), &first).expect("first edit");
        let second = WorkspaceChangeSet {
            turn_id: "offline-run".into(),
            files: vec![
                crate::types::WorkspaceFileChange {
                    path: "src/notes.ts".into(),
                    mode: "modified".into(),
                    added_lines: 1,
                    removed_lines: 2,
                    bytes_before: 120,
                    bytes_after: 110,
                    recovery_snapshot: Some("snap-2".into()),
                    diff_preview: Some("-new\n+final".into()),
                    ..Default::default()
                },
                crate::types::WorkspaceFileChange {
                    path: "src/store.ts".into(),
                    mode: "created".into(),
                    added_lines: 20,
                    bytes_after: 400,
                    recovery_snapshot: Some("snap-3".into()),
                    diff_preview: Some("+store".into()),
                    ..Default::default()
                },
            ],
            recovery_available: true,
            ..Default::default()
        };
        let merged =
            save_workspace_change_set(&db, Some(&conversation.id), &second).expect("merge");
        assert_eq!(merged.files.len(), 2);
        assert_eq!(merged.files[0].bytes_before, 100);
        assert_eq!(merged.files[0].bytes_after, 110);
        assert_eq!(merged.files[0].added_lines, 3);
        assert_eq!(merged.files[0].removed_lines, 3);
        assert_eq!(merged.added_lines, 23);
        assert!(merged.files[0]
            .diff_preview
            .as_deref()
            .unwrap()
            .contains("later edit"));

        let validation = VerificationResult {
            command: "npm test".into(),
            status: "passed".into(),
            output: "8 tests passed".into(),
            duration_ms: 120,
            created_at: Utc::now().to_rfc3339(),
        };
        save_verification_result(&db, Some(&conversation.id), "offline-run", &validation)
            .expect("save validation");
        save_usage_record_with_surface(
            &db,
            "local",
            "fixture-model",
            "assistant",
            Some(&conversation.id),
            "offline-run",
            &crate::types::UsageStats {
                input_tokens: Some(10),
                output_tokens: Some(5),
                ..Default::default()
            },
        )
        .expect("save usage");
        save_tool_argument_repair(
            &db,
            Some(&conversation.id),
            "offline-run",
            &ToolArgumentRepair {
                tool_name: "filesystem_write_many".into(),
                original: "{}".into(),
                normalized: serde_json::json!({}),
                reason: "fixture normalization".into(),
                rejected: false,
            },
        )
        .expect("save repair count");
        let metrics = agent_run_metrics(&db, "offline-run").expect("run metrics");
        assert_eq!(metrics.input_tokens, 10);
        assert_eq!(metrics.output_tokens, 5);
        assert_eq!(metrics.repair_attempts, 1);
        assert_eq!(metrics.changed_files, 2);
        assert_eq!(metrics.checks_passed, 1);
        assert_eq!(metrics.checks_failed, 0);
        let connection = db.lock().expect("lock");
        let payload: String = connection
            .query_row(
                "SELECT payload_json FROM workspace_change_sets WHERE turn_id = 'offline-run'",
                [],
                |row| row.get(0),
            )
            .expect("saved change set");
        let saved: WorkspaceChangeSet = serde_json::from_str(&payload).expect("payload");
        assert_eq!(saved.validation_results[0].status, "passed");
        assert_eq!(saved.validation_results[0].output, "8 tests passed");
    }
}
