//! Migration có version, dùng `PRAGMA user_version` (AD-09).
//!
//! Bảng bám theo AD-09 của `ARCHITECTURE.md`. Cột `message_id` của `part` để
//! `NULL` ở M1 vì turn hiện chỉ lưu part, chưa tách message ra bảng riêng.

use rusqlite::Connection;

use ananke_core::StoreError;

const SCHEMA_V1: &str = r#"
CREATE TABLE session (
    id          TEXT PRIMARY KEY,
    parent_id   TEXT,
    workspace   TEXT NOT NULL DEFAULT '',
    worktree    TEXT,
    title       TEXT NOT NULL DEFAULT '',
    model       TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    archived_at INTEGER
);

CREATE TABLE turn (
    id         TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    status     TEXT NOT NULL,
    usage_json TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER
);
CREATE UNIQUE INDEX turn_session_seq ON turn (session_id, seq);

CREATE TABLE part (
    id           TEXT PRIMARY KEY,
    session_id   TEXT NOT NULL,
    turn_id      TEXT NOT NULL REFERENCES turn (id) ON DELETE CASCADE,
    message_id   TEXT,
    kind         TEXT NOT NULL,
    ordinal      INTEGER NOT NULL,
    content_json TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'complete'
);
CREATE INDEX part_turn_ordinal ON part (turn_id, ordinal);

-- Event log là write-ahead cho UI: append-only, `seq` toàn phần trong session.
CREATE TABLE event (
    session_id   TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    type         TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (session_id, seq)
);

CREATE TABLE permission (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    turn_id     TEXT NOT NULL,
    kind        TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    decision    TEXT,
    scope       TEXT,
    resolved_at INTEGER
);

CREATE TABLE todo (
    id         TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    content    TEXT NOT NULL,
    status     TEXT NOT NULL,
    priority   TEXT NOT NULL,
    ordinal    INTEGER NOT NULL
);

CREATE TABLE checkpoint (
    id         TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_id    TEXT NOT NULL,
    kind       TEXT NOT NULL,
    ref        TEXT NOT NULL
);
"#;

const SCHEMA_V2: &str = r#"
ALTER TABLE session ADD COLUMN permission_mode TEXT NOT NULL DEFAULT 'workspace-write';
"#;

const MIGRATIONS: &[&str] = &[SCHEMA_V1, SCHEMA_V2];

fn map_err(err: rusqlite::Error) -> StoreError {
    StoreError::Failed(err.to_string())
}

/// Chạy mọi migration còn thiếu, mỗi migration trong một transaction riêng.
pub(crate) fn apply(conn: &Connection) -> Result<(), StoreError> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(map_err)?;

    for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.unchecked_transaction().map_err(map_err)?;
        tx.execute_batch(sql).map_err(map_err)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)
            .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
    }

    Ok(())
}