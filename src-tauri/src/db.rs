use std::{path::Path, sync::Mutex};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    models::{
        AiProposal, AppSettings, CreateFeedInput, CreateFeedResult, EncryptedSecretRecord,
        FeedEvent, FeishuPlanTaskMapping, FeishuSheetState, MemoryDetail, MemorySummary,
        MemoryVersion, PlanItem, PlanProposal, ReviewItem, Stats, VaultMeta,
    },
};

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        let database = Self {
            connection: Mutex::new(connection),
        };
        database.migrate()?;
        Ok(database)
    }

    #[cfg(test)]
    pub fn in_memory() -> AppResult<Self> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let database = Self {
            connection: Mutex::new(connection),
        };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&self) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS feed_events (
                id TEXT PRIMARY KEY,
                raw_content TEXT NOT NULL,
                content_type TEXT NOT NULL DEFAULT 'text',
                source_type TEXT NOT NULL DEFAULT 'manual',
                source_metadata TEXT,
                processing_status TEXT NOT NULL DEFAULT 'pending',
                created_at INTEGER NOT NULL,
                deleted_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                memory_type TEXT NOT NULL DEFAULT 'Unclassified',
                current_version_id TEXT,
                lifecycle_status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                archived_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS memory_versions (
                id TEXT PRIMARY KEY,
                memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                summary TEXT,
                structured_data TEXT NOT NULL DEFAULT '{}',
                confidence REAL NOT NULL,
                author_type TEXT NOT NULL,
                model_info TEXT,
                change_reason TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memory_version_sources (
                memory_version_id TEXT NOT NULL REFERENCES memory_versions(id) ON DELETE CASCADE,
                feed_event_id TEXT NOT NULL REFERENCES feed_events(id) ON DELETE CASCADE,
                evidence_role TEXT NOT NULL DEFAULT 'primary',
                PRIMARY KEY (memory_version_id, feed_event_id)
            );

            CREATE TABLE IF NOT EXISTS memory_relations (
                id TEXT PRIMARY KEY,
                from_memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                to_memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                relation_type TEXT NOT NULL,
                confidence REAL NOT NULL,
                status TEXT NOT NULL DEFAULT 'suggested',
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS processing_runs (
                id TEXT PRIMARY KEY,
                feed_event_id TEXT NOT NULL REFERENCES feed_events(id) ON DELETE CASCADE,
                pipeline_version TEXT NOT NULL,
                model_info TEXT,
                input_hash TEXT,
                output_json TEXT,
                status TEXT NOT NULL,
                error_code TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS review_items (
                id TEXT PRIMARY KEY,
                feed_event_id TEXT REFERENCES feed_events(id) ON DELETE CASCADE,
                proposed_action TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                risk_level TEXT NOT NULL,
                reason TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at INTEGER NOT NULL,
                resolved_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                actor_type TEXT NOT NULL,
                action TEXT NOT NULL,
                target_type TEXT NOT NULL,
                target_id TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS plans (
                id TEXT PRIMARY KEY,
                feed_event_id TEXT NOT NULL REFERENCES feed_events(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                details TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                link_url TEXT,
                notes TEXT,
                scheduled_at INTEGER,
                status TEXT NOT NULL,
                clarification_question TEXT,
                source_title TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                reminded_at INTEGER,
                feishu_synced_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS feishu_source_rows (
                source_key TEXT PRIMARY KEY,
                row_hash TEXT NOT NULL,
                plan_id TEXT REFERENCES plans(id) ON DELETE SET NULL,
                last_seen_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS feishu_plan_cleanup_queue (
                plan_id TEXT PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS feishu_plan_tasks (
                plan_id TEXT PRIMARY KEY,
                task_guid TEXT NOT NULL,
                task_url TEXT,
                plan_updated_at INTEGER NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS vault_meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                salt BLOB NOT NULL,
                verifier_nonce BLOB NOT NULL,
                verifier_ciphertext BLOB NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS secret_records (
                id TEXT PRIMARY KEY,
                nonce BLOB NOT NULL,
                ciphertext BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                feishu_synced_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS feishu_secret_cleanup_queue (
                secret_id TEXT PRIMARY KEY
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                version_id UNINDEXED,
                memory_id UNINDEXED,
                title,
                body,
                summary,
                tokenize = 'unicode61'
            );

            CREATE INDEX IF NOT EXISTS idx_feed_created ON feed_events(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_versions_memory ON memory_versions(memory_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_review_status ON review_items(status, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_plans_status_time ON plans(status, scheduled_at, created_at DESC);

            INSERT OR IGNORE INTO feishu_plan_cleanup_queue (plan_id)
            SELECT plan_id FROM feishu_source_rows WHERE plan_id IS NOT NULL;
            DELETE FROM plans
            WHERE id IN (SELECT plan_id FROM feishu_source_rows WHERE plan_id IS NOT NULL);
            DELETE FROM feishu_source_rows;
            DELETE FROM settings WHERE key = 'feishu_source_state';
            "#,
        )?;
        ensure_plan_columns(&connection)?;
        Ok(())
    }

    pub fn create_feed(&self, input: CreateFeedInput) -> AppResult<CreateFeedResult> {
        let content = input.content.trim();
        if content.is_empty() {
            return Err(AppError::Validation("内容不能为空".to_string()));
        }
        if content.chars().count() > 100_000 {
            return Err(AppError::Validation(
                "单条内容不能超过 100000 字".to_string(),
            ));
        }

        let now = Utc::now().timestamp_millis();
        let feed_id = Uuid::new_v4().to_string();
        let memory_id = Uuid::new_v4().to_string();
        let version_id = Uuid::new_v4().to_string();
        let source_type = input.source_type.unwrap_or_else(|| "manual".to_string());
        let title = derive_title(content);

        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO feed_events (id, raw_content, source_type, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![feed_id, content, source_type, now],
        )?;
        transaction.execute(
            "INSERT INTO memories (id, memory_type, current_version_id, created_at) VALUES (?1, 'Unclassified', ?2, ?3)",
            params![memory_id, version_id, now],
        )?;
        transaction.execute(
            "INSERT INTO memory_versions
             (id, memory_id, title, body, confidence, author_type, change_reason, created_at)
             VALUES (?1, ?2, ?3, ?4, 1.0, 'user', '原始投喂', ?5)",
            params![version_id, memory_id, title, content, now],
        )?;
        transaction.execute(
            "INSERT INTO memory_version_sources (memory_version_id, feed_event_id) VALUES (?1, ?2)",
            params![version_id, feed_id],
        )?;
        transaction.execute(
            "INSERT INTO memory_fts (version_id, memory_id, title, body, summary) VALUES (?1, ?2, ?3, ?4, '')",
            params![version_id, memory_id, title, content],
        )?;
        insert_audit(
            &transaction,
            "user",
            "create",
            "feed_event",
            &feed_id,
            json!({}),
        )?;
        transaction.commit()?;

        Ok(CreateFeedResult { feed_id, memory_id })
    }

    pub fn create_selection_feed(
        &self,
        selected_text: &str,
        surrounding_text: &str,
        source_title: &str,
    ) -> AppResult<CreateFeedResult> {
        let content = selected_text.trim();
        if content.is_empty() {
            return Err(AppError::Validation("选中文字不能为空".to_string()));
        }
        if content.chars().count() > 4_000 || surrounding_text.chars().count() > 6_000 {
            return Err(AppError::Validation(
                "选区或周边内容超过授权读取上限".to_string(),
            ));
        }

        let now = Utc::now().timestamp_millis();
        let feed_id = Uuid::new_v4().to_string();
        let memory_id = Uuid::new_v4().to_string();
        let version_id = Uuid::new_v4().to_string();
        let title = derive_title(content);
        let metadata = json!({
            "sourceTitle": truncate(source_title, 500),
            "surroundingText": surrounding_text,
            "contextBoundary": "same-accessibility-control, 1000 characters before and after",
        });

        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO feed_events
             (id, raw_content, source_type, source_metadata, created_at)
             VALUES (?1, ?2, 'selection', ?3, ?4)",
            params![feed_id, content, metadata.to_string(), now],
        )?;
        transaction.execute(
            "INSERT INTO memories (id, memory_type, current_version_id, created_at)
             VALUES (?1, 'Unclassified', ?2, ?3)",
            params![memory_id, version_id, now],
        )?;
        transaction.execute(
            "INSERT INTO memory_versions
             (id, memory_id, title, body, confidence, author_type, change_reason, created_at)
             VALUES (?1, ?2, ?3, ?4, 1.0, 'user', '系统选区投喂', ?5)",
            params![version_id, memory_id, title, content, now],
        )?;
        transaction.execute(
            "INSERT INTO memory_version_sources (memory_version_id, feed_event_id)
             VALUES (?1, ?2)",
            params![version_id, feed_id],
        )?;
        transaction.execute(
            "INSERT INTO memory_fts (version_id, memory_id, title, body, summary)
             VALUES (?1, ?2, ?3, ?4, '')",
            params![version_id, memory_id, title, content],
        )?;
        insert_audit(
            &transaction,
            "user",
            "create_from_selection",
            "feed_event",
            &feed_id,
            json!({ "sourceTitle": truncate(source_title, 500) }),
        )?;
        transaction.commit()?;
        Ok(CreateFeedResult { feed_id, memory_id })
    }

    pub fn create_plan(
        &self,
        feed_event_id: &str,
        proposal: &PlanProposal,
        scheduled_at: Option<i64>,
        source_title: &str,
    ) -> AppResult<PlanItem> {
        validate_plan_proposal(proposal, scheduled_at)?;
        let now = Utc::now().timestamp_millis();
        let id = Uuid::new_v4().to_string();
        let status = if scheduled_at.is_some() {
            "scheduled"
        } else {
            "needs_clarification"
        };
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "INSERT INTO plans
             (id, feed_event_id, title, details, content, link_url, notes, scheduled_at, status,
              clarification_question, source_title, created_at, updated_at, reminded_at,
              feishu_synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, NULL, NULL)",
            params![
                id,
                feed_event_id,
                proposal.title.trim(),
                proposal.details.trim(),
                proposal.content.trim(),
                proposal.link_url.as_deref().map(str::trim),
                proposal.notes.as_deref().map(str::trim),
                scheduled_at,
                status,
                proposal.clarification_question,
                truncate(source_title, 500),
                now,
            ],
        )?;
        self.get_plan_locked(&connection, &id)
    }

    pub fn list_plans(&self, include_done: bool) -> AppResult<Vec<PlanItem>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection.prepare(
            "SELECT id, feed_event_id, title, details, content, link_url, notes,
                    scheduled_at, status, clarification_question, source_title,
                    created_at, updated_at, reminded_at, feishu_synced_at
             FROM plans
             WHERE (?1 = 1 OR status != 'done')
             ORDER BY status = 'needs_clarification' DESC,
                      scheduled_at IS NULL, scheduled_at, created_at DESC",
        )?;
        let rows = statement.query_map([include_done], map_plan)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_plan(&self, plan_id: &str) -> AppResult<PlanItem> {
        let connection = self.connection.lock().expect("database lock poisoned");
        self.get_plan_locked(&connection, plan_id)
    }

    pub fn schedule_plan(
        &self,
        plan_id: &str,
        proposal: &PlanProposal,
        scheduled_at: i64,
    ) -> AppResult<PlanItem> {
        validate_plan_proposal(proposal, Some(scheduled_at))?;
        let connection = self.connection.lock().expect("database lock poisoned");
        let changed = connection.execute(
            "UPDATE plans
             SET title = ?2, details = ?3, content = ?4, link_url = ?5, notes = ?6,
                 scheduled_at = ?7, status = 'scheduled', clarification_question = NULL,
                 updated_at = ?8, reminded_at = NULL
             WHERE id = ?1",
            params![
                plan_id,
                proposal.title.trim(),
                proposal.details.trim(),
                proposal.content.trim(),
                proposal.link_url.as_deref().map(str::trim),
                proposal.notes.as_deref().map(str::trim),
                scheduled_at,
                Utc::now().timestamp_millis(),
            ],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("计划不存在".to_string()));
        }
        self.get_plan_locked(&connection, plan_id)
    }

    pub fn update_plan_clarification(
        &self,
        plan_id: &str,
        proposal: &PlanProposal,
    ) -> AppResult<PlanItem> {
        validate_plan_proposal(proposal, None)?;
        let connection = self.connection.lock().expect("database lock poisoned");
        let changed = connection.execute(
            "UPDATE plans
             SET title = ?2, details = ?3, content = ?4, link_url = ?5, notes = ?6,
                 status = 'needs_clarification', clarification_question = ?7,
                 updated_at = ?8, reminded_at = NULL
             WHERE id = ?1",
            params![
                plan_id,
                proposal.title.trim(),
                proposal.details.trim(),
                proposal.content.trim(),
                proposal.link_url.as_deref().map(str::trim),
                proposal.notes.as_deref().map(str::trim),
                proposal.clarification_question.as_deref(),
                Utc::now().timestamp_millis(),
            ],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("计划不存在".to_string()));
        }
        self.get_plan_locked(&connection, plan_id)
    }

    pub fn set_plan_done(&self, plan_id: &str, done: bool) -> AppResult<PlanItem> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let status = if done { "done" } else { "scheduled" };
        let changed = connection.execute(
            "UPDATE plans SET status = ?2, updated_at = MAX(?3, updated_at + 1) WHERE id = ?1",
            params![plan_id, status, Utc::now().timestamp_millis()],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("计划不存在".to_string()));
        }
        self.get_plan_locked(&connection, plan_id)
    }

    pub fn apply_remote_plan_done(
        &self,
        plan_id: &str,
        done: bool,
        expected_updated_at: i64,
        source: &str,
    ) -> AppResult<Option<PlanItem>> {
        let sheet_source = match source {
            "feishu_sheet" => true,
            "feishu_task" => false,
            _ => return Err(AppError::Validation("远端计划状态来源不受支持".to_string())),
        };
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT status, scheduled_at, updated_at, feishu_synced_at
                 FROM plans WHERE id = ?1",
                [plan_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((current_status, scheduled_at, updated_at, feishu_synced_at)) = current else {
            return Ok(None);
        };
        if updated_at != expected_updated_at
            || (sheet_source && !feishu_synced_at.is_some_and(|value| value >= updated_at))
            || (current_status == "done") == done
        {
            return Ok(None);
        }

        let status = if done {
            "done"
        } else if scheduled_at.is_some() {
            "scheduled"
        } else {
            "needs_clarification"
        };
        let now = Utc::now()
            .timestamp_millis()
            .max(expected_updated_at.saturating_add(1));
        let next_feishu_synced_at = if sheet_source {
            Some(now)
        } else {
            feishu_synced_at
        };
        let changed = transaction.execute(
            "UPDATE plans
             SET status = ?2, updated_at = ?3, feishu_synced_at = ?4
             WHERE id = ?1 AND updated_at = ?5",
            params![
                plan_id,
                status,
                now,
                next_feishu_synced_at,
                expected_updated_at
            ],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        transaction.execute(
            "INSERT INTO audit_log
             (id, actor_type, action, target_type, target_id, metadata_json, created_at)
             VALUES (?1, 'external', 'plan.status_synced_from_feishu', 'plan', ?2, ?3, ?4)",
            params![
                Uuid::new_v4().to_string(),
                plan_id,
                json!({ "source": source, "done": done }).to_string(),
                now
            ],
        )?;
        let plan = self.get_plan_locked(&transaction, plan_id)?;
        transaction.commit()?;
        Ok(Some(plan))
    }

    pub fn list_due_plan_reminders(&self, now: i64, due_at: i64) -> AppResult<Vec<PlanItem>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection.prepare(
            "SELECT id, feed_event_id, title, details, content, link_url, notes,
                    scheduled_at, status, clarification_question, source_title,
                    created_at, updated_at, reminded_at, feishu_synced_at
             FROM plans
             WHERE status = 'scheduled'
               AND reminded_at IS NULL
               AND scheduled_at IS NOT NULL
               AND scheduled_at BETWEEN ?1 AND ?2
             ORDER BY scheduled_at",
        )?;
        let oldest_recoverable = now - 24 * 60 * 60 * 1000;
        let rows = statement.query_map(params![oldest_recoverable, due_at], map_plan)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn mark_plan_reminded(&self, plan_id: &str, reminded_at: i64) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let changed = connection.execute(
            "UPDATE plans SET reminded_at = ?2 WHERE id = ?1 AND reminded_at IS NULL",
            params![plan_id, reminded_at],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("计划不存在或已经提醒".to_string()));
        }
        Ok(())
    }

    pub fn list_pending_feishu_plans(&self, limit: i64) -> AppResult<Vec<PlanItem>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection.prepare(
            "SELECT id, feed_event_id, title, details, content, link_url, notes,
                    scheduled_at, status, clarification_question, source_title,
                    created_at, updated_at, reminded_at, feishu_synced_at
             FROM plans
             WHERE feishu_synced_at IS NULL OR feishu_synced_at < updated_at
             ORDER BY updated_at
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit.clamp(1, 500)], map_plan)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn count_pending_feishu_plans(&self) -> AppResult<i64> {
        let connection = self.connection.lock().expect("database lock poisoned");
        count(
            &connection,
            "SELECT COUNT(*) FROM plans
             WHERE feishu_synced_at IS NULL OR feishu_synced_at < updated_at",
        )
    }

    pub fn list_feishu_plan_cleanup_ids(&self) -> AppResult<Vec<String>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection
            .prepare("SELECT plan_id FROM feishu_plan_cleanup_queue ORDER BY plan_id LIMIT 500")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn complete_feishu_plan_cleanup(&self, plan_ids: &[String]) -> AppResult<()> {
        if plan_ids.is_empty() {
            return Ok(());
        }
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        for plan_id in plan_ids {
            transaction.execute(
                "DELETE FROM feishu_plan_cleanup_queue WHERE plan_id = ?1",
                [plan_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_feishu_plan_task_mappings(&self) -> AppResult<Vec<FeishuPlanTaskMapping>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection.prepare(
            "SELECT plan_id, task_guid, task_url, plan_updated_at, completed
             FROM feishu_plan_tasks ORDER BY plan_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(FeishuPlanTaskMapping {
                plan_id: row.get(0)?,
                task_guid: row.get(1)?,
                task_url: row.get(2)?,
                plan_updated_at: row.get(3)?,
                completed: row.get::<_, i64>(4)? != 0,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn save_feishu_plan_task_mapping(&self, mapping: &FeishuPlanTaskMapping) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "INSERT INTO feishu_plan_tasks
             (plan_id, task_guid, task_url, plan_updated_at, completed)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(plan_id) DO UPDATE SET
               task_guid = excluded.task_guid,
               task_url = excluded.task_url,
               plan_updated_at = excluded.plan_updated_at,
               completed = excluded.completed",
            params![
                mapping.plan_id,
                mapping.task_guid,
                mapping.task_url,
                mapping.plan_updated_at,
                mapping.completed as i64,
            ],
        )?;
        Ok(())
    }

    pub fn delete_feishu_plan_task_mapping(&self, plan_id: &str) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "DELETE FROM feishu_plan_tasks WHERE plan_id = ?1",
            [plan_id],
        )?;
        Ok(())
    }

    pub fn count_pending_feishu_plan_tasks(&self) -> AppResult<i64> {
        let connection = self.connection.lock().expect("database lock poisoned");
        count(
            &connection,
            "SELECT COUNT(*)
             FROM plans p
             LEFT JOIN feishu_plan_tasks t ON t.plan_id = p.id
             WHERE p.scheduled_at IS NOT NULL
               AND (
                 (p.status != 'done' AND
                   (t.plan_id IS NULL OR t.plan_updated_at < p.updated_at OR t.completed = 1))
                 OR
                 (p.status = 'done' AND t.plan_id IS NOT NULL AND
                   (t.plan_updated_at < p.updated_at OR t.completed = 0))
               )",
        )
        .and_then(|pending| {
            count(
                &connection,
                "SELECT COUNT(*) FROM feishu_plan_tasks t
                 WHERE NOT EXISTS (SELECT 1 FROM plans p WHERE p.id = t.plan_id)",
            )
            .map(|orphaned| pending + orphaned)
        })
    }

    pub fn get_feishu_task_sync_error(&self) -> AppResult<Option<String>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'feishu_task_sync_error'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn save_feishu_task_sync_error(&self, error: Option<&str>) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        if let Some(error) = error {
            connection.execute(
                "INSERT INTO settings (key, value) VALUES ('feishu_task_sync_error', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [truncate(error, 2_000)],
            )?;
        } else {
            connection.execute(
                "DELETE FROM settings WHERE key = 'feishu_task_sync_error'",
                [],
            )?;
        }
        Ok(())
    }

    pub fn mark_plan_feishu_synced(&self, plan_id: &str, synced_at: i64) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let changed = connection.execute(
            "UPDATE plans SET feishu_synced_at = ?2 WHERE id = ?1",
            params![plan_id, synced_at],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("计划不存在".to_string()));
        }
        Ok(())
    }

    pub fn get_feishu_sheet_state(&self) -> AppResult<Option<FeishuSheetState>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let value: Option<String> = connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'feishu_sheet'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(AppError::from))
            .transpose()
    }

    pub fn save_feishu_sheet_state(&self, state: &FeishuSheetState) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "INSERT INTO settings (key, value) VALUES ('feishu_sheet', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(state)?],
        )?;
        connection.execute("UPDATE plans SET feishu_synced_at = NULL", [])?;
        Ok(())
    }

    pub fn get_feishu_sync_error(&self) -> AppResult<Option<String>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'feishu_sync_error'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn save_feishu_sync_error(&self, error: Option<&str>) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        if let Some(error) = error {
            connection.execute(
                "INSERT INTO settings (key, value) VALUES ('feishu_sync_error', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [error],
            )?;
        } else {
            connection.execute("DELETE FROM settings WHERE key = 'feishu_sync_error'", [])?;
        }
        Ok(())
    }

    fn get_plan_locked(&self, connection: &Connection, plan_id: &str) -> AppResult<PlanItem> {
        connection
            .query_row(
                "SELECT id, feed_event_id, title, details, content, link_url, notes,
                        scheduled_at, status, clarification_question, source_title,
                        created_at, updated_at, reminded_at, feishu_synced_at
                 FROM plans WHERE id = ?1",
                [plan_id],
                map_plan,
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("计划不存在".to_string()))
    }

    pub fn list_feeds(&self, query: Option<String>, limit: i64) -> AppResult<Vec<FeedEvent>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let pattern = format!("%{}%", query.unwrap_or_default().trim());
        let mut statement = connection.prepare(
            "SELECT f.id, f.raw_content, f.source_type, f.processing_status, f.created_at,
                    (SELECT mv.memory_id
                     FROM memory_version_sources mvs
                     JOIN memory_versions mv ON mv.id = mvs.memory_version_id
                     JOIN memories m ON m.id = mv.memory_id
                     WHERE mvs.feed_event_id = f.id AND m.archived_at IS NULL
                     ORDER BY (mv.id = m.current_version_id) DESC, mv.created_at DESC
                     LIMIT 1) AS memory_id
             FROM feed_events f
             WHERE f.deleted_at IS NULL AND f.raw_content LIKE ?1
             ORDER BY f.created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![pattern, limit.clamp(1, 500)], |row| {
            Ok(FeedEvent {
                id: row.get(0)?,
                raw_content: row.get(1)?,
                source_type: row.get(2)?,
                processing_status: row.get(3)?,
                created_at: row.get(4)?,
                memory_id: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_memories(
        &self,
        query: Option<String>,
        memory_type: Option<String>,
        limit: i64,
    ) -> AppResult<Vec<MemorySummary>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let query = query.unwrap_or_default();
        let pattern = format!("%{}%", query.trim());
        let fts_query = build_fts_query(&query);
        let memory_type = memory_type.unwrap_or_default();
        let mut statement = connection.prepare(
            "SELECT m.id, m.memory_type, m.lifecycle_status, v.title, v.body, v.summary,
                    v.confidence, v.author_type, m.created_at, v.created_at,
                    (SELECT COUNT(DISTINCT mvs.feed_event_id)
                     FROM memory_version_sources mvs
                     JOIN memory_versions sv ON sv.id = mvs.memory_version_id
                     WHERE sv.memory_id = m.id)
             FROM memories m
             JOIN memory_versions v ON v.id = m.current_version_id
             WHERE m.archived_at IS NULL
               AND (?1 = '' OR m.memory_type = ?1)
               AND (?2 = '%%'
                    OR v.title LIKE ?2
                    OR v.body LIKE ?2
                    OR COALESCE(v.summary, '') LIKE ?2
                    OR m.id IN (
                        SELECT memory_id FROM memory_fts WHERE memory_fts MATCH ?3
                    ))
             ORDER BY v.created_at DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![memory_type, pattern, fts_query, limit.clamp(1, 500)],
            map_memory_summary,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_memory(&self, memory_id: &str) -> AppResult<MemoryDetail> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let memory = connection
            .query_row(
                "SELECT m.id, m.memory_type, m.lifecycle_status, v.title, v.body, v.summary,
                        v.confidence, v.author_type, m.created_at, v.created_at,
                        (SELECT COUNT(DISTINCT mvs.feed_event_id)
                         FROM memory_version_sources mvs
                         JOIN memory_versions sv ON sv.id = mvs.memory_version_id
                         WHERE sv.memory_id = m.id)
                 FROM memories m JOIN memory_versions v ON v.id = m.current_version_id
                 WHERE m.id = ?1",
                [memory_id],
                map_memory_summary,
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("记忆不存在".to_string()))?;

        let mut statement = connection.prepare(
            "SELECT id, title, body, summary, confidence, author_type, model_info,
                    change_reason, created_at
             FROM memory_versions
             WHERE memory_id = ?1
             ORDER BY
               (id = (SELECT current_version_id FROM memories WHERE id = ?1)) DESC,
               created_at DESC",
        )?;
        let versions = statement
            .query_map([memory_id], |row| {
                let version_id: String = row.get(0)?;
                let mut source_statement = connection.prepare(
                    "SELECT feed_event_id FROM memory_version_sources WHERE memory_version_id = ?1",
                )?;
                let source_event_ids = source_statement
                    .query_map([&version_id], |source_row| source_row.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                Ok(MemoryVersion {
                    id: version_id,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    summary: row.get(3)?,
                    confidence: row.get(4)?,
                    author_type: row.get(5)?,
                    model_info: row.get(6)?,
                    change_reason: row.get(7)?,
                    created_at: row.get(8)?,
                    source_event_ids,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(MemoryDetail { memory, versions })
    }

    pub fn get_feed_for_processing(&self, feed_id: &str) -> AppResult<(String, String)> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection
            .query_row(
                "SELECT f.raw_content, mvs.memory_id
                 FROM feed_events f
                 JOIN memory_version_sources mvs ON mvs.feed_event_id = f.id
                 WHERE f.id = ?1 LIMIT 1",
                [feed_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("投喂记录不存在".to_string()))
    }

    pub fn recent_context(
        &self,
        exclude_memory_id: &str,
        limit: i64,
    ) -> AppResult<Vec<MemorySummary>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection.prepare(
            "SELECT m.id, m.memory_type, m.lifecycle_status, v.title, v.body, v.summary,
                    v.confidence, v.author_type, m.created_at, v.created_at, 1
             FROM memories m JOIN memory_versions v ON v.id = m.current_version_id
             WHERE m.id != ?1 AND m.archived_at IS NULL
             ORDER BY v.created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![exclude_memory_id, limit], map_memory_summary)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn start_processing(&self, feed_id: &str, model: &str) -> AppResult<String> {
        let run_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "INSERT INTO processing_runs
             (id, feed_event_id, pipeline_version, model_info, status, created_at)
             VALUES (?1, ?2, 'v0.1', ?3, 'running', ?4)",
            params![run_id, feed_id, model, now],
        )?;
        connection.execute(
            "UPDATE feed_events SET processing_status = 'processing' WHERE id = ?1",
            [feed_id],
        )?;
        Ok(run_id)
    }

    pub fn fail_processing(&self, run_id: &str, feed_id: &str, error: &str) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "UPDATE processing_runs SET status = 'failed', error_code = ?2 WHERE id = ?1",
            params![run_id, truncate(error, 500)],
        )?;
        connection.execute(
            "UPDATE feed_events SET processing_status = 'pending' WHERE id = ?1",
            [feed_id],
        )?;
        Ok(())
    }

    pub fn create_review(
        &self,
        run_id: &str,
        feed_id: &str,
        memory_id: &str,
        proposal: &AiProposal,
        model: &str,
    ) -> AppResult<String> {
        validate_proposal(proposal)?;
        if proposal.action != "ask" {
            return Err(AppError::Validation(
                "普通分类应自动提交，不能进入待澄清队列".to_string(),
            ));
        }

        let review_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let payload = json!({
            "memoryId": memory_id,
            "feedId": feed_id,
            "memoryType": proposal.memory_type,
            "title": proposal.title,
            "summary": proposal.summary,
            "body": proposal.body,
            "confidence": proposal.confidence,
            "model": model,
            "question": proposal.question,
            "targetMemoryId": proposal.target_memory_id,
            "relationType": proposal.relation_type,
        });
        let output = serde_json::to_string(proposal)?;
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO review_items
             (id, feed_event_id, proposed_action, payload_json, risk_level, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                review_id,
                feed_id,
                "ask",
                payload.to_string(),
                "high",
                truncate(&proposal.reason, 500),
                now
            ],
        )?;
        transaction.execute(
            "UPDATE processing_runs SET status = 'review', output_json = ?2 WHERE id = ?1",
            params![run_id, output],
        )?;
        transaction.execute(
            "UPDATE feed_events SET processing_status = 'review' WHERE id = ?1",
            [feed_id],
        )?;
        transaction.commit()?;
        Ok(review_id)
    }

    pub fn apply_ai_proposal(
        &self,
        run_id: &str,
        feed_id: &str,
        memory_id: &str,
        proposal: &AiProposal,
        model: &str,
    ) -> AppResult<()> {
        validate_proposal(proposal)?;
        if proposal.action == "ask" {
            return Err(AppError::Validation(
                "歧义内容必须进入待澄清队列".to_string(),
            ));
        }

        let now = Utc::now().timestamp_millis();
        let effective_type = if proposal.confidence < 0.55 {
            "Unclassified"
        } else {
            proposal.memory_type.as_str()
        };
        let payload = json!({
            "memoryId": memory_id,
            "feedId": feed_id,
            "memoryType": effective_type,
            "title": proposal.title,
            "summary": proposal.summary,
            "body": proposal.body,
            "confidence": proposal.confidence,
            "model": model,
        });
        let output = serde_json::to_string(proposal)?;
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;

        let mut applied_action = proposal.action.as_str();
        let mut audit_target = memory_id;
        match proposal.action.as_str() {
            "ignore" => {
                transaction.execute(
                    "UPDATE memories SET lifecycle_status = 'ignored', archived_at = ?2 WHERE id = ?1",
                    params![memory_id, now],
                )?;
            }
            "update" if proposal.confidence >= 0.75 => {
                let target_id = proposal.target_memory_id.as_deref().unwrap_or_default();
                if active_memory_exists(&transaction, target_id)? && target_id != memory_id {
                    apply_existing_update(
                        &transaction,
                        &payload,
                        target_id,
                        now,
                        "ai",
                        "AI 根据新来源更新记忆",
                    )?;
                    audit_target = target_id;
                } else {
                    applied_action = "create_fallback";
                    apply_enrichment(&transaction, &payload, now, "ai", "AI 自动分类与整理")?;
                }
            }
            "link" if proposal.confidence >= 0.7 => {
                apply_enrichment(&transaction, &payload, now, "ai", "AI 自动分类与整理")?;
                let target_id = proposal.target_memory_id.as_deref().unwrap_or_default();
                if active_memory_exists(&transaction, target_id)? && target_id != memory_id {
                    insert_relation(
                        &transaction,
                        memory_id,
                        target_id,
                        proposal.relation_type.as_deref().unwrap_or("related"),
                        proposal.confidence,
                        now,
                    )?;
                } else {
                    applied_action = "create_fallback";
                }
            }
            _ => {
                if matches!(proposal.action.as_str(), "update" | "link") {
                    applied_action = "create_low_confidence";
                }
                apply_enrichment(&transaction, &payload, now, "ai", "AI 自动分类与整理")?;
            }
        }
        transaction.execute(
            "UPDATE processing_runs SET status = 'completed', output_json = ?2 WHERE id = ?1",
            params![run_id, output],
        )?;
        transaction.execute(
            "UPDATE feed_events SET processing_status = ?2 WHERE id = ?1",
            params![
                feed_id,
                if proposal.action == "ignore" {
                    "ignored"
                } else {
                    "classified"
                }
            ],
        )?;
        insert_audit(
            &transaction,
            "ai",
            if proposal.action == "ignore" {
                "auto_ignore"
            } else {
                applied_action
            },
            "memory",
            audit_target,
            json!({
                "memoryType": effective_type,
                "confidence": proposal.confidence,
                "model": model,
                "requestedAction": proposal.action,
            }),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_reviews(&self) -> AppResult<Vec<ReviewItem>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection.prepare(
            "SELECT id, proposed_action, risk_level, reason, status, payload_json, created_at
             FROM review_items WHERE status = 'pending' ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let payload_string: String = row.get(5)?;
            Ok(ReviewItem {
                id: row.get(0)?,
                proposed_action: row.get(1)?,
                risk_level: row.get(2)?,
                reason: row.get(3)?,
                status: row.get(4)?,
                payload: serde_json::from_str(&payload_string).unwrap_or(Value::Null),
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn resolve_review(&self, review_id: &str, accept: bool) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        let feed_id: Option<String> = transaction
            .query_row(
                "SELECT feed_event_id
                 FROM review_items WHERE id = ?1 AND status = 'pending'",
                [review_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("待澄清项不存在或已处理".to_string()))?;
        let now = Utc::now().timestamp_millis();

        let status = if accept { "accepted" } else { "rejected" };
        transaction.execute(
            "UPDATE review_items SET status = ?2, resolved_at = ?3 WHERE id = ?1",
            params![review_id, status, now],
        )?;
        if let Some(feed_id) = feed_id {
            transaction.execute(
                "UPDATE feed_events SET processing_status = ?2 WHERE id = ?1",
                params![feed_id, status],
            )?;
        }
        insert_audit(
            &transaction,
            "user",
            if accept {
                "accept_review"
            } else {
                "reject_review"
            },
            "review_item",
            review_id,
            json!({}),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn delete_feed(&self, feed_id: &str) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        let memory_ids = {
            let mut statement = transaction.prepare(
                "SELECT DISTINCT mv.memory_id
                 FROM memory_version_sources mvs
                 JOIN memory_versions mv ON mv.id = mvs.memory_version_id
                 WHERE mvs.feed_event_id = ?1",
            )?;
            let rows = statement.query_map([feed_id], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if memory_ids.is_empty() {
            return Err(AppError::NotFound("投喂记录不存在".to_string()));
        }
        for memory_id in &memory_ids {
            let remaining_sources = {
                let mut statement = transaction.prepare(
                    "SELECT DISTINCT f.id, f.raw_content
                     FROM feed_events f
                     JOIN memory_version_sources mvs ON mvs.feed_event_id = f.id
                     JOIN memory_versions mv ON mv.id = mvs.memory_version_id
                     WHERE mv.memory_id = ?1 AND f.id != ?2 AND f.deleted_at IS NULL
                     ORDER BY f.created_at",
                )?;
                let rows = statement.query_map(params![memory_id, feed_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            transaction.execute("DELETE FROM memory_fts WHERE memory_id = ?1", [memory_id])?;
            if remaining_sources.is_empty() {
                transaction.execute("DELETE FROM memories WHERE id = ?1", [memory_id])?;
                continue;
            }

            // Every derived version may contain the deleted source. Rebuild from remaining raw
            // events instead of trying to redact model-generated prose.
            transaction.execute(
                "DELETE FROM memory_versions WHERE memory_id = ?1",
                [memory_id],
            )?;
            let version_id = Uuid::new_v4().to_string();
            let body = remaining_sources
                .iter()
                .map(|(_, raw)| raw.as_str())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            let title = derive_title(&remaining_sources[0].1);
            let now = Utc::now().timestamp_millis();
            transaction.execute(
                "INSERT INTO memory_versions
                 (id, memory_id, title, body, summary, confidence, author_type, change_reason, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1.0, 'system', '删除来源后从剩余原文重建', ?6)",
                params![
                    version_id,
                    memory_id,
                    title,
                    body,
                    "删除来源后等待 AI 重新整理",
                    now
                ],
            )?;
            for (source_id, _) in &remaining_sources {
                transaction.execute(
                    "INSERT INTO memory_version_sources (memory_version_id, feed_event_id)
                     VALUES (?1, ?2)",
                    params![version_id, source_id],
                )?;
            }
            transaction.execute(
                "UPDATE memories SET memory_type = 'Unclassified', current_version_id = ?2
                 WHERE id = ?1",
                params![memory_id, version_id],
            )?;
            transaction.execute(
                "INSERT INTO memory_fts (version_id, memory_id, title, body, summary)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    version_id,
                    memory_id,
                    title,
                    body,
                    "删除来源后等待 AI 重新整理"
                ],
            )?;
        }
        transaction.execute("DELETE FROM feed_events WHERE id = ?1", [feed_id])?;
        insert_audit(
            &transaction,
            "user",
            "permanent_delete",
            "feed_event",
            feed_id,
            json!({ "contentRetained": false }),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_stats(&self) -> AppResult<Stats> {
        let connection = self.connection.lock().expect("database lock poisoned");
        Ok(Stats {
            total_feeds: count(&connection, "SELECT COUNT(*) FROM feed_events")?,
            total_memories: count(&connection, "SELECT COUNT(*) FROM memories WHERE archived_at IS NULL")?,
            pending_reviews: count(&connection, "SELECT COUNT(*) FROM review_items WHERE status = 'pending'")?,
            pending_processing: count(
                &connection,
                "SELECT COUNT(*) FROM feed_events WHERE processing_status IN ('pending', 'processing')",
            )?,
        })
    }

    pub fn get_settings(&self) -> AppResult<AppSettings> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let settings_json: Option<String> = connection
            .query_row("SELECT value FROM settings WHERE key = 'app'", [], |row| {
                row.get(0)
            })
            .optional()?;
        match settings_json {
            Some(value) => Ok(serde_json::from_str(&value).unwrap_or_default()),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn get_vault_meta(&self) -> AppResult<Option<VaultMeta>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection
            .query_row(
                "SELECT salt, verifier_nonce, verifier_ciphertext FROM vault_meta WHERE id = 1",
                [],
                |row| {
                    Ok(VaultMeta {
                        salt: row.get(0)?,
                        verifier_nonce: row.get(1)?,
                        verifier_ciphertext: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn save_vault_meta(&self, meta: &VaultMeta) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "INSERT INTO vault_meta
             (id, salt, verifier_nonce, verifier_ciphertext, created_at)
             VALUES (1, ?1, ?2, ?3, ?4)",
            params![
                meta.salt,
                meta.verifier_nonce,
                meta.verifier_ciphertext,
                Utc::now().timestamp_millis()
            ],
        )?;
        Ok(())
    }

    pub fn count_secret_records(&self) -> AppResult<i64> {
        let connection = self.connection.lock().expect("database lock poisoned");
        count(&connection, "SELECT COUNT(*) FROM secret_records")
    }

    pub fn insert_secret_record(&self, record: &EncryptedSecretRecord) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO secret_records
             (id, nonce, ciphertext, created_at, updated_at, feishu_synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id,
                record.nonce,
                record.ciphertext,
                record.created_at,
                record.updated_at,
                record.feishu_synced_at
            ],
        )?;
        insert_audit(
            &transaction,
            "user",
            "secret_created",
            "secret_record",
            &record.id,
            json!({ "plaintextLogged": false }),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn update_secret_record(&self, record: &EncryptedSecretRecord) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let changed = connection.execute(
            "UPDATE secret_records
             SET nonce = ?2, ciphertext = ?3, updated_at = ?4, feishu_synced_at = ?5
             WHERE id = ?1",
            params![
                record.id,
                record.nonce,
                record.ciphertext,
                record.updated_at,
                record.feishu_synced_at
            ],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("秘密记录不存在".to_string()));
        }
        Ok(())
    }

    pub fn get_encrypted_secret_record(&self, id: &str) -> AppResult<EncryptedSecretRecord> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection
            .query_row(
                "SELECT id, nonce, ciphertext, created_at, updated_at, feishu_synced_at
                 FROM secret_records WHERE id = ?1",
                [id],
                map_encrypted_secret,
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("秘密记录不存在".to_string()))
    }

    pub fn list_encrypted_secret_records(&self) -> AppResult<Vec<EncryptedSecretRecord>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement = connection.prepare(
            "SELECT id, nonce, ciphertext, created_at, updated_at, feishu_synced_at
             FROM secret_records ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([], map_encrypted_secret)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn delete_secret_record(&self, id: &str) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        let synced: Option<i64> = transaction
            .query_row(
                "SELECT feishu_synced_at FROM secret_records WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if synced.is_some() {
            transaction.execute(
                "INSERT OR IGNORE INTO feishu_secret_cleanup_queue (secret_id) VALUES (?1)",
                [id],
            )?;
        }
        let changed = transaction.execute("DELETE FROM secret_records WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(AppError::NotFound("秘密记录不存在".to_string()));
        }
        insert_audit(
            &transaction,
            "user",
            "secret_deleted",
            "secret_record",
            id,
            json!({ "plaintextLogged": false }),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_secret_feishu_synced(&self, id: &str, synced_at: i64) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "UPDATE secret_records SET feishu_synced_at = ?2 WHERE id = ?1",
            params![id, synced_at],
        )?;
        Ok(())
    }

    pub fn count_pending_feishu_secrets(&self) -> AppResult<i64> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let pending = count(
            &connection,
            "SELECT COUNT(*) FROM secret_records
             WHERE feishu_synced_at IS NULL OR feishu_synced_at < updated_at",
        )?;
        let cleanup = count(
            &connection,
            "SELECT COUNT(*) FROM feishu_secret_cleanup_queue",
        )?;
        Ok(pending + cleanup)
    }

    pub fn list_feishu_secret_cleanup(&self) -> AppResult<Vec<String>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let mut statement =
            connection.prepare("SELECT secret_id FROM feishu_secret_cleanup_queue")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn complete_feishu_secret_cleanup(&self, ids: &[String]) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database lock poisoned");
        let transaction = connection.transaction()?;
        for id in ids {
            transaction.execute(
                "DELETE FROM feishu_secret_cleanup_queue WHERE secret_id = ?1",
                [id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_feishu_secret_sheet_state(&self) -> AppResult<Option<FeishuSheetState>> {
        self.get_json_setting("feishu_secret_sheet")
    }

    pub fn save_feishu_secret_sheet_state(&self, state: &FeishuSheetState) -> AppResult<()> {
        self.save_json_setting("feishu_secret_sheet", state)?;
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute("UPDATE secret_records SET feishu_synced_at = NULL", [])?;
        Ok(())
    }

    pub fn get_feishu_secret_sync_error(&self) -> AppResult<Option<String>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'feishu_secret_sync_error'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn save_feishu_secret_sync_error(&self, error: Option<&str>) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        if let Some(error) = error {
            connection.execute(
                "INSERT INTO settings (key, value) VALUES ('feishu_secret_sync_error', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [error],
            )?;
        } else {
            connection.execute(
                "DELETE FROM settings WHERE key = 'feishu_secret_sync_error'",
                [],
            )?;
        }
        Ok(())
    }

    fn get_json_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> AppResult<Option<T>> {
        let connection = self.connection.lock().expect("database lock poisoned");
        let value: Option<String> = connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(AppError::from))
            .transpose()
    }

    fn save_json_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> AppResult<()> {
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    pub fn update_settings(&self, settings: &AppSettings) -> AppResult<()> {
        validate_llm_endpoint(&settings.llm_endpoint)?;
        validate_embedding_endpoint(&settings.embedding_endpoint)?;
        if settings.llm_model.trim().is_empty() || settings.embedding_model.trim().is_empty() {
            return Err(AppError::Validation("模型名称不能为空".to_string()));
        }
        if ![256, 512, 1024, 2048].contains(&settings.embedding_dimensions) {
            return Err(AppError::Validation(
                "Embedding 维度只能是 256、512、1024 或 2048".to_string(),
            ));
        }
        if !matches!(settings.mobile_push_provider.as_str(), "ntfy" | "webhook") {
            return Err(AppError::Validation(
                "手机推送通道只能是 ntfy 或 webhook".to_string(),
            ));
        }
        if ![0, 5, 15, 30, 60].contains(&settings.mobile_reminder_minutes) {
            return Err(AppError::Validation(
                "提醒提前量只能是 0、5、15、30 或 60 分钟".to_string(),
            ));
        }
        if settings.feishu_source_enabled || !settings.feishu_source_url.trim().is_empty() {
            validate_feishu_source_url(&settings.feishu_source_url)?;
        }
        let connection = self.connection.lock().expect("database lock poisoned");
        connection.execute(
            "INSERT INTO settings (key, value) VALUES ('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    pub fn export_json(&self, path: &Path) -> AppResult<()> {
        let feeds = self.list_feeds(None, 100_000)?;
        let memories = self.list_memories(None, None, 100_000)?;
        let plans = self.list_plans(true)?;
        let export = json!({
            "schemaVersion": 2,
            "exportedAt": Utc::now().to_rfc3339(),
            "feeds": feeds,
            "memories": memories,
            "plans": plans,
        });
        std::fs::write(path, serde_json::to_vec_pretty(&export)?)?;
        Ok(())
    }
}

fn apply_enrichment(
    transaction: &Transaction<'_>,
    payload: &Value,
    now: i64,
    author_type: &str,
    change_reason: &str,
) -> AppResult<()> {
    let memory_id = payload["memoryId"]
        .as_str()
        .ok_or_else(|| AppError::Validation("建议缺少记忆 ID".to_string()))?;
    let feed_id = payload["feedId"]
        .as_str()
        .ok_or_else(|| AppError::Validation("建议缺少来源 ID".to_string()))?;
    let title = payload["title"].as_str().unwrap_or("未命名记忆");
    let summary = payload["summary"].as_str().unwrap_or("");
    let memory_type = payload["memoryType"].as_str().unwrap_or("Unclassified");
    let confidence = payload["confidence"].as_f64().unwrap_or(0.0);
    let model = payload["model"].as_str().unwrap_or("unknown");
    let body = match payload["body"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        Some(body) => body.to_string(),
        None => transaction.query_row(
            "SELECT raw_content FROM feed_events WHERE id = ?1",
            [feed_id],
            |row| row.get(0),
        )?,
    };
    let version_id = Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO memory_versions
         (id, memory_id, title, body, summary, confidence, author_type, model_info, change_reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            version_id,
            memory_id,
            title,
            body,
            summary,
            confidence,
            author_type,
            model,
            change_reason,
            now
        ],
    )?;
    transaction.execute(
        "INSERT INTO memory_version_sources (memory_version_id, feed_event_id) VALUES (?1, ?2)",
        params![version_id, feed_id],
    )?;
    transaction.execute(
        "UPDATE memories SET memory_type = ?2, current_version_id = ?3 WHERE id = ?1",
        params![memory_id, memory_type, version_id],
    )?;
    transaction.execute(
        "INSERT INTO memory_fts (version_id, memory_id, title, body, summary) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![version_id, memory_id, title, body, summary],
    )?;
    Ok(())
}

fn apply_existing_update(
    transaction: &Transaction<'_>,
    payload: &Value,
    target_memory_id: &str,
    now: i64,
    author_type: &str,
    change_reason: &str,
) -> AppResult<()> {
    let provisional_memory_id = payload["memoryId"]
        .as_str()
        .ok_or_else(|| AppError::Validation("判断缺少临时记忆 ID".to_string()))?;
    let feed_id = payload["feedId"]
        .as_str()
        .ok_or_else(|| AppError::Validation("判断缺少来源 ID".to_string()))?;
    let title = payload["title"].as_str().unwrap_or("未命名记忆");
    let summary = payload["summary"].as_str().unwrap_or("");
    let body = payload["body"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::AiInvalid("更新已有记忆时必须返回完整 body".to_string()))?;
    let memory_type = payload["memoryType"].as_str().unwrap_or("Unclassified");
    let confidence = payload["confidence"].as_f64().unwrap_or(0.0);
    let model = payload["model"].as_str().unwrap_or("unknown");
    let version_id = Uuid::new_v4().to_string();

    transaction.execute(
        "INSERT INTO memory_versions
         (id, memory_id, title, body, summary, confidence, author_type, model_info, change_reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            version_id,
            target_memory_id,
            title,
            body,
            summary,
            confidence,
            author_type,
            model,
            change_reason,
            now
        ],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO memory_version_sources (memory_version_id, feed_event_id)
         SELECT ?1, mvs.feed_event_id
         FROM memory_version_sources mvs
         JOIN memory_versions mv ON mv.id = mvs.memory_version_id
         WHERE mv.memory_id = ?2",
        params![version_id, target_memory_id],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO memory_version_sources (memory_version_id, feed_event_id)
         VALUES (?1, ?2)",
        params![version_id, feed_id],
    )?;
    transaction.execute(
        "UPDATE memories SET memory_type = ?2, current_version_id = ?3 WHERE id = ?1",
        params![target_memory_id, memory_type, version_id],
    )?;
    transaction.execute(
        "INSERT INTO memory_fts (version_id, memory_id, title, body, summary)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![version_id, target_memory_id, title, body, summary],
    )?;

    transaction.execute(
        "DELETE FROM memory_fts WHERE memory_id = ?1",
        [provisional_memory_id],
    )?;
    transaction.execute(
        "DELETE FROM memories WHERE id = ?1",
        [provisional_memory_id],
    )?;
    Ok(())
}

fn active_memory_exists(transaction: &Transaction<'_>, memory_id: &str) -> AppResult<bool> {
    Ok(transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1 AND archived_at IS NULL)",
        [memory_id],
        |row| row.get(0),
    )?)
}

fn insert_relation(
    transaction: &Transaction<'_>,
    from_memory_id: &str,
    to_memory_id: &str,
    relation_type: &str,
    confidence: f64,
    now: i64,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO memory_relations
         (id, from_memory_id, to_memory_id, relation_type, confidence, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'accepted', ?6)",
        params![
            Uuid::new_v4().to_string(),
            from_memory_id,
            to_memory_id,
            relation_type,
            confidence,
            now
        ],
    )?;
    Ok(())
}

fn validate_proposal(proposal: &AiProposal) -> AppResult<()> {
    let allowed_types = [
        "Knowledge",
        "Project",
        "Decision",
        "Idea",
        "Task",
        "Preference",
        "Person",
        "Experience",
        "Unclassified",
    ];
    let allowed_actions = ["create", "update", "link", "ask", "ignore"];
    if !allowed_types.contains(&proposal.memory_type.as_str()) {
        return Err(AppError::AiInvalid("未知记忆类型".to_string()));
    }
    if !allowed_actions.contains(&proposal.action.as_str()) {
        return Err(AppError::AiInvalid("未知处理动作".to_string()));
    }
    if !(0.0..=1.0).contains(&proposal.confidence) {
        return Err(AppError::AiInvalid("置信度超出范围".to_string()));
    }
    if proposal.title.trim().is_empty() || proposal.summary.trim().is_empty() {
        return Err(AppError::AiInvalid("标题或摘要为空".to_string()));
    }
    if proposal.title.chars().count() > 36 || proposal.summary.chars().count() > 1000 {
        return Err(AppError::AiInvalid("标题或摘要超过长度限制".to_string()));
    }
    if proposal.body.as_deref().unwrap_or_default().chars().count() > 12_000 {
        return Err(AppError::AiInvalid("记忆正文超过长度限制".to_string()));
    }
    if matches!(proposal.action.as_str(), "update" | "link")
        && proposal
            .target_memory_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(AppError::AiInvalid("更新或关联缺少目标记忆 ID".to_string()));
    }
    if proposal.action == "update"
        && proposal.confidence >= 0.75
        && proposal
            .body
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(AppError::AiInvalid(
            "更新已有记忆时缺少完整正文".to_string(),
        ));
    }
    let allowed_relations = ["related", "supports", "contradicts", "part_of", "follows"];
    if proposal.action == "link"
        && !allowed_relations.contains(&proposal.relation_type.as_deref().unwrap_or_default())
    {
        return Err(AppError::AiInvalid("未知记忆关系类型".to_string()));
    }
    if proposal.action == "ask"
        && proposal
            .question
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(AppError::AiInvalid("澄清问题不能为空".to_string()));
    }
    Ok(())
}

fn insert_audit(
    transaction: &Transaction<'_>,
    actor: &str,
    action: &str,
    target_type: &str,
    target_id: &str,
    metadata: Value,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO audit_log
         (id, actor_type, action, target_type, target_id, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            Uuid::new_v4().to_string(),
            actor,
            action,
            target_type,
            target_id,
            metadata.to_string(),
            Utc::now().timestamp_millis()
        ],
    )?;
    Ok(())
}

fn map_memory_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemorySummary> {
    Ok(MemorySummary {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        lifecycle_status: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        summary: row.get(5)?,
        confidence: row.get(6)?,
        author_type: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        source_count: row.get(10)?,
    })
}

fn ensure_plan_columns(connection: &Connection) -> AppResult<()> {
    let mut statement = connection.prepare("PRAGMA table_info(plans)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    let migrations = [
        (
            "content",
            "ALTER TABLE plans ADD COLUMN content TEXT NOT NULL DEFAULT ''",
        ),
        ("link_url", "ALTER TABLE plans ADD COLUMN link_url TEXT"),
        ("notes", "ALTER TABLE plans ADD COLUMN notes TEXT"),
        (
            "reminded_at",
            "ALTER TABLE plans ADD COLUMN reminded_at INTEGER",
        ),
        (
            "feishu_synced_at",
            "ALTER TABLE plans ADD COLUMN feishu_synced_at INTEGER",
        ),
    ];
    for (column, sql) in migrations {
        if !columns.iter().any(|existing| existing == column) {
            connection.execute(sql, [])?;
        }
    }
    Ok(())
}

fn map_plan(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlanItem> {
    Ok(PlanItem {
        id: row.get(0)?,
        feed_event_id: row.get(1)?,
        title: row.get(2)?,
        details: row.get(3)?,
        content: row.get(4)?,
        link_url: row.get(5)?,
        notes: row.get(6)?,
        scheduled_at: row.get(7)?,
        status: row.get(8)?,
        clarification_question: row.get(9)?,
        source_title: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        reminded_at: row.get(13)?,
        feishu_synced_at: row.get(14)?,
    })
}

fn map_encrypted_secret(row: &rusqlite::Row<'_>) -> rusqlite::Result<EncryptedSecretRecord> {
    Ok(EncryptedSecretRecord {
        id: row.get(0)?,
        nonce: row.get(1)?,
        ciphertext: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        feishu_synced_at: row.get(5)?,
    })
}

pub(crate) fn validate_plan_proposal(
    proposal: &PlanProposal,
    scheduled_at: Option<i64>,
) -> AppResult<()> {
    if proposal.title.trim().is_empty() || proposal.title.chars().count() > 80 {
        return Err(AppError::AiInvalid("计划标题为空或超过 80 字".to_string()));
    }
    if proposal.details.trim().is_empty() || proposal.details.chars().count() > 4_000 {
        return Err(AppError::AiInvalid(
            "计划详情为空或超过 4000 字".to_string(),
        ));
    }
    if proposal.content.trim().is_empty() || proposal.content.chars().count() > 60 {
        return Err(AppError::AiInvalid("计划内容为空或超过 60 字".to_string()));
    }
    if let Some(link_url) = proposal.link_url.as_deref() {
        validate_http_url(link_url)?;
    }
    if proposal
        .notes
        .as_deref()
        .is_some_and(|notes| notes.chars().count() > 500)
    {
        return Err(AppError::AiInvalid("计划注意事项超过 500 字".to_string()));
    }
    if scheduled_at.is_none()
        && proposal
            .clarification_question
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(AppError::AiInvalid(
            "缺少时间时必须给出澄清问题".to_string(),
        ));
    }
    Ok(())
}

fn validate_http_url(value: &str) -> AppResult<()> {
    let url = reqwest::Url::parse(value.trim())
        .map_err(|_| AppError::AiInvalid("计划链接不是有效网址".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::AiInvalid(
            "计划链接只允许 http 或 https".to_string(),
        ));
    }
    Ok(())
}

fn count(connection: &Connection, sql: &str) -> AppResult<i64> {
    Ok(connection.query_row(sql, [], |row| row.get(0))?)
}

fn derive_title(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or(content).trim();
    let title: String = first_line.chars().take(36).collect();
    if first_line.chars().count() > 36 {
        format!("{title}...")
    } else {
        title
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn build_fts_query(value: &str) -> String {
    let query = value
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ");
    if query.is_empty() {
        "__feednote_no_match__".to_string()
    } else {
        query
    }
}

pub fn validate_llm_endpoint(endpoint: &str) -> AppResult<()> {
    let normalized = endpoint.trim().trim_end_matches('/');
    let local = normalized.starts_with("http://127.0.0.1:")
        || normalized.starts_with("http://localhost:")
        || normalized == "http://127.0.0.1"
        || normalized == "http://localhost";
    let authorized_cloud = normalized == "https://open.bigmodel.cn/api/anthropic";
    if !local && !authorized_cloud {
        return Err(AppError::Validation(
            "只允许连接已授权的智谱 Anthropic 地址或本机模型服务".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_embedding_endpoint(endpoint: &str) -> AppResult<()> {
    let normalized = endpoint.trim().trim_end_matches('/');
    let local = normalized.starts_with("http://127.0.0.1:")
        || normalized.starts_with("http://localhost:")
        || normalized == "http://127.0.0.1"
        || normalized == "http://localhost";
    let authorized_cloud = normalized == "https://open.bigmodel.cn/api/paas/v4";
    if !local && !authorized_cloud {
        return Err(AppError::Validation(
            "只允许连接已授权的智谱 Embedding 地址或本机服务".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_feishu_source_url(value: &str) -> AppResult<()> {
    let url = reqwest::Url::parse(value.trim())
        .map_err(|_| AppError::Validation("飞书来源必须是有效的 HTTPS 表格链接".to_string()))?;
    let host = url.host_str().unwrap_or_default();
    let allowed_host = host == "feishu.cn"
        || host.ends_with(".feishu.cn")
        || host == "larksuite.com"
        || host.ends_with(".larksuite.com");
    let segments: Vec<_> = url.path_segments().into_iter().flatten().collect();
    let has_sheet_token = segments
        .windows(2)
        .any(|parts| parts[0] == "sheets" && !parts[1].trim().is_empty());
    if url.scheme() != "https" || !allowed_host || !has_sheet_token {
        return Err(AppError::Validation(
            "飞书来源只允许 feishu.cn 或 larksuite.com 的 /sheets/ HTTPS 链接".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(content: &str) -> CreateFeedInput {
        CreateFeedInput {
            content: content.to_string(),
            source_type: None,
        }
    }

    fn plan_proposal(needs_clarification: bool) -> PlanProposal {
        PlanProposal {
            title: "提交桌面日记验收".to_string(),
            details: "检查选区投喂和桌面计划卡片。".to_string(),
            content: "功能验收".to_string(),
            link_url: Some("https://example.com/meeting".to_string()),
            notes: Some("准备测试数据".to_string()),
            scheduled_for: (!needs_clarification).then(|| "2026-08-31T09:00:00+08:00".to_string()),
            time_evidence: None,
            needs_clarification,
            clarification_question: needs_clarification.then(|| "准备在几点验收？".to_string()),
        }
    }

    #[test]
    fn selection_feed_plan_can_be_clarified_and_scheduled() {
        let database = Database::in_memory().unwrap();
        let feed = database
            .create_selection_feed(
                "明天验收桌面日记",
                "我明天验收桌面日记，但还没有约定几点。",
                "项目聊天",
            )
            .unwrap();
        let pending = database
            .create_plan(&feed.feed_id, &plan_proposal(true), None, "项目聊天")
            .unwrap();
        assert_eq!(pending.status, "needs_clarification");
        assert_eq!(database.list_plans(false).unwrap().len(), 1);

        let scheduled = database
            .schedule_plan(&pending.id, &plan_proposal(false), 1_788_134_400_000)
            .unwrap();
        assert_eq!(scheduled.status, "scheduled");
        assert_eq!(scheduled.scheduled_at, Some(1_788_134_400_000));
        assert_eq!(scheduled.content, "功能验收");
        assert_eq!(
            scheduled.link_url.as_deref(),
            Some("https://example.com/meeting")
        );
        assert_eq!(database.list_pending_feishu_plans(20).unwrap().len(), 1);
        database
            .mark_plan_feishu_synced(&scheduled.id, scheduled.updated_at + 1)
            .unwrap();
        assert!(database.list_pending_feishu_plans(20).unwrap().is_empty());

        let due = database
            .list_due_plan_reminders(1_788_130_000_000, 1_788_134_400_000)
            .unwrap();
        assert_eq!(due.len(), 1);
        database
            .mark_plan_reminded(&scheduled.id, 1_788_133_500_000)
            .unwrap();
        assert!(database
            .list_due_plan_reminders(1_788_130_000_000, 1_788_134_400_000)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn feishu_task_mapping_tracks_create_update_and_completion() {
        let database = Database::in_memory().unwrap();
        let feed = database
            .create_selection_feed("明天验收", "明天上午九点验收", "测试")
            .unwrap();
        let plan = database
            .create_plan(
                &feed.feed_id,
                &plan_proposal(false),
                Some(1_788_134_400_000),
                "测试",
            )
            .unwrap();
        assert_eq!(database.count_pending_feishu_plan_tasks().unwrap(), 1);

        database
            .save_feishu_plan_task_mapping(&FeishuPlanTaskMapping {
                plan_id: plan.id.clone(),
                task_guid: "task-guid".to_string(),
                task_url: Some("https://applink.feishu.cn/task".to_string()),
                plan_updated_at: plan.updated_at,
                completed: false,
            })
            .unwrap();
        assert_eq!(database.count_pending_feishu_plan_tasks().unwrap(), 0);

        let done = database.set_plan_done(&plan.id, true).unwrap();
        assert_eq!(database.count_pending_feishu_plan_tasks().unwrap(), 1);
        database
            .save_feishu_plan_task_mapping(&FeishuPlanTaskMapping {
                plan_id: plan.id,
                task_guid: "task-guid".to_string(),
                task_url: None,
                plan_updated_at: done.updated_at,
                completed: true,
            })
            .unwrap();
        assert_eq!(database.count_pending_feishu_plan_tasks().unwrap(), 0);
    }

    #[test]
    fn remote_completion_respects_local_changes_and_sheet_sync_state() {
        let database = Database::in_memory().unwrap();
        let feed = database
            .create_selection_feed("明天验收", "明天上午九点验收", "测试")
            .unwrap();
        let plan = database
            .create_plan(
                &feed.feed_id,
                &plan_proposal(false),
                Some(1_788_134_400_000),
                "测试",
            )
            .unwrap();

        assert!(database
            .apply_remote_plan_done(&plan.id, true, plan.updated_at, "feishu_sheet")
            .unwrap()
            .is_none());
        database
            .mark_plan_feishu_synced(&plan.id, plan.updated_at)
            .unwrap();
        let sheet_done = database
            .apply_remote_plan_done(&plan.id, true, plan.updated_at, "feishu_sheet")
            .unwrap()
            .unwrap();
        assert_eq!(sheet_done.status, "done");
        assert_eq!(sheet_done.feishu_synced_at, Some(sheet_done.updated_at));

        let local_reopened = database.set_plan_done(&plan.id, false).unwrap();
        assert!(database
            .apply_remote_plan_done(&plan.id, true, local_reopened.updated_at, "feishu_sheet")
            .unwrap()
            .is_none());
        let task_done = database
            .apply_remote_plan_done(&plan.id, true, local_reopened.updated_at, "feishu_task")
            .unwrap()
            .unwrap();
        assert_eq!(task_done.status, "done");
        assert!(task_done
            .feishu_synced_at
            .is_some_and(|value| value < task_done.updated_at));
        assert_eq!(database.list_pending_feishu_plans(20).unwrap().len(), 1);
    }

    #[test]
    fn migrates_existing_plan_table_without_rebuilding_user_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE plans (
                    id TEXT PRIMARY KEY,
                    feed_event_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    details TEXT NOT NULL,
                    scheduled_at INTEGER,
                    status TEXT NOT NULL,
                    clarification_question TEXT,
                    source_title TEXT NOT NULL DEFAULT '',
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );",
            )
            .unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        let connection = database.connection.lock().unwrap();
        let mut statement = connection.prepare("PRAGMA table_info(plans)").unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for expected in [
            "content",
            "link_url",
            "notes",
            "reminded_at",
            "feishu_synced_at",
        ] {
            assert!(columns.iter().any(|column| column == expected));
        }
    }

    #[test]
    fn migration_removes_legacy_source_plans_and_queues_remote_cleanup() {
        let database = Database::in_memory().unwrap();
        {
            let connection = database.connection.lock().unwrap();
            connection
                .execute(
                    "INSERT INTO feed_events (id, raw_content, source_type, created_at)
                     VALUES ('legacy-feed', '旧投递记录', 'feishu_source', 1)",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO plans
                     (id, feed_event_id, title, details, status, source_title, created_at, updated_at)
                     VALUES ('legacy-plan', 'legacy-feed', '旧投递计划', '不应展示',
                             'needs_clarification', '飞书投递记录·第 2 行', 1, 1)",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO feishu_source_rows (source_key, row_hash, plan_id, last_seen_at)
                     VALUES ('source-row', 'hash', 'legacy-plan', 1)",
                    [],
                )
                .unwrap();
        }

        database.migrate().unwrap();

        assert!(database.list_plans(true).unwrap().is_empty());
        assert_eq!(
            database.list_feishu_plan_cleanup_ids().unwrap(),
            vec!["legacy-plan"]
        );
    }

    #[test]
    fn feed_creates_immutable_source_and_memory() {
        let database = Database::in_memory().unwrap();
        let result = database.create_feed(input("周五前完成技术方案")).unwrap();
        let feeds = database.list_feeds(None, 20).unwrap();
        let memory = database.get_memory(&result.memory_id).unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(memory.versions.len(), 1);
        assert_eq!(memory.versions[0].body, "周五前完成技术方案");
        assert_eq!(memory.versions[0].author_type, "user");
    }

    #[test]
    fn dismissing_clarification_does_not_change_memory() {
        let database = Database::in_memory().unwrap();
        let result = database.create_feed(input("数据库使用 SQLite")).unwrap();
        let run = database.start_processing(&result.feed_id, "test").unwrap();
        let review = database
            .create_review(
                &run,
                &result.feed_id,
                &result.memory_id,
                &AiProposal {
                    memory_type: "Decision".to_string(),
                    title: "数据库决定".to_string(),
                    summary: "项目使用 SQLite".to_string(),
                    body: None,
                    action: "ask".to_string(),
                    target_memory_id: None,
                    relation_type: None,
                    confidence: 0.4,
                    reason: "缺少项目上下文".to_string(),
                    question: Some("这是哪个项目的数据库决定？".to_string()),
                },
                "test",
            )
            .unwrap();
        database.resolve_review(&review, false).unwrap();
        let memory = database.get_memory(&result.memory_id).unwrap();
        assert_eq!(memory.versions.len(), 1);
        assert_eq!(memory.memory.memory_type, "Unclassified");
    }

    #[test]
    fn ai_classification_is_applied_without_review() {
        let database = Database::in_memory().unwrap();
        let result = database
            .create_feed(input("周五前把 FeedNote 的搜索做完"))
            .unwrap();
        let run = database.start_processing(&result.feed_id, "test").unwrap();
        database
            .apply_ai_proposal(
                &run,
                &result.feed_id,
                &result.memory_id,
                &AiProposal {
                    memory_type: "Task".to_string(),
                    title: "完成 FeedNote 搜索".to_string(),
                    summary: "周五前完成 FeedNote 搜索功能。".to_string(),
                    body: Some("周五前完成 FeedNote 搜索功能。".to_string()),
                    action: "create".to_string(),
                    target_memory_id: None,
                    relation_type: None,
                    confidence: 0.92,
                    reason: "包含明确任务与截止时间".to_string(),
                    question: None,
                },
                "test",
            )
            .unwrap();

        let memory = database.get_memory(&result.memory_id).unwrap();
        let feeds = database.list_feeds(None, 10).unwrap();
        let reviews = database.list_reviews().unwrap();
        assert_eq!(memory.memory.memory_type, "Task");
        assert_eq!(memory.versions.len(), 2);
        assert_eq!(memory.versions[0].author_type, "ai");
        assert_eq!(feeds[0].processing_status, "classified");
        assert!(reviews.is_empty());
    }

    #[test]
    fn ai_updates_existing_memory_and_delete_rebuilds_from_remaining_sources() {
        let database = Database::in_memory().unwrap();
        let first = database
            .create_feed(input("FeedNote 搜索功能正在开发"))
            .unwrap();
        let first_run = database.start_processing(&first.feed_id, "test").unwrap();
        database
            .apply_ai_proposal(
                &first_run,
                &first.feed_id,
                &first.memory_id,
                &AiProposal {
                    memory_type: "Project".to_string(),
                    title: "FeedNote 搜索开发".to_string(),
                    summary: "搜索功能正在开发。".to_string(),
                    body: Some("FeedNote 搜索功能正在开发。".to_string()),
                    action: "create".to_string(),
                    target_memory_id: None,
                    relation_type: None,
                    confidence: 0.95,
                    reason: "新项目进展".to_string(),
                    question: None,
                },
                "test",
            )
            .unwrap();

        let second = database
            .create_feed(input("FeedNote 搜索功能已经完成"))
            .unwrap();
        let second_run = database.start_processing(&second.feed_id, "test").unwrap();
        database
            .apply_ai_proposal(
                &second_run,
                &second.feed_id,
                &second.memory_id,
                &AiProposal {
                    memory_type: "Project".to_string(),
                    title: "FeedNote 搜索开发".to_string(),
                    summary: "搜索功能已经完成。".to_string(),
                    body: Some("FeedNote 搜索功能已经从开发中推进到已完成。".to_string()),
                    action: "update".to_string(),
                    target_memory_id: Some(first.memory_id.clone()),
                    relation_type: None,
                    confidence: 0.96,
                    reason: "同一项目的新进展".to_string(),
                    question: None,
                },
                "test",
            )
            .unwrap();

        let memories = database.list_memories(None, None, 20).unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].source_count, 2);
        assert!(memories[0].body.contains("已完成"));
        assert!(database.get_memory(&second.memory_id).is_err());

        database.delete_feed(&second.feed_id).unwrap();
        let rebuilt = database.get_memory(&first.memory_id).unwrap();
        assert_eq!(rebuilt.memory.source_count, 1);
        assert!(rebuilt.memory.body.contains("正在开发"));
        assert!(!rebuilt.memory.body.contains("已经完成"));
        assert_eq!(rebuilt.memory.memory_type, "Unclassified");
    }

    #[test]
    fn endpoint_rejects_unauthorized_hosts() {
        assert!(validate_llm_endpoint("https://example.com").is_err());
        assert!(validate_llm_endpoint("https://open.bigmodel.cn/api/anthropic").is_ok());
        assert!(validate_llm_endpoint("http://127.0.0.1:11434").is_ok());
        assert!(validate_embedding_endpoint("https://open.bigmodel.cn/api/paas/v4").is_ok());
    }

    #[test]
    fn feishu_source_rejects_non_sheet_or_non_feishu_urls() {
        assert!(validate_feishu_source_url("https://team.feishu.cn/sheets/abc123").is_ok());
        assert!(validate_feishu_source_url("https://example.com/sheets/abc123").is_err());
        assert!(validate_feishu_source_url("https://team.feishu.cn/docx/abc123").is_err());
    }

    #[test]
    fn search_finds_original_content() {
        let database = Database::in_memory().unwrap();
        database
            .create_feed(input("OffscreenCanvas 可以把渲染移到 Worker"))
            .unwrap();
        let results = database
            .list_memories(Some("OffscreenCanvas".to_string()), None, 20)
            .unwrap();
        assert_eq!(results.len(), 1);
    }
}
