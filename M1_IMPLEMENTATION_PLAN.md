# M1 Milestone Implementation Plan

**Version:** 1.0  
**Date:** 2026-11-17  
**Scope:** Turn & Approval (AD-06, AD-07, AD-08, AD-15, AD-17, AD-19)

---

## Overview

M1 completes the agent loop with:
- Cancel/interrupt mechanism + resume awaiting_tool (AD-17)
- Retention/prune event log + pragmas (AD-19)
- Interrupt/steer endpoints, admission control 429, resync_required, permission scope
- Extended EventPayloadDto for M1 events + regen OpenAPI
- HTTP tests for DoD §11

---

## 1. ananke-core: Cancel/Interrupt + AD-17 Resume

### Files to Modify
| File | Action | Description |
|------|--------|-------------|
| `crates/ananke-core/src/engine.rs` | **Modify** | Already has `cancel_turn`, `steer_turn`, `resume_turn` - verify completeness |
| `crates/ananke-core/src/turn.rs` | **Modify** | Add `is_cancelled()` check, `cancel()` method, `pending_tool_calls()` |
| `crates/ananke-core/src/event.rs` | **Modify** | Add `EventKind::TurnInterrupted`, `EventKind::ResourceThrottled` |
| `crates/ananke-core/src/approval.rs` | **No change** | Already supports `Decision` variants |

### Public Interfaces

#### Engine (engine.rs)
```rust
// Already implemented - verify:
pub async fn cancel_turn(&self, session: &mut Session, turn_id: &TurnId) -> Result<(), EngineError>
pub async fn steer_turn(&self, session: &mut Session, turn_id: &TurnId, new_input: Vec<Message>) -> Result<(), EngineError>
pub async fn resume_turn(&self, session: &mut Session, turn_id: &TurnId) -> Result<(), EngineError>
```

#### Turn (turn.rs) - Add/Verify
```rust
pub fn is_cancelled(&self) -> bool           // Returns true if cancelled flag set
pub fn cancel(&mut self)                      // Sets cancelled flag
pub fn pending_tool_calls(&self) -> Vec<(ToolCallId, String, Value)>  // For resume
```

#### EventKind (event.rs) - Add
```rust
TurnInterrupted { turn_id: TurnId }
ResourceThrottled { limit: String, current: u32, action: String }
```

### Dependencies
- `turn.rs` → used by `engine.rs` for status checks
- `event.rs` → used by `engine.rs` for event emission

### Acceptance Criteria (DoD §11)
- [ ] Turn persist trước khi dispatch tool (AD-17 verified in test `persist_turn_truoc_dispatch_tool_ad_17`)
- [ ] `kill -9` giữa turn rồi restart vẫn đúng lịch sử, turn `awaiting_tool` tự hoàn tất
- [ ] Interrupt/steer endpoints return 204, turn status updated correctly

---

## 2. ananke-store: Retention/Prune + Pragmas (AD-19)

### Files to Modify
| File | Action | Description |
|------|--------|-------------|
| `crates/ananke-store/src/store.rs` | **Modify** | Already has `RetentionConfig`, `prune_old_events`, background task - verify completeness |
| `crates/ananke-store/src/migrations.rs` | **No change** | Schema already at V1 |

### Public Interfaces (Already Implemented - Verify)

#### RetentionConfig (store.rs)
```rust
pub struct RetentionConfig {
    pub max_events_per_session: u64,      // Default: 10,000
    pub max_event_age_days: u32,          // Default: 30
    pub prune_interval_secs: u64,         // Default: 3600 (1h)
    pub enable_auto_prune: bool,          // Default: true
}
```

#### SqliteStore Methods
```rust
pub fn new_with_config(path: impl AsRef<Path>, retention: RetentionConfig) -> Result<Self, StoreError>
pub fn prune_events(&self, session_id: &SessionId, keep_last: u64) -> Result<usize, StoreError>
pub fn prune_old_events(&self) -> Result<usize, StoreError>  // Auto-prune all sessions
pub fn prune_all(&self) -> Result<usize, StoreError>         // Manual trigger
pub fn checkpoint(&self) -> Result<(), StoreError>           // WAL TRUNCATE
pub fn shutdown(&mut self)                                    // Graceful shutdown
```

#### PRAGMA Configuration (configure function)
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -2000;        -- 2MB cache
PRAGMA mmap_size = 0;             -- Disable mmap (Termux)
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA wal_autocheckpoint = 1000; -- Periodic TRUNCATE
```

### Dependencies
- Background prune task uses `tokio::spawn` with `broadcast::channel` for shutdown
- `prune_old_events_static` called by background task

### Acceptance Criteria (DoD §11)
- [ ] Event log retention bật từ M1 (không đợi M3)
- [ ] `part.delta` không persist; retention + checkpoint `TRUNCATE` bật từ M1 (AD-19)
- [ ] Trên máy 1 GB: RSS service idle + 2 session < 80 MB

---

## 3. ananke-server: Interrupt/Steer, Admission Control 429, Resync, Permission Scope

### Files to Modify
| File | Action | Description |
|------|--------|-------------|
| `crates/ananke-server/src/routes/turn.rs` | **Modify** | Already has `interrupt_turn`, `steer_turn` - verify 429 admission control |
| `crates/ananke-server/src/state.rs` | **Modify** | Already has `active_turns` atomic counter for admission control |
| `crates/ananke-server/src/routes/sse.rs` | **Modify** | Already has `resync_required` gap detection - verify |
| `crates/ananke-server/src/approval.rs` | **Modify** | Already has `AllowAlways` scope persistence - verify |
| `crates/ananke-server/src/hub.rs` | **Modify** | Already has `emit_resource_throttled` - verify |
| `crates/ananke-server/src/routes/permission.rs` | **Verify/Modify** | Permission decision endpoint with scope |

### Public Interfaces

#### Turn Routes (turn.rs)
```rust
// Already implemented - verify:
POST /api/session/{id}/turn/{turnID}/interrupt  -> 204, turn status = Interrupted
POST /api/session/{id}/turn/{turnID}/steer       -> 204, turn continues with new input

// Admission control (AD-15):
// - max_resident_sessions: 429 + resource.throttled event
// - max_parallel_turns: 429 + resource.throttled event
```

#### Session Registry (state.rs)
```rust
// Already implemented - verify:
pub fn global_active_turn_count(&self) -> usize        // Atomic counter
pub fn increment_active_turns(&self)                   // Called when turn starts
pub fn decrement_active_turns(&self)                   // Called when turn ends
pub async fn resident_session_count(&self) -> usize    // For session limit
```

#### SSE with Resync (sse.rs)
```rust
// Already implemented - verify:
GET /api/session/{id}/event?since={seq}
GET /api/session/{id}/event  (with Last-Event-ID header)

// Gap detection -> EventResyncRequired {
    last_server_seq: u64,
    reason: String
}
```

#### Approval with Scope (approval.rs)
```rust
// Already implemented - verify:
pub fn resolve(&self, id: &PermissionId, decision: Decision, scope: Option<String>, session_id: &SessionId) -> bool
pub fn get_allow_always_scope(&self, id: &PermissionId) -> Option<String>  // Memory + store fallback
```

#### EventHub Resource Throttling (hub.rs)
```rust
// Already implemented - verify:
pub fn emit_resource_throttled(&self, limit: String, current: u32, action: String)
```

### Dependencies
- `turn.rs` → uses `state.rs` (SessionRegistry, limits)
- `sse.rs` → uses `hub.rs` (subscribe_from, gap detection)
- `approval.rs` → uses `store.rs` (save_allow_always_scope, get_allow_always_scope)
- `hub.rs` → uses `store.rs` (max_event_seq, append_events)

### Acceptance Criteria (DoD §11)
- [ ] Vượt giới hạn → xếp hàng hoặc `429 limit_exceeded`, kèm event `resource.throttled`. **Không bao giờ OOM im lặng.**
- [ ] SSE replay qua `since`/`Last-Event-ID` hoạt động đúng
- [ ] Permission `allow_always` scope (once/session/project) persist và auto-approve session mới
- [ ] Event mới có `seq`, có test replay qua `since`

---

## 4. ananke-protocol: Extend EventPayloadDto + Regen OpenAPI

### Files to Modify
| File | Action | Description |
|------|--------|-------------|
| `crates/ananke-protocol/src/event.rs` | **Modify** | Already has `TurnInterrupted`, `ResourceThrottled`, `EventResyncRequired` - verify |
| `crates/ananke-protocol/src/turn.rs` | **Verify** | Already has `TurnStatusDto::Interrupted`, `TurnStatusDto::AwaitingTool` |
| `crates/ananke-protocol/src/permission.rs` | **Verify** | Already has `PermissionScopeDto`, `PermissionDecisionDto::AllowAlways` |
| `crates/ananke-protocol/src/openapi.rs` | **No change** | Generator picks up DTO changes |
| `crates/ananke-protocol/src/bin/gen.rs` | **No change** | CLI for regeneration |

### Public Interfaces (Already Defined - Verify)

#### EventPayloadDto (event.rs)
```rust
// M1 additions - already present:
TurnInterrupted { turn_id: String }
ResourceThrottled { limit: String, current: u32, action: ThrottleAction }
EventResyncRequired { last_server_seq: u64, reason: String }
ThrottleAction { Queued, Rejected }
```

#### TurnStatusDto (turn.rs)
```rust
// Already present:
Running, AwaitingTool, Completed, Interrupted, Failed { reason }
```

#### Permission Types (permission.rs)
```rust
// Already present:
PermissionScopeDto { Once, Session, Project }
PermissionDecisionDto { Allow, AllowAlways, Deny }
PermissionDecisionRequest { decision, scope? }
```

### Regenerate OpenAPI
```bash
cargo run -p ananke-protocol -- gen openapi
# Verify openapi.json updated
```

### Acceptance Criteria (DoD §11)
- [ ] Thay đổi protocol có cập nhật `openapi.json` và chạy lại codegen trong cùng PR
- [ ] Client bỏ qua event type chưa biết (forward compatibility)

---

## 5. HTTP Tests for DoD §11

### Files to Modify
| File | Action | Description |
|------|--------|-------------|
| `crates/ananke-server/tests/api.rs` | **Modify** | Add/verify M1 test cases |

### Test Cases (Already Implemented - Verify)

| Test | DoD Criteria | Status |
|------|--------------|--------|
| `part_delta_khong_persist_trong_replay` | PartDelta không persist; test khẳng định điều này | ✅ Exists |
| `replay_qua_since_va_last_event_id` | Event mới có seq, test replay qua since | ✅ Exists |
| `persist_turn_truoc_dispatch_tool_ad_17` | Turn persist trước dispatch tool; kill -9 không mất lịch sử | ✅ Exists |
| `limit_exceeded_max_resident_sessions` | Vượt max_sessions → 429 + resource.throttled | ✅ Exists |
| `limit_exceeded_max_parallel_turns` | Vượt max_parallel_turns → 429 + resource.throttled | ✅ Exists |
| `interrupt_turn_endpoint` | POST /interrupt → 204, status Interrupted | ✅ Exists |
| `steer_turn_endpoint` | POST /steer → 204, turn continues | ✅ Exists |
| `sse_resync_required_stale_last_event_id` | Resync_required on stale Last-Event-ID | ✅ Exists |
| `sse_resync_required_gap_detection` | Resync_required on gap detection | ✅ Exists |
| `permission_allow_always_scope` | AllowAlways scope persist + auto-approve new session | ✅ Exists |

### Additional Tests Needed (If Missing)
- [ ] Test `resume_turn` via HTTP (currently no endpoint - engine method only)
- [ ] Test checkpoint/prune background task
- [ ] Test `max_context_tokens` / `max_part_bytes` limits
- [ ] Test `max_subagents` limit

### Run Tests
```bash
cargo test -p ananke-server --test api
```

---

## Component Dependency Graph

```
┌─────────────────────────────────────────────────────────────────┐
│                      M1 COMPONENT DEPENDENCIES                   │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ananke-protocol (DTOs + OpenAPI)                               │
│       ▲                    ▲                      ▲              │
│       │                    │                      │              │
│       │                    │                      │              │
│  ananke-core ◄──────┐      │                      │              │
│  (engine, turn,     │      │                      │              │
│   event, approval)  │      │                      │              │
│       ▲             │      │                      │              │
│       │             │      │                      │              │
│  ananke-store ◄─────┘      │                      │              │
│  (retention, PRAGMA,      │                      │              │
│   prune, checkpoint)      │                      │              │
│       ▲                   │                      │              │
│       │                   │                      │              │
│  ananke-server ◄──────────┴──────────────────────┘              │
│  (routes, state, hub, approval, SSE, tests)                     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Implementation Order

| Phase | Component | Tasks | Estimated |
|-------|-----------|-------|-----------|
| 1 | **Verify Core** | Confirm `cancel_turn`, `steer_turn`, `resume_turn` complete in engine.rs; verify Turn events | 1h |
| 2 | **Verify Store** | Confirm `RetentionConfig`, background prune, PRAGMA config, checkpoint | 1h |
| 3 | **Verify Server** | Confirm interrupt/steer endpoints, admission control 429, SSE resync, permission scope | 2h |
| 4 | **Verify Protocol** | Confirm EventPayloadDto has all M1 events; regen openapi.json | 0.5h |
| 5 | **Run Tests** | Execute all M1 HTTP tests in api.rs; fix any failures | 2h |
| 6 | **Integration** | Full integration test: create session → turn → interrupt → steer → resume → replay | 1h |

**Total Estimate: ~7.5 hours**

---

## Key Acceptance Checklist (DoD §11)

| # | Criterion | Verification |
|---|-----------|--------------|
| 1 | Không thêm dependency từ `ananke-core` sang `protocol`/`server` | `cargo tree -p ananke-core` |
| 2 | Không thêm import engine vào client packages | Manual review |
| 3 | Thay đổi protocol có cập nhật `openapi.json` + codegen | `cargo run -p ananke-protocol -- gen openapi` + git diff |
| 4 | Endpoint mới có test ở mức HTTP | `cargo test -p ananke-server --test api` |
| 5 | Event mới có `seq`, test replay qua `since` | `replay_qua_since_va_last_event_id` |
| 6 | Endpoint ghi dữ liệu có permission check | `permission_allow_always_scope` |
| 7 | Không log secret; không trả API key | Code review |
| 8 | `part.delta` không ghi event log | `part_delta_khong_persist_trong_replay` |
| 9 | Mọi giới hạn AD-15 có test: vượt cap → 429/throttled, không OOM | `limit_exceeded_*` |
| 10 | Turn persist trước dispatch tool; kill -9 không mất lịch sử | `persist_turn_truoc_dispatch_tool_ad_17` |
| 11 | Build `aarch64-linux-android` chạy trong CI | CI config |
| 12 | Endpoint client từ xa kiểm tra scope device token | Manual review |
| 13 | Plugin process riêng, engine sống sót khi runner kill -9 | N/A (M2) |
| 14 | Hook transform/guard có budget, timeout test | N/A (M2) |
| 15 | Plugin không đọc/ghi file ngoài capability RPC | N/A (M2) |

---

## Notes

1. **Already Implemented**: Most M1 features are already coded. This plan is primarily a **verification and completion** plan.

2. **Missing HTTP Endpoint**: `resume_turn` exists in `Engine` but no HTTP route. Consider adding `POST /api/session/{id}/turn/{turnID}/resume` if needed.

3. **Scope Persistence**: `ApprovalBroker` already persists `AllowAlways` scope to store. Verify `get_allow_always_scope` works across restarts.

4. **Background Prune**: `SqliteStore::spawn_prune_task` runs every `prune_interval_secs`. Verify `shutdown()` called on server stop.

5. **EventHub Gap Detection**: SSE handler detects gaps and emits `EventResyncRequired`. Verify with concurrent event emission.

6. **OpenAPI Regeneration**: Run generator after any DTO changes:
   ```bash
   cargo run -p ananke-protocol -- gen openapi
   git diff openapi.json  # Should show only expected changes
   ```