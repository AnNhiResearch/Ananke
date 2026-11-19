//! Cài đặt [`ananke_core::SessionStore`] trên SQLite, kèm event log append-only.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ananke_core::event::now_millis;
use ananke_core::{
    Event, EventKind, Part, Session, SessionId, SessionStore, StoreError, Turn, TurnId, TurnStatus,
    Usage,
};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

const SQL_INSERT_EVENT: &str = "INSERT INTO event (session_id, seq, type, payload_json, created_at) \
     VALUES (?1, ?2, ?3, ?4, ?5)";

const SQL_UPSERT_TURN: &str = "INSERT INTO turn (id, session_id, seq, status, usage_json, started_at, ended_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
     ON CONFLICT (id) DO UPDATE SET \
        status = excluded.status, usage_json = excluded.usage_json, ended_at = excluded.ended_at";

const SQL_INSERT_PART: &str = "INSERT INTO part (id, session_id, turn_id, message_id, kind, ordinal, content_json, status) \
     VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, 'complete')";

/// Store SQLite: một connection duy nhất bọc trong `Mutex`.
///
/// SQLite là I/O cục bộ và các truy vấn ở đây đều ngắn, nên chạy đồng bộ là
/// đúng cho máy 1 GB (AD-19): không cần thread pool, không thêm bộ nhớ đệm.
/// Mở rộng với retention config và background prune task (M1).

/// Metadata session phía server, đọc/ghi thẳng vào bảng `session`.
///
/// Khác với [`Session`] của core (nặng, kèm toàn bộ turn), struct này chỉ chứa
/// những cột mà API danh sách/chi tiết session cần.
#[derive(Clone, Debug)]
pub struct SessionMeta {
    pub id: SessionId,
    pub parent_id: Option<SessionId>,
    pub workspace: String,
    pub worktree: Option<String>,
    pub title: String,
    pub model: String,
    pub created_at: u64,
    pub archived_at: Option<u64>,
}

/// Cấu hình retention/prune cho event log (M1).
#[derive(Clone, Debug)]
pub struct RetentionConfig {
    /// Số event tối đa giữ lại cho mỗi session (mặc định: 10000)
    pub max_events_per_session: u64,
    /// Tuổi tối đa của event tính bằng ngày (mặc định: 30)
    pub max_event_age_days: u32,
    /// Khoảng thời gian giữa các lần auto-prune tính bằng giây (mặc định: 3600 = 1h)
    pub prune_interval_secs: u64,
    /// Bật auto-prune định kỳ (mặc định: true)
    pub enable_auto_prune: bool,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_events_per_session: 10_000,
            max_event_age_days: 30,
            prune_interval_secs: 3_600,
            enable_auto_prune: true,
        }
    }
}

/// SqliteStore mở rộng với background prune task.
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
    retention_config: RetentionConfig,
    shutdown_tx: Option<broadcast::Sender<()>>,
    prune_handle: Option<JoinHandle<()>>,
}

impl SqliteStore {
    /// Mở (hoặc tạo) database tại `path`, chạy PRAGMA và migration.
    /// Sử dụng cấu hình retention mặc định.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::new_with_config(path, RetentionConfig::default())
    }

    /// Database trong RAM, dùng cho test.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::new_with_config(":memory:", RetentionConfig::default())
    }

    /// Tạo SqliteStore với cấu hình retention tùy chỉnh.
    /// Spawn background prune task nếu `enable_auto_prune = true`.
    pub fn new_with_config(path: impl AsRef<Path>, retention: RetentionConfig) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && path != Path::new(":memory:") {
                std::fs::create_dir_all(parent).map_err(failed)?;
            }
        }

        let conn = Connection::open(path).map_err(map_err)?;
        configure(&conn)?;
        crate::migrations::apply(&conn)?;

        let (shutdown_tx, _) = broadcast::channel(1);
        let conn_arc = Arc::new(Mutex::new(conn));
        let mut store = Self {
            conn: conn_arc.clone(),
            retention_config: retention.clone(),
            shutdown_tx: Some(shutdown_tx.clone()),
            prune_handle: None,
        };

        if retention.enable_auto_prune {
            store.spawn_prune_task(shutdown_tx.subscribe());
        }

        Ok(store)
    }

    fn spawn_prune_task(&mut self, mut shutdown_rx: broadcast::Receiver<()>) {
        let interval = Duration::from_secs(self.retention_config.prune_interval_secs);
        let config = self.retention_config.clone();
        let conn = self.conn_clone();

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        if let Err(e) = Self::prune_old_events_static(&conn, &config) {
                            eprintln!("Auto-prune failed: {e}");
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });

        self.prune_handle = Some(handle);
    }

    /// Static version of prune_old_events for use in background task
    fn prune_old_events_static(conn: &Arc<Mutex<Connection>>, config: &RetentionConfig) -> Result<usize, StoreError> {
        let conn_guard = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut total_deleted = 0;

        // Get all active session IDs (archived_at IS NULL)
        let session_ids: Vec<String> = {
            let mut stmt = conn_guard.prepare(
                "SELECT id FROM session WHERE archived_at IS NULL"
            ).map_err(map_err)?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(map_err)?;
            rows.filter_map(Result::ok).collect()
        };

        let max_age_ms = config.max_event_age_days as i64 * 24 * 60 * 60 * 1000;
        let cutoff_time = now_millis() as i64 - max_age_ms;

        for session_id in session_ids {
            // Delete events older than max_event_age_days
            let deleted_old = conn_guard.execute(
                "DELETE FROM event WHERE session_id = ?1 AND created_at < ?2",
                params![session_id, cutoff_time],
            ).map_err(map_err)?;
            total_deleted += deleted_old;

            // Keep at most max_events_per_session newest events
            let deleted_excess = conn_guard.execute(
                "DELETE FROM event WHERE session_id = ?1 AND seq <= (
                    SELECT COALESCE(MAX(seq), 0) - ?2 FROM event WHERE session_id = ?1
                )",
                params![session_id, config.max_events_per_session as i64],
            ).map_err(map_err)?;
            total_deleted += deleted_excess;
        }

        Ok(total_deleted)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Get a clone of the inner connection Arc for background tasks
    fn conn_clone(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    /// Ghi metadata session. `turn` và `message` do [`SessionStore`] quản lý.
    pub fn save_session(&self, session: &Session) -> Result<(), StoreError> {
        let conn = self.lock();
        let mode_str = match session.permission_mode {
            ananke_core::PermissionMode::ReadOnly => "read-only",
            ananke_core::PermissionMode::WorkspaceWrite => "workspace-write",
            ananke_core::PermissionMode::DangerFullAccess => "danger-full-access",
            ananke_core::PermissionMode::External => "external",
        };
        conn.execute(
            "INSERT INTO session (id, parent_id, workspace, worktree, title, model, permission_mode, created_at, archived_at) \
             VALUES (?1, NULL, '', NULL, ?2, ?3, ?4, ?5, NULL) \
             ON CONFLICT (id) DO UPDATE SET title = excluded.title, model = excluded.model, permission_mode = excluded.permission_mode",
            params![
                session.id.as_str(),
                &session.title,
                &session.model,
                mode_str,
                now_millis() as i64
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    /// Nạp lại session cùng toàn bộ turn. Trả `NotFound` nếu chưa `save_session`.
    pub fn load_session(&self, session_id: &SessionId) -> Result<Session, StoreError> {
        let (title, model, mode_str) = {
            let conn = self.lock();
            conn.query_row(
                "SELECT title, model, permission_mode FROM session WHERE id = ?1",
                [session_id.as_str()],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2).unwrap_or_else(|_| "workspace-write".to_string()),
                )),
            )
            .optional()
            .map_err(map_err)?
            .ok_or_else(|| StoreError::NotFound(format!("session {session_id}")))?
        };

        let mode = match mode_str.as_str() {
            "read-only" => ananke_core::PermissionMode::ReadOnly,
            "danger-full-access" => ananke_core::PermissionMode::DangerFullAccess,
            "external" => ananke_core::PermissionMode::External,
            _ => ananke_core::PermissionMode::WorkspaceWrite,
        };

        let mut session = Session::new(model).with_title(title).with_permission_mode(mode);
        session.id = session_id.clone();
        for turn in self.read_turns(session_id)? {
            session.push_turn(turn);
        }
        Ok(session)
    }

    /// Append event vào log. `part.delta` bị bỏ qua theo AD-08; trả số event đã ghi.
    pub fn append_events(&self, events: &[Event]) -> Result<usize, StoreError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(map_err)?;
        let written = write_events(&tx, events)?;
        tx.commit().map_err(map_err)?;
        Ok(written)
    }

    /// Replay event có `seq` lớn hơn `since` (client nối muộn hoặc mất kết nối).
    pub fn events_since(
        &self,
        session_id: &SessionId,
        since: u64,
    ) -> Result<Vec<Event>, StoreError> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT seq, payload_json, created_at FROM event \
                 WHERE session_id = ?1 AND seq > ?2 ORDER BY seq",
            )
            .map_err(map_err)?;

        let rows = stmt
            .query_map(params![session_id.as_str(), since as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(map_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_err)?;

        rows.into_iter()
            .map(|(seq, payload, at)| {
                Ok(Event {
                    seq: seq as u64,
                    session_id: session_id.clone(),
                    at: at as u64,
                    kind: parse_json::<EventKind>(&payload, "event.payload_json")?,
                })
            })
            .collect()
    }

    /// Retention (AD-19): chỉ giữ `keep_last` event mới nhất của session.
    pub fn prune_events(&self, session_id: &SessionId, keep_last: u64) -> Result<usize, StoreError> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM event WHERE session_id = ?1 AND seq <= ( \
                SELECT COALESCE(MAX(seq), 0) - ?2 FROM event WHERE session_id = ?1 \
             )",
            params![session_id.as_str(), keep_last as i64],
        )
        .map_err(map_err)
    }

    /// Checkpoint WAL rồi truncate để file WAL không phình trên flash nhỏ (AD-19).
    pub fn checkpoint(&self) -> Result<(), StoreError> {
        let conn = self.lock();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(map_err)
    }

    /// Prune old events for all active sessions (archived_at IS NULL).
    /// Deletes events older than `max_event_age_days` and keeps at most
    /// `max_events_per_session` newest events per session.
    /// Returns total number of deleted events.
    pub fn prune_old_events(&self) -> Result<usize, StoreError> {
        Self::prune_old_events_static(&self.conn_clone(), &self.retention_config)
    }

    /// Public API để trigger prune thủ công từ bên ngoài.
    /// Giống `prune_old_events` nhưng rõ ràng hơn về mục đích.
    pub fn prune_all(&self) -> Result<usize, StoreError> {
        self.prune_old_events()
    }

    /// Tắt background prune task gracefully.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.prune_handle.take() {
            handle.abort();
        }
    }

    /// Ghi hoặc cập nhật metadata session (upsert theo `id`).
    pub fn upsert_session_meta(&self, meta: &SessionMeta) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO session (id, parent_id, workspace, worktree, title, model, created_at, archived_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT (id) DO UPDATE SET \
                parent_id = excluded.parent_id, \
                workspace = excluded.workspace, \
                worktree = excluded.worktree, \
                title = excluded.title, \
                model = excluded.model, \
                created_at = excluded.created_at, \
                archived_at = excluded.archived_at",
            params![
                meta.id.as_str(),
                meta.parent_id.as_ref().map(|parent| parent.as_str()),
                &meta.workspace,
                meta.worktree.as_deref(),
                &meta.title,
                &meta.model,
                meta.created_at as i64,
                meta.archived_at.map(|at| at as i64),
            ],
        )?;
        Ok(())
    }

    /// Nạp metadata của một session; `None` nếu chưa tồn tại.
    pub fn get_session_meta(&self, id: &SessionId) -> rusqlite::Result<Option<SessionMeta>> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, parent_id, workspace, worktree, title, model, created_at, archived_at \
             FROM session WHERE id = ?1",
            [id.as_str()],
            row_to_session_meta,
        )
        .optional()
    }

    /// Liệt kê session chưa archive, mới nhất trước.
    pub fn list_session_meta(&self) -> rusqlite::Result<Vec<SessionMeta>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, parent_id, workspace, worktree, title, model, created_at, archived_at \
             FROM session WHERE archived_at IS NULL ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], row_to_session_meta)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Cập nhật `title`/`model`; `None` giữ nguyên giá trị cũ. `false` nếu session không tồn tại.
    pub fn update_session_meta(
        &self,
        id: &SessionId,
        title: Option<&str>,
        model: Option<&str>,
    ) -> rusqlite::Result<bool> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE session SET title = COALESCE(?2, title), model = COALESCE(?3, model) \
             WHERE id = ?1",
            params![id.as_str(), title, model],
        )?;
        Ok(changed > 0)
    }

    /// Đánh dấu archive (chỉ set khi `archived_at` còn NULL). `false` nếu session không tồn tại.
    pub fn archive_session(&self, id: &SessionId) -> rusqlite::Result<bool> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE session SET archived_at = COALESCE(archived_at, ?2) WHERE id = ?1",
            params![id.as_str(), now_millis() as i64],
        )?;
        Ok(changed > 0)
    }

    /// Xoá session cùng toàn bộ dữ liệu con trong một transaction. `false` nếu không tồn tại.
    pub fn delete_session(&self, id: &SessionId) -> rusqlite::Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;

        let exists = tx
            .query_row("SELECT 1 FROM session WHERE id = ?1", [id.as_str()], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        if !exists {
            return Ok(false);
        }

        tx.execute("DELETE FROM event WHERE session_id = ?1", [id.as_str()])?;
        tx.execute(
            "DELETE FROM part WHERE session_id = ?1 \
             OR turn_id IN (SELECT id FROM turn WHERE session_id = ?1)",
            [id.as_str()],
        )?;
        tx.execute("DELETE FROM turn WHERE session_id = ?1", [id.as_str()])?;
        tx.execute("DELETE FROM permission WHERE session_id = ?1", [id.as_str()])?;
        tx.execute("DELETE FROM todo WHERE session_id = ?1", [id.as_str()])?;
        tx.execute("DELETE FROM checkpoint WHERE session_id = ?1", [id.as_str()])?;
        tx.execute("DELETE FROM session WHERE id = ?1", [id.as_str()])?;
        tx.commit()?;
        Ok(true)
    }

    /// Save AllowAlways scope for a permission request.
    /// Used for future auto-approval of same tool+args+scope.
    pub fn save_allow_always_scope(
        &self,
        permission_id: &ananke_core::PermissionId,
        session_id: &SessionId,
        scope: &str,
    ) -> Result<(), StoreError> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO permission (id, session_id, turn_id, kind, payload_json, decision, scope, resolved_at) \
             VALUES (?1, ?2, '', '', '', 'allow_always', ?3, ?4) \
             ON CONFLICT (id) DO UPDATE SET scope = excluded.scope, decision = excluded.decision, resolved_at = excluded.resolved_at",
            params![
                permission_id.as_str(),
                session_id.as_str(),
                scope,
                now_millis() as i64
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    /// Get persisted AllowAlways scope for a permission request.
    /// Returns None if not found or not an AllowAlways decision.
    pub fn get_allow_always_scope(
        &self,
        permission_id: &ananke_core::PermissionId,
    ) -> Result<Option<String>, StoreError> {
        let conn = self.lock();
        let scope: Option<String> = conn
            .query_row(
                "SELECT scope FROM permission WHERE id = ?1 AND decision = 'allow_always'",
                [permission_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_err)?;
        Ok(scope)
    }

    /// `seq` event lớn nhất của session, `0` nếu chưa có event nào.
    pub fn max_event_seq(&self, id: &SessionId) -> rusqlite::Result<u64> {
        let conn = self.lock();
        let seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM event WHERE session_id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )?;
        Ok(seq as u64)
    }

    /// Session có tồn tại trong bảng `session` hay không.
    pub fn session_exists(&self, id: &SessionId) -> rusqlite::Result<bool> {
        let conn = self.lock();
        let found = conn
            .query_row("SELECT 1 FROM session WHERE id = ?1", [id.as_str()], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        Ok(found)
    }

    /// Ghi turn và event trong **cùng một transaction**: hoặc cả hai cùng vào,
    /// hoặc cả hai cùng biến mất (AD-09, không có event "ma" khi rollback).
    pub fn save_turn_with_events(
        &self,
        session_id: &SessionId,
        turn: &Turn,
        events: &[Event],
    ) -> Result<(), StoreError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(map_err)?;
        write_turn(&tx, session_id, turn)?;
        write_events(&tx, events)?;
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn read_turns(&self, session_id: &SessionId) -> Result<Vec<Turn>, StoreError> {
        let conn = self.lock();
        let mut turn_stmt = conn
            .prepare("SELECT id, status, usage_json FROM turn WHERE session_id = ?1 ORDER BY seq")
            .map_err(map_err)?;

        let rows = turn_stmt
            .query_map([session_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(map_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_err)?;
        drop(turn_stmt);

        let mut part_stmt = conn
            .prepare("SELECT content_json FROM part WHERE turn_id = ?1 ORDER BY ordinal")
            .map_err(map_err)?;

        let mut turns = Vec::with_capacity(rows.len());
        for (id, status, usage) in rows {
            let parts = part_stmt
                .query_map([id.as_str()], |row| row.get::<_, String>(0))
                .map_err(map_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_err)?;

            turns.push(Turn {
                id: TurnId::from(id),
                status: parse_json::<TurnStatus>(&status, "turn.status")?,
                parts: parts
                    .iter()
                    .map(|raw| parse_json::<Part>(raw, "part.content_json"))
                    .collect::<Result<Vec<_>, _>>()?,
                usage: parse_json::<Usage>(&usage, "turn.usage_json")?,
                cancelled: std::sync::atomic::AtomicBool::new(false),
            });
        }

        Ok(turns)
    }
}

#[async_trait]
impl SessionStore for SqliteStore {
    async fn save_turn(&self, session_id: &SessionId, turn: &Turn) -> Result<(), StoreError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(map_err)?;
        write_turn(&tx, session_id, turn)?;
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    async fn load_turns(&self, session_id: &SessionId) -> Result<Vec<Turn>, StoreError> {
        self.read_turns(session_id)
    }

    async fn set_turn_status(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        status: &TurnStatus,
    ) -> Result<(), StoreError> {
        let status_json = serde_json::to_string(status).map_err(serde_err)?;
        let ended_at = status.is_terminal().then(|| now_millis() as i64);

        let conn = self.lock();
        let changed = conn
            .execute(
                "UPDATE turn SET status = ?1, ended_at = ?2 WHERE id = ?3 AND session_id = ?4",
                params![status_json, ended_at, turn_id.as_str(), session_id.as_str()],
            )
            .map_err(map_err)?;

        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "turn {turn_id} trong session {session_id}"
            )));
        }
        Ok(())
    }
}

/// PRAGMA theo AD-19: ít RAM, WAL, không mmap trên Termux.
/// Added PRAGMA wal_autocheckpoint = 1000 for periodic TRUNCATE checkpoint.
fn configure(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -2000;
         PRAGMA mmap_size = 0;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA wal_autocheckpoint = 1000;",
    )
    .map_err(map_err)
}

impl Drop for SqliteStore {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn write_turn(tx: &Transaction<'_>, session_id: &SessionId, turn: &Turn) -> Result<(), StoreError> {
    let now = now_millis() as i64;
    let status = serde_json::to_string(&turn.status).map_err(serde_err)?;
    let usage = serde_json::to_string(&turn.usage).map_err(serde_err)?;
    let ended_at = turn.status.is_terminal().then_some(now);

    let seq: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM turn WHERE session_id = ?1",
            [session_id.as_str()],
            |row| row.get(0),
        )
        .map_err(map_err)?;

    tx.execute(
        SQL_UPSERT_TURN,
        params![
            turn.id.as_str(),
            session_id.as_str(),
            seq,
            status,
            usage,
            now,
            ended_at
        ],
    )
    .map_err(map_err)?;

    tx.execute("DELETE FROM part WHERE turn_id = ?1", [turn.id.as_str()])
        .map_err(map_err)?;

    for (ordinal, part) in turn.parts.iter().enumerate() {
        tx.execute(
            SQL_INSERT_PART,
            params![
                part.id().as_str(),
                session_id.as_str(),
                turn.id.as_str(),
                part_kind_name(part),
                ordinal as i64,
                serde_json::to_string(part).map_err(serde_err)?
            ],
        )
        .map_err(map_err)?;
    }

    Ok(())
}

fn write_events(tx: &Transaction<'_>, events: &[Event]) -> Result<usize, StoreError> {
    let mut written = 0;
    for event in events {
        // AD-08: delta chỉ để stream, không bao giờ vào đĩa.
        if !event.kind.is_persistent() {
            continue;
        }

        let payload = serde_json::to_value(&event.kind).map_err(serde_err)?;
        let kind = payload
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();

        tx.execute(
            SQL_INSERT_EVENT,
            params![
                event.session_id.as_str(),
                event.seq as i64,
                kind,
                payload.to_string(),
                event.at as i64
            ],
        )
        .map_err(map_err)?;
        written += 1;
    }
    Ok(written)
}

fn part_kind_name(part: &Part) -> &'static str {
    match part {
        Part::Text { .. } => "text",
        Part::Reasoning { .. } => "reasoning",
        Part::ToolCall { .. } => "tool_call",
        Part::ToolResult { .. } => "tool_result",
    }
}

fn row_to_session_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionMeta> {
    Ok(SessionMeta {
        id: SessionId::from(row.get::<_, String>(0)?),
        parent_id: row.get::<_, Option<String>>(1)?.map(SessionId::from),
        workspace: row.get(2)?,
        worktree: row.get(3)?,
        title: row.get(4)?,
        model: row.get(5)?,
        created_at: row.get::<_, i64>(6)? as u64,
        archived_at: row.get::<_, Option<i64>>(7)?.map(|at| at as u64),
    })
}

fn parse_json<T: DeserializeOwned>(raw: &str, field: &str) -> Result<T, StoreError> {
    serde_json::from_str(raw)
        .map_err(|err| StoreError::Failed(format!("{field} không đọc được: {err}")))
}

fn map_err(err: rusqlite::Error) -> StoreError {
    StoreError::Failed(err.to_string())
}

fn serde_err(err: serde_json::Error) -> StoreError {
    StoreError::Failed(err.to_string())
}

fn failed(err: std::io::Error) -> StoreError {
    StoreError::Failed(err.to_string())
}