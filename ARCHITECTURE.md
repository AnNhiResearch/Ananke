# Ananke — Tài liệu kiến trúc

| | |
|---|---|
| Trạng thái | Proposed (chờ review) |
| Phiên bản tài liệu | 0.1 |
| Ngày | 2026-11-15 |
| Phạm vi | Kiến trúc engine, giao thức client–engine, các surface giai đoạn 1 |
| Nguồn tham chiếu | OpenCode 2.x (headless engine + HTTP/SSE + SDK sinh tự động), Codex App Server (JSON-RPC trên stdio) |
| Đã chốt | TUI chính thức viết bằng TypeScript (OpenTUI) và gọi API; vẫn expose `/api/tui/*` để client ngoài điều khiển TUI |
| Đã chốt | Core chạy headless trên thiết bị nhỏ (Pi, Termux); TUI là artifact **cài rời**, không kèm mặc định; điện thoại/máy khác là client ngang hàng |
| Đã chốt | Lua (LuaJIT) là plugin runner **tích hợp sẵn** của sản phẩm; protocol plugin **mở cho mọi ngôn ngữ** (AD-20, AD-21) |

---

## 1. Bối cảnh

Ananke là một coding agent do chúng ta xây dựng. Yêu cầu đặt ra ngay từ đầu:

- Một **engine duy nhất** chứa agent loop, tool, sandbox, provider, persistence.
- Nhiều **surface**: TUI, desktop app, web/API + SDK, app điện thoại ở giai đoạn 1; IDE extension, CI/script, dịch vụ nội bộ ở giai đoạn sau.
- **Không được lặp lại phần lõi** ở bất kỳ surface nào. Thêm surface mới phải là "viết client", không phải "viết lại agent".

Bài học từ hai hệ thống đã khảo sát:

| | Codex | OpenCode |
|---|---|---|
| Đường biên | JSON-RPC 2.0 trên stdio | REST + SSE, OpenAPI 3.1 |
| Process model | mỗi client spawn một `app-server` con | một daemon dùng chung, client autodiscover |
| Client | sinh type TS/JSON Schema từ Rust | sinh SDK từ OpenAPI |
| Approval | server gửi request ngược, pause turn | event SSE + POST trả lời |
| Điểm mạnh | ngữ nghĩa hai chiều giàu, không mở port, sandbox chặt | client đa ngôn ngữ, debug bằng `curl`, nhiều client song song |

Ananke chọn **mô hình của OpenCode làm trục chính** (engine headless + protocol-first + daemon dùng chung), đồng thời giữ **hợp đồng kiểu tĩnh sinh từ Rust** theo cách của Codex. Lý do: engine viết bằng Rust, nhưng client phải viết được bằng TypeScript/Python/Go mà không cần bindings FFI.

---

## 2. Nguyên tắc kiến trúc

1. **Core không biết UI.** `ananke-core` chỉ nhận lệnh có kiểu và phát event. Không có khái niệm terminal, cửa sổ, hay editor trong core.
2. **Protocol là nguồn sự thật duy nhất.** Mọi thứ client thấy đều được định nghĩa trong `ananke-protocol`. Client không được import core.
3. **Một engine sống lâu, nhiều client.** Engine là daemon; TUI, desktop, web, điện thoại đều là client thứ cấp.
4. **State thuộc về server.** Client chỉ render và gửi intent. Đóng tab, mất mạng, sleep máy không làm mất turn.
5. **Event có thể replay.** Mọi thay đổi đi qua event log có thứ tự, client reconnect thì phát lại từ `seq` cuối.
6. **Local-first, remote-optional.** Cùng một protocol chạy trên UDS (local), TCP (web/remote), và in-process (embedded/test).
7. **Phân tầng được enforce bằng CI**, không chỉ bằng quy ước.
8. **Mở rộng bằng process, không bằng nhúng.** Plugin chạy ngoài engine và chỉ có đúng quyền engine cấp; engine sống sót khi plugin chết (AD-20 … AD-23).

---

## 3. Quyết định kiến trúc

Mỗi quyết định ghi theo dạng: bối cảnh → quyết định → hệ quả → phương án đã loại.

### AD-01 — Core là Rust library độc lập với UI

**Quyết định.** `ananke-core` là crate thuần: agent loop, context management, compaction, tool dispatch, provider abstraction. Nó giao tiếp ra ngoài bằng hai kênh:

```
Command  ──►  Core session  ──►  Event
```

Không có `println!` cho UI, không có dependency vào TUI/http.

**Hệ quả.** Server, embedded mode và test đều dùng được core. Muốn thêm surface chỉ cần chuyển Command/Event thành transport tương ứng.

**Đã loại.** Nhúng core trực tiếp vào TUI binary (kiểu Codex trước 0.117): nhanh lúc đầu nhưng biến TUI thành surface đặc biệt, chặn remote execution.

---

### AD-02 — Một daemon dùng chung, không phải mỗi client một process

**Quyết định.** Ananke chạy như một **background service** theo user:

- `ananke service start|stop|status`
- `ananke run` (mặc định): đảm bảo service đang chạy, rồi mở TUI như một client.
- Singleton được bảo vệ bằng lockfile + `service.json` trong `~/.ananke/`.
- Service sống độc lập: đóng TUI, tắt desktop app, turn vẫn tiếp tục.

**Hệ quả.**

- Nhiều client cùng lúc: TUI + desktop + script + IDE trên cùng session.
- Tác vụ nền (scheduled task, subagent chạy dài) không chết theo client.
- Cần quản lý vòng đời service cẩn thận: version skew, upgrade, crash recovery, orphan process.

**Đã loại.** Per-client child process (kiểu Codex): đơn giản hơn về lifecycle nhưng không chia sẻ session, không chạy nền được, mỗi client phải tự pin binary.

---

### AD-03 — HTTP REST + SSE, OpenAPI 3.1 là hợp đồng

**Quyết định.** Giao thức chính là HTTP/JSON với SSE cho streaming.

- Base path: `/api`
- Spec: `GET /api/openapi.json`, xem tại `GET /api/doc`
- Version: `GET /api/meta` trả `protocolVersion`, `capabilities`; mọi request mang header `X-Ananke-Protocol: 1`
- Lỗi: RFC 9457 Problem Details (`application/problem+json`)

**Hệ quả.**

- Client đa ngôn ngữ không cần FFI; SDK sinh từ spec.
- Debug bằng `curl` và Swagger UI.
- SSE chỉ một chiều; approval phải mô hình hoá thành event + POST (xem AD-07).

**Đã loại.**

- JSON-RPC thuần (kiểu Codex): ngữ nghĩa đẹp nhưng đòi hỏi client tự viết transport hai chiều, khó dùng từ browser, không có hệ sinh thái codegen.
- gRPC: cần protobuf toolchain ở mọi client, streaming hai chiều tốt nhưng khó debug và khó dùng trong browser.

---

### AD-04 — Transport tách khỏi protocol

Cùng một protocol chạy trên bốn transport:

| Transport | Dùng cho | Đặc điểm |
|---|---|---|
| **UDS / named pipe** (mặc định production) | TUI, desktop app, CLI, embedded tooling | Không mở port, không cần token, quyền `0600`; Windows: `\\.\pipe\ananke-<user>` |
| **TCP loopback** (`127.0.0.1:9876`) | Web client, TUI dev, browser SDK, công cụ ngoài | Thống nhất port `127.0.0.1:9876` cho local HTTP/SSE service |
| **TCP public** (`--listen 0.0.0.0:9876 --tls-*`) | Remote dev, CI runner, dịch vụ nội bộ, **điện thoại qua pairing** | Bắt buộc TLS + auth; pairing cấp device token riêng (AD-18) |
| **In-process** | Test, `ananke-embedded`, client third-party chạy cùng process | Router chạy trong cùng process, không serialize qua socket |

Discovery: `~/.ananke/service.json`

```json
{
  "pid": 48213,
  "version": "0.1.0",
  "protocolVersion": 1,
  "transport": "unix",
  "path": "/Users/me/.ananke/run/ananke.sock",
  "startedAt": "2026-11-15T09:12:33Z"
}
```

Client đọc file này, kết nối, gọi `/api/meta`, kiểm tra `protocolVersion`. Nếu file cũ, fallback sang `ananke service start`. Có thể bật mDNS (`--mdns`) cho remote dev trong LAN.

**Hệ quả.** Mặc định an toàn hơn OpenCode (vốn dùng TCP `:4096`): không có port mở, không có CSRF bề mặt, không đụng port khi nhiều user. Web client do chính service phục vụ tại `/` nên same-origin, không cần CORS.

---

### AD-05 — Client không thể import core

**Quyết định.** Luật phụ thuộc một chiều:

```
schema ──► protocol ──► server ──► (binary)
                │
                └────► client ──► client apps (tui, desktop, mobile, web, sdk)
core ──► server
```

- `ananke-core` không phụ thuộc `ananke-protocol`, `ananke-server`.
- `ananke-client` chỉ phụ thuộc `ananke-protocol`.
- Package TS `@ananke/sdk` **không** được import bất kỳ module engine nào.

**Enforce.** Ngoài review, có test CI:

1. Kiểm tra `Cargo.toml` của `core` không chứa `ananke-protocol`/`ananke-server`.
2. Kiểm tra `packages/sdk-ts/package.json` không có dependency vào engine.
3. `cargo deny` chặn dependency vòng.
4. Test kiểu "import boundary": build SDK ở môi trường browser-only, không có Node API.

**Hệ quả.** Không thể "tiện tay" gọi hàm core từ client. Mọi thứ phải đi qua protocol — đúng mục tiêu.

---

### AD-06 — Primitive: Session / Turn / Part

| Primitive | Định nghĩa | Vòng đời |
|---|---|---|
| **Session** | Container bền vững của một cuộc làm việc, gắn một workspace + worktree | `session.created` → `session.updated` → `session.archived`; fork được, revert được |
| **Turn** | Một đơn vị việc agent làm cho một input | `turn.started` → `turn.completed` / `turn.failed` / `turn.interrupted`; là đơn vị interrupt và rollback |
| **Part** | Đơn vị nội dung nguyên tử trong một message | `part.started` → `part.delta` (0..N) → `part.completed` |

Loại Part: `text`, `reasoning`, `tool_call`, `tool_result`, `command`, `file_change`, `permission_request`, `compaction`, `attachment`.

**Vì sao không dùng Message/Part thuần như OpenCode.** OpenCode không có Turn như primitive, nên việc "huỷ một lượt", "steer giữa lượt", "rollback tới trước lượt này" phải suy ra từ message id — dễ lệch khi có subagent và tool chạy song song. Turn là mốc rõ ràng cho interrupt/rollback/usage/cost.

**Vì sao không dùng Item như Codex.** Item gộp cả input lẫn output; khi cần hiển thị một message có nhiều đoạn stream (text + reasoning + tool) thì phải tự nhóm. Part tách bạch nội dung khỏi message, khớp với cách UI render.

Ánh xạ khi đọc tài liệu tham chiếu: `Thread ≈ Session`, `Message ≈ Turn`, `Item ≈ Part`.

---

### AD-07 — Approval là event + POST, không cần kênh ngược

**Quyết định.**

1. Engine phát `permission.requested` (kèm `permissionID`, loại hành động, command/path, lý do, scope đề xuất), turn **tạm dừng**.
2. Client hiển thị, người dùng chọn, rồi gọi:

```
POST /api/session/{sessionID}/permission/{permissionID}
{ "decision": "allow" | "deny" | "allow_always", "scope": "once" | "session" | "project" }
```

3. Engine phát `permission.resolved`, turn tiếp tục hoặc kết thúc.

**Hệ quả.** Không cần bidirectional transport; browser làm được. Nhưng client phải correlate event với request, và engine phải giữ state permission bền vững để reconnect vẫn trả lời được.

**Đã loại.** Server-initiated request kiểu Codex: chính xác hơn về mặt "turn đang chờ ai", nhưng đòi hỏi kênh hai chiều và làm phức tạp mọi client.

---

### AD-08 — Event log có thứ tự, replay được

- Mỗi session có `seq` tăng đơn điệu.
- SSE per-session: `GET /api/session/{id}/event?since={seq}`; mỗi frame có `id: {seq}`.
- Client reconnect dùng `Last-Event-ID`; server phát lại phần thiếu từ event log.
- **`part.delta` không được persist.** Delta chỉ broadcast trực tiếp cho subscriber; event log chỉ ghi `part.started` / `part.completed` cùng snapshot nội dung. Reconnect giữa một turn đang stream nhận `event.resync_required` rồi lấy `/state`. Không có luật này thì một turn stream dài sẽ tạo hàng nghìn row và đốt flash trên thiết bị nhỏ.
- Buffer delta trong RAM bị chặn trên: vượt `max_part_bytes` thì phần dư ghi ra file, part chỉ giữ `ref`.
- Stream toàn cục: `GET /api/event` cho session list, service health, usage.
- Nếu event log đã bị compact quá mốc yêu cầu, server trả `event.resync_required`; client gọi `GET /api/session/{id}/state` để lấy snapshot rồi stream tiếp.

**Hệ quả.** UI không cần tự dựng lại state từ các delta rời rạc. Đây là điều kiện để desktop app mở nhiều cửa sổ, web tab đóng/mở, TUI reconnect.

---

### AD-09 — Persistence: SQLite + event log

Bảng chính:

```
session(id, parent_id, workspace, worktree, title, model, created_at, archived_at)
turn(id, session_id, seq, status, usage_json, started_at, ended_at)
part(id, session_id, turn_id, message_id, kind, ordinal, content_json, status)
event(session_id, seq, type, payload_json, created_at)
permission(id, session_id, turn_id, kind, payload_json, decision, scope, resolved_at)
todo(id, session_id, content, status, priority, ordinal)
checkpoint(id, session_id, turn_id, kind, ref)
```

- WAL mode, migration có version, chạy lúc service start.
- Event log là **write-ahead cho UI**: ghi event trong cùng transaction với state, publish sau commit (transaction side effect) để không có event "ma" khi rollback.
- Session nạp lazy vào RAM; session không có subscriber và không hoạt động bị unload (mặc định 30 phút), phát `session.unloaded` trước khi unload.
- Mọi dữ liệu nằm trong **data dir** (`~/.ananke/` mặc định; `$PREFIX/var/lib/ananke` trên Termux) với quyền `0700`. Đường dẫn đặt qua `--data-dir`, không hard-code (AD-19).

---

### AD-10 — Codegen: schema Rust → OpenAPI → SDK

Pipeline:

```
ananke-protocol (Rust structs + schemars/utoipa)
        │  cargo run -p ananke-protocol -- gen openapi
        ▼
openapi.json (commit vào repo)
        │  openapi-generator / hey-api
        ▼
packages/sdk-ts   (@ananke/sdk)
packages/sdk-py   (ananke-sdk)
packages/sdk-go
```

Quy tắc:

- `openapi.json` **được commit**. CI chạy lại generator và fail nếu diff — chống trôi schema.
- SDK là generated, không sửa tay; phần tiện ích (retry, SSE reconnect, discovery) nằm ở lớp wrapper mỏng bên trên, không đụng file generated.
- Rust client (`ananke-client`) dùng chung `ananke-protocol` type, không qua OpenAPI.

---

### AD-11 — Embedded mode

`ananke-embedded` chạy nguyên router HTTP với transport in-memory:

```rust
let engine = ananke_embedded::start(Config::default()).await?;
let client = ananke_client::Client::in_process(engine.handle());
```

Dùng cho: unit/integration test, client Rust third-party chạy cùng process, nhúng Ananke vào sản phẩm khác, và benchmark. Ngữ nghĩa protocol giữ nguyên — chỉ đổi transport.

---

### AD-12 — TUI chính thức viết bằng TypeScript, chạy như một client

**Quyết định.** TUI chính thức của Ananke là một **client TypeScript** dùng OpenTUI, nằm ở `packages/tui/`, nói chuyện với engine qua `@ananke/sdk`. Nó **không** là crate Rust, không link `ananke-core`, không có đường tắt nào vào engine.

- Stack: TypeScript + [OpenTUI](https://github.com/sst/opentui) (renderer terminal), cùng bộ SDK mà desktop và web dùng.
- Khởi động: `ananke run` → đảm bảo service → TUI `discover()` (`service.json`) → mở SSE → render snapshot + stream.
- TUI chỉ giữ state hiển thị (viewport, focus, filter); state thật thuộc server.
- Không có chế độ in-process cho TUI: engine là process Rust riêng, TUI luôn đi qua UDS. In-process (`ananke-embedded`) chỉ dành cho client Rust chạy cùng process (AD-11).

**Hệ quả.**

- Một ngôn ngữ cho TUI, desktop, mobile, web → chung `@ananke/sdk`, chung bug, chung fix, chung conformance test.
- TUI trở thành "khách hàng số 1" của protocol: nếu API đủ cho TUI thì đủ cho client ngoài.
- Client viết bằng Rust **vẫn hợp lệ**: chỉ cần gọi HTTP API (hoặc dùng crate `ananke-client`), không cần xin phép. TUI TS không phải đặc quyền, chỉ là lựa chọn mặc định của chúng ta.
- Đánh đổi: TUI cần runtime JS (Bun/Node) trên máy chạy nó; chấp nhận được vì TUI là artifact **cài rời** — ai không dùng TUI thì không phải kéo theo runtime (AD-14).

**Đã loại.** `ananke-tui` viết bằng Rust/`ratatui`: hiệu năng tốt hơn và không cần runtime JS, nhưng tạo ra client "hạng nhất" khác ngôn ngữ với desktop/web, dễ lệch hành vi và làm protocol bị thiết kế quanh một client Rust.

---

### AD-13 — TUI expose `/api/tui/*` để client ngoài điều khiển

**Quyết định.** TUI đang chạy mở một **control endpoint** nhỏ, để client khác (IDE extension, script, trợ lý khác) điều khiển nó thay vì mở TUI thứ hai.

- TUI đăng ký endpoint với service khi khởi động; service chuyển tiếp tại `/api/tui/*`. Nhờ vậy client ngoài chỉ cần biết một địa chỉ duy nhất (service), không cần tự tìm TUI.
- Nếu không có TUI nào đang chạy, service trả `409 tui_not_attached`.
- Cùng auth như phần còn lại của `/api` (UDS `0600`, token nếu TCP). Không mở thêm bề mặt mạng mới.

**Hệ quả.** IDE có thể "gửi prompt vào TUI đang mở", mở help, đổi theme, chạy slash command — đúng mô hình OpenCode. Đây là cam kết **giai đoạn 1**, không phải "để sau".

**Đã loại.** Chỉ cho phép client ngoài tạo session mới: đơn giản hơn nhưng không đáp ứng được ca "người dùng đang gõ dở trong TUI, IDE muốn chèn context vào đúng chỗ đó".

---

### AD-14 — Headless-first: TUI là artifact cài rời

**Quyết định.** Sản phẩm lõi là **engine + service + CLI**, không có UI, không có runtime JS.

| Artifact | Chứa gì | Phụ thuộc |
|---|---|---|
| `ananke` (binary Rust) | engine, service, SQLite, CLI, HTTP router | chỉ libc / bionic |
| `@ananke/tui` (cài rời) | TUI TypeScript + OpenTUI | Bun/Node |
| `@ananke/sdk` | SDK sinh từ OpenAPI | runtime JS bất kỳ |
| app desktop / mobile | client riêng | SDK hoặc HTTP thuần |

- `ananke serve` / `ananke run --headless`: chạy engine, không cần TUI, không cần JS.
- `ananke run`: nếu chưa cài TUI thì **không tự tải**, chỉ in hướng dẫn (`ananke tui install`, hoặc dùng app). Cài TUI là hành động tường minh của người dùng.
- Version TUI bị pin theo `protocolVersion`, không phải "mới nhất".

**Hệ quả.** Thiết bị 1 GB RAM, container, CI, máy chủ không UI đều chạy được lõi với chi phí ~25–40 MB. Không ai phải trả giá RAM/đĩa cho thứ họ không dùng.

**Đã loại.** Đóng gói TUI chung với binary (như Codex gộp TUI vào `codex`): tiện cho người dùng phổ thông nhưng biến mọi deployment headless thành phải kéo theo runtime JS.

---

### AD-15 — Ngân sách tài nguyên và admission control

**Quyết định.** Mọi giới hạn là cấu hình, có giá trị mặc định theo RAM máy:

```toml
[limits]
max_resident_sessions = 8       # constrained: 4
max_parallel_turns    = 2       # constrained: 1
max_subagents         = 2       # constrained: 1
max_context_tokens    = 48_000  # constrained: 24_000
max_part_bytes        = 256_000 # vượt → spill ra file, part giữ ref
max_event_bytes       = 64_000
```

- Vượt giới hạn → xếp hàng hoặc `429 limit_exceeded`, kèm event `resource.throttled`. **Không bao giờ OOM im lặng.**
- Profile chọn theo `MemTotal`: `< 1.5 GB` → `constrained`, `< 4 GB` → `small`, còn lại → `default`. Ghi đè được bằng config.
- `constrained` tắt: parallel subagent, LSP nền, watch file, và (mặc định) TUI full.

**Hệ quả.** "Chạy được 3 sub-agent trên máy 1 GB" trở thành câu trả lời có kiểm chứng (`resource.throttled` thay vì bị LMK giết), và test tải có ngưỡng rõ ràng.

---

### AD-16 — Hợp đồng sub-agent

**Quyết định.** Sub-agent là một **session con**, không phải bản copy của session cha.

- Spawn qua tool `task`, tạo `session(parent_id, depth = depth_cha + 1)`.
- **Brief-in:** con chỉ nhận `{prompt, refs}` — không kế thừa transcript, không kế thừa tool state. Cha truyền ngữ cảnh bằng lời, không bằng lịch sử.
- **Summary-out:** con bắt buộc kết thúc bằng summary có cấu trúc (`files_changed`, `findings`, `next_steps`, `status`). Cha nhận **đúng một `tool_result` part** chứa summary đó.
- **Giới hạn:** `max_turns`, `max_context_tokens`, `max_wall_time` riêng cho con; vượt thì dừng và trả summary một phần kèm `status: "partial"`.
- **Song song có cap:** vượt `max_subagents` thì xếp hàng, event `subagent.queued`.
- **Unload ngay khi xong:** transcript ở lại SQLite để audit, không giữ trong RAM.
- `constrained`: `depth = 1` (con không spawn con), `max_subagents = 1`.

**Lý do.** Mô hình "copy cả history cho con" khiến RAM tỉ lệ với *số con × kích thước context cha* — chết trên thiết bị nhỏ. Brief/summary khiến RAM tỉ lệ với *kích thước brief + summary*, gần như hằng số.

**Đã loại.** Chia sẻ session cha cho nhiều sub-agent ghi song song: tiết kiệm context nhưng phá thứ tự turn và biến mọi lỗi thành lỗi tranh chấp khó tái hiện.

---

### AD-17 — Suspend tại await point cho tool dài

**Quyết định.** Turn được **persist trước khi dispatch tool**. Tool khai báo `long_running` thì engine thả task và resume khi kết quả về; tool ngắn vẫn chạy inline.

```
slice: nạp state → 1 model completion → persist part → dispatch tool → kết thúc slice
                                      │
                    tool ngắn ────────┴──► chờ inline, giữ task
                    tool dài ─────────────► thả task; kết quả vào input queue → slice mới
```

- "Dài" xác định theo loại tool (build, test, CI, approval, `sleep`) và ngưỡng thời gian, không theo cảm tính.
- Vì turn đã persist, Android kill service giữa lúc chờ **không mất turn**: khởi động lại, thấy turn `awaiting_tool`, hoàn tất khi có kết quả.
- Không làm slice đầy đủ + queue bền vững (mức 3 của mô hình Infinity) ở giai đoạn 1 — chỉ mượn đúng điểm treo.

**Hệ quả.** Sub-agent chờ build 20 phút chỉ chiếm phần history của nó, không chiếm task và stack. Đây là điều kiện để AD-16 khả thi trên 1 GB.

---

### AD-18 — Client từ xa (điện thoại/máy khác) là công dân hạng nhất

**Quyết định.** Điện thoại không phải "remote control của TUI", mà là **một client đầy đủ** trên cùng protocol.

- Service bật `--listen 0.0.0.0:4096 --tls-cert/--tls-key`; mDNS `_ananke._tcp` để client tìm trong LAN.
- **Pairing:** `ananke pair` in QR chứa `{endpoint, cert_fingerprint, one-time code}`. Client gửi `POST /api/pair` → nhận **device token** riêng (scope + hạn). Token thu hồi độc lập từng thiết bị.
- Không dùng chung token với TUI local; không có quyền "chỉ TUI mới làm được X".
- Approval, interrupt, steer, permission đều đi qua event + POST (AD-07) nên hoạt động y hệt trên điện thoại.
- Nếu không có TUI nào chạy, điện thoại vẫn dùng được đầy đủ — `/api/tui/*` chỉ là phần thêm, không phải điều kiện.

**Hệ quả.** Máy nhỏ chạy lõi trong góc nhà, điện thoại là UI chính. Đây là lý do AD-14 tách TUI ra: trên thiết bị headless, UI duy nhất có thể là điện thoại.

**Đã loại.** Chỉ cho phép client từ xa qua SSH port-forward: an toàn nhưng đòi hỏi người dùng biết SSH, không dùng được trên điện thoại.

---

### AD-19 — Android/Termux và thiết bị hạn chế là target hạng nhất

**Quyết định.** Mỗi bản phát hành có build `aarch64-linux-android` (NDK) và `aarch64-unknown-linux-gnu` (Pi). Người dùng không phải `cargo build` trên thiết bị.

- `--data-dir` thay cho hard-code `~/.ananke`; Termux mặc định `$PREFIX/var/lib/ananke`.
- **Sandbox theo năng lực kernel**, phát hiện lúc start và ghi vào `/api/meta`:

| Nền tảng | Thực tế |
|---|---|
| Linux ≥ 5.13 (Pi OS) | Landlock + seccomp — enforce được `workspace-write` |
| Android/Termux | không Landlock, không userns → `sandbox = "none"`; permission rules là hàng rào duy nhất, `/api/meta` báo rõ |
| macOS | Seatbelt |
| Windows | restricted token |

- Không có systemd: `ananke serve --foreground` + `termux-services`/`runit`. Android có thể kill bất kỳ lúc nào → mọi state phải phục hồi từ SQLite (AD-09) và turn đã persist (AD-17).
- SQLite: `cache_size = -2000`, `mmap_size = 0`, `synchronous = NORMAL`, checkpoint `TRUNCATE` định kỳ; retention event log bật từ M1 (không đợi M3).
- Tiêu chí chấp nhận trên máy 1 GB: RSS service idle + 2 session < 80 MB; một turn có approval hoàn tất; kill -9 rồi khởi động lại vẫn đúng lịch sử.

**Hệ quả.** "Chạy trên Pi/TV box" là tiêu chí kiểm thử, không phải lời hứa. Và nó buộc thiết kế phải trung thực về sandbox thay vì giả định có.

---

### AD-20 — Plugin là process riêng, protocol mở cho mọi ngôn ngữ

**Quyết định.** Plugin là một **process riêng** nói JSON-RPC 2.0 qua stdin/stdout, mỗi message một dòng JSON (NDJSON). Engine không nhúng runtime của bất kỳ ngôn ngữ nào để chạy plugin.

- Hợp đồng là protocol, không phải thư viện: ai tạo được một process đọc/ghi stdout là viết được plugin.
- Manifest `ananke-plugin.json`: `id`, `version`, `protocolVersion`, `runtime` (`lua`/`node`/`python`/`binary`/…), `hooks`, `tools`, `permissions`, `limits`. Mặc định chỉ các runner do Ananke phát hành được bật; `runtime: "binary"` phải bật tường minh (§14.8).
- Engine **không tin** plugin. Mọi thứ plugin làm được đều do engine cấp qua RPC (AD-22), không do plugin tự gọi syscall.
- SDK tham chiếu có cho Rust, TypeScript, Python, Go — chúng chỉ là wrapper mỏng của protocol, không phải đường bắt buộc.
- Plugin bên thứ ba phát hành dưới dạng thư mục (hoặc archive) có manifest; cài bằng `ananke plugin install`, không cần build lại engine.

**Lý do.** Không có runtime nào nhúng được mọi ngôn ngữ, và mọi cách nhúng đều biến lỗi plugin thành lỗi engine. Process là ranh giới duy nhất mà Rust, Lua, Node, Python, Go đều có sẵn, đồng thời cho crash isolation và kill được.

**Đã loại.**
- Plugin in-process dưới dạng `cdylib`/`.so` — ABI Rust không ổn định, một `panic`/segfault kéo cả engine, không cách nào thu hồi quyền.
- WASM ở giai đoạn 1 — cô lập tốt nhất nhưng phải mang host runtime và bộ SDK per-language chưa trưởng thành; xem §15.4.

---

### AD-21 — Lua (LuaJIT) là runner tích hợp sẵn của sản phẩm

**Quyết định.** Bản phát hành Ananke mang theo `ananke-plugin-lua` (~600 KB–1 MB), để viết plugin bằng Lua không cần cài runtime nào thêm. Runner này **cũng là một process theo AD-20**, không phải engine nhúng Lua.

- `ananke plugin new --lua my-plugin` sinh manifest + skeleton; `ananke plugin install ./my-plugin` đăng ký (thêm `--enable` để bật ngay).
- Runner chạy LuaJIT với `LUAJIT_DISABLE_FFI`; stdlib bị cắt: không `io`, không `debug`, `require` chỉ nạp được file trong thư mục plugin, `os` chỉ còn whitelist thuần (`date`, `time`, `clock`).
- Viết trên `mlua` để có thể đổi LuaJIT → PUC Lua 5.4 mà **không đổi API phía plugin**, nếu LuaJIT ngừng được maintain.
- Lua **không nằm trên hot path**: agent loop, streaming, built-in tool vẫn là Rust. Lua chỉ chạy ở các hook rời rạc và trong tool do plugin đăng ký.
- Plugin Lua và plugin ngôn ngữ khác dùng chung một protocol, cùng một bộ hook, cùng một mô hình quyền.

**Lý do.** Một runner tích hợp sẵn biến "viết plugin" thành việc sửa một file `.lua` rồi `ananke plugin install`, không cần toolchain. LuaJIT có cold start 2–10 ms và RSS 2–4 MB nên chấp nhận được trên máy 1 GB — nhưng chỉ vì nó là process rời, không phải vì nó nhẹ hơn Rust.

**Đã loại.** Nhúng Lua vào engine (`mlua` in-process) để tiết kiệm vài ms mỗi hook: đổi lấy việc một `while true do end` trong plugin treo toàn bộ daemon.

---

### AD-22 — Capability đi qua engine, không qua syscall

**Quyết định.** Plugin không tự mở file, không tự spawn process, không tự mở socket. Mọi tác động ra ngoài đi qua RPC do engine thực thi:

| RPC | Engine áp |
|---|---|
| `fs.read` / `fs.write` / `fs.list` | sandbox policy (AD-19) + permission rules (AD-07) |
| `exec` | cùng đường với tool `bash`, cùng approval |
| `http` | allowlist domain + ghi audit |
| `secrets.get` | chỉ secret trong scope `plugin.<id>`; không trả API key provider |
| `state.get` / `state.set` | KV do engine cấp, namespace theo `plugin.id` |

- Manifest khai báo `permissions`; quyền không khai báo thì engine trả lỗi có mã, không im lặng cấp.
- Mỗi lần gọi capability được ghi vào session log với `pluginID`; **model thấy gì thì log có đó** (AD-08).
- Tool do plugin đăng ký chạy qua cùng đường `tools/pre-execute` như built-in tool: approval, sandbox, event log không có ngoại lệ.
- Trên Android/Termux (`sandbox = "none"`), permission rules là hàng rào duy nhất — vì vậy runner chính thức không expose syscall, còn plugin `binary` tự viết phải được người dùng bật tường minh.

**Hệ quả.** Plugin không thể "nói dối" về quyền: nó chỉ có những gì engine gọi hộ. Đổi lại, plugin không dùng được thư viện native tuỳ ý — đó là mục đích, không phải hạn chế.

---

### AD-23 — Hook budget, vòng đời và state của plugin

**Quyết định.** Hook chia hai lớp, vì hậu quả khi plugin chậm hoặc chết là khác nhau:

| Lớp | Hook | Khi plugin chậm/chết |
|---|---|---|
| `observe` | `session.*`, `turn.*`, `tool.after`, `plugin.*` | fail-open: bỏ qua, ghi `plugin.error`, turn tiếp tục |
| `transform` / `guard` | `tool.before`, `message.transform`, `turn.pre-step` | fail-closed: hết budget thì bỏ plugin khỏi chuỗi và ghi `plugin.error`; `guard` từ chối thì turn dừng có lý do |

- Budget mặc định **100 ms** cho tổng các `transform`/`guard` tại một điểm hook. Quá budget → plugin bị coi là timeout; **3 lần timeout liên tiếp → `plugin.disabled`** + event, không tự bật lại.
- Không hook nào chạy trên từng token stream. `part.delta` chỉ đi qua engine.
- Plugin chết giữa hook (`EOF`, exit code bất kỳ): `observe` bỏ qua; `transform` coi như không có plugin đó; `guard` coi như từ chối và turn dừng với `plugin.error`.
- State: chỉ qua `state.get`/`state.set`, lưu trong SQLite theo `plugin.id`. Plugin không có file state riêng, để backup và `revert` hoạt động đúng.
- Trên profile `constrained`: plugin idle quá 60 s bị stop, khởi động lại lazy (Lua: ~2–10 ms). Không giữ nhiều runner sống cùng lúc nếu RAM dưới ngưỡng AD-15.
- Event mới: `plugin.started`, `plugin.stopped`, `plugin.disabled`, `plugin.error`.

**Lý do.** Một plugin quan sát không được phép làm hỏng một turn. Một plugin có quyền chặn tool thì phải chịu ngân sách thời gian, nếu không nó trở thành điểm treo toàn hệ thống.

**Đã loại.** Timeout vô hạn kiểu OpenCode (hook chạy tuần tự, không giới hạn) — trên TV box 1 GB, một hook treo là một daemon chết.

---

### AD-24 — File hướng dẫn dự án (`AGENTS.md`) là input một chiều, có ngân sách

**Quyết định.** Engine ghép **một** message `system` ảo cho mỗi lượt gọi model, từ các nguồn theo thứ tự cố định:

| Thứ tự | Nguồn | Bị ghi đè bởi |
|---|---|---|
| 1 | prompt nền của engine (hằng trong binary, có `BASE_PROMPT_VERSION`) | 2, 3, 4 |
| 2 | `~/.config/ananke/AGENTS.md` (người dùng, không bắt buộc) | 3, 4 |
| 3 | `AGENTS.md` ở workspace root | 4 |
| 4 | `AGENTS.md` trong thư mục con của file mà tool vừa chạm | thắng |

- Tương thích: chỉ đọc `CLAUDE.md` khi **không** có `AGENTS.md` trong cùng thư mục. Không đọc `.cursorrules`, không quét `*.md` ở root — đọc ngầm file lạ chính là cách một repo bơm chỉ thị vào model.
- `system` **không** được append vào `Session::messages()`. Nó được dựng lại trong `Engine::model_step` mỗi lượt, vì transcript là thứ được persist, replay (`since`), và resume — nhét system vào đó thì mỗi lượt gửi lại thêm một bản nữa.
- **Ngân sách:** `max_instructions_bytes` = 32_000 (`constrained`: 12_000), `max_instruction_files` = 8. Vượt → cắt từ ưu tiên **thấp nhất** trước (global trước, subdir cuối) và ghi `instructions.truncated: true` vào system để model biết mình đang thiếu, thay vì đoán.
- **Nạp lại:** cache theo `(path, len, mtime)`; đổi file → nạp lại ở ranh giới turn, phát `instructions.loaded {files, bytes, truncated, changed}`. Trên `constrained` **không** watch file (AD-15 tắt watch) — chỉ so mtime khi turn bắt đầu.
- **Hợp đồng một chiều:** file hướng dẫn là **dữ liệu không tin cậy**, ngang hàng mọi text khác trong workspace. Nó **không** cấp quyền, không tắt approval, không thêm tool. Chặn quyền là việc của permission rules (AD-07) và plugin `guard` (AD-23) — thứ người dùng cấu hình, không phải thứ repo tự khai.
- Client vẫn thêm được instruction riêng qua `session.create.instructions` (mảng string, chung ngân sách byte) — đường này không qua file nên không bị cắt theo file.

**Lý do.** Hiện `Engine::model_step` chỉ gửi `session.messages()` + `tools.schemas()` — **không có slot system**, nên quy ước `AGENTS.md` (Codex, OpenCode, Pi đều dùng) không có chỗ để sống. Đặt nó ở tầng request thay vì tầng session giữ nguyên AD-06/AD-08: system không phải Part, không có `seq`, không replay.

**Hệ quả.** Repo tự mô tả quy tắc của nó (như chính file `AGENTS.md` của Ananke) mà client không phải gửi kèm; đổi file giữa phiên có hiệu lực từ turn sau, không cần session mới.

**Đã loại.**
- Nhét instructions vào Part/message của session: replay và resume nhân bản context, và `session.messages().len()` thành con số vô nghĩa.
- Quét mọi markdown ở root để "không bỏ sót": biến mỗi repo thành bề mặt injection và ăn hết ngân sách token trên máy 1 GB.
- Giữ instructions cố định suốt session: sửa `AGENTS.md` mà model vẫn theo bản cũ là lỗi khó hiểu khi debug.

---

### AD-25 — MCP client: tool MCP không vào tool list, chỉ đi qua 3 tầng

**Quyết định.** Ananke là **MCP client**, không phải MCP server, và **không tích hợp sẵn server nào**: người dùng tự cài server (hoặc trỏ tới một URL có sẵn), `mcp.json` chỉ là nơi khai báo. MCP vì thế chỉ là **một API ngoài** — Ananke hỗ trợ điều phối, không phân phối. Mỗi server chịu cùng chính sách như plugin (AD-20 … AD-22): không có quyền gì hơn một plugin Lua.

**Tool MCP không bao giờ được nạp vào `ToolRegistry` của session chính.** Không `tools/list` nào đổ thẳng vào prompt, không `inputSchema` nào nằm trong tool list của agent chính. Agent chính thấy đúng ba tool, và mọi capability MCP đi qua ba tầng theo bậc chi phí:

| Tầng | Tool lộ ra agent chính | Làm gì | Khi nào dùng |
|---|---|---|---|
| 0 | `mcp_search` | Truy vấn **index** SQLite FTS trên `server/tool/description`; trả ≤ 8 tên + mô tả một dòng. Không spawn, không network | Mỗi khi agent cần biết có capability phù hợp |
| 1 | `mcp_call` | Một lần gọi duy nhất. Schema chỉ dùng để **validate tham số phía server**, không vào prompt; kết quả cắt theo byte budget rồi mới vào history | Việc một phát là xong, output nhỏ |
| 2 | `task` (subagent, AD-16) | Spawn subagent với allowlist `mcp__<server>__<tool>`; context riêng, trả báo cáo cuối, **context con bị drop** | Nhiều bước, output lớn/bẩn, tool `mutating` cần thử-sai |

- **Vì sao ba tầng, không phải "mọi thứ qua subagent".** Nạp thẳng tool MCP vào prompt (mô hình Cline/Roo) cháy context. Nhưng bắt mọi lần gọi đi qua subagent thì trả thêm một lượt model + một system message cho ca rẻ nhất — "đọc một file qua server filesystem". Tầng 2 dùng để **cách ly bẩn và cách ly quyền**, không phải để cách ly mọi lần gọi.
- Khai báo trong `mcp.json` (project) và `~/.config/ananke/mcp.json` (user), cả hai transport trong cùng một file:

```json
{
  "servers": {
    "fs": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "."],
      "env": { "NODE_OPTIONS": "--max-old-space-size=64" },
      "sandbox": "inherited",
      "mutating": true,
      "longRunning": false,
      "limits": { "startTimeoutMs": 10000, "toolTimeoutMs": 30000 }
    }
  }
}
```

- **Tên tool:** `mcp__<server>__<tool>`, chuẩn hoá về `[a-z0-9_-]`, ≤ 64 ký tự (khớp `valid_name` của manifest plugin). Dùng ở cả ba tầng, vì `ToolRegistry::register` coi trùng tên là `RegistryError::Duplicate`, MCP không có namespace, và model không được nhầm tool MCP với built-in.
- **Transport `command`:** process con nói JSON-RPC 2.0 qua stdio, NDJSON — **cùng codec với plugin host (AD-20)**, không thêm dependency.
- **Transport `url`:** streamable HTTP, **không có process con** — rẻ nhất cho máy 1 GB: không reap zombie, không idle-stop, chỉ cần timeout và token. Server remote do người dùng tự dựng:

```json
"sentry": {
  "url": "https://mcp.example.com/mcp",
  "headers": { "Authorization": "Bearer ${SENTRY_TOKEN}" },
  "mutating": false
}
```

- **`headers`/`env` chỉ nhận giá trị tường minh hoặc `${VAR}` đọc từ env của daemon**, không log giá trị. `env` của transport `command` là whitelist, không kế thừa môi trường daemon — kế thừa là cách rò `OPENAI_API_KEY` sang một package `npx` chưa ai kiểm chứng.
- **`mutating` mặc định `true`** (fail-safe, cùng lý do với `ManifestTool`): tool MCP đi qua permission rules và approval như built-in; chỉ khi người dùng tự viết `"mutating": false` mới bỏ qua.
- **Index, không phải schema cache nạp sẵn:** daemon khởi động **không** spawn server nào và **không** giữ `inputSchema` nào trong RAM. `tools/list` được lưu thành index tìm kiếm trong SQLite (`server`, `tool`, `description`, `schema_hash`, `tool_list_hash`); process chỉ sống khi có `tools/call`. Đây là khác biệt giữa "dùng được 3 MCP server" và "daemon không start nổi trên 1 GB".
- **Sticky pin:** sau `mcp_search`, schema của ≤ 3 tool trúng được giữ cho N lượt kế tiếp rồi bỏ, để bước sau không phải search lại. Không pin thì mỗi bước là một vòng round-trip thừa; pin vô hạn thì quay lại đúng vấn đề của tool list.
- **RAM:** `max_mcp_servers` = 2 (`small`/`default`), **1** (`constrained`), `0` = tắt hẳn. Idle quá 60 s (`constrained`) / 5 phút → stop process, **giữ** index. Một server Node là ~40–60 MB RSS, tức 4–6% RAM của máy 1 GB — nên cap là cứng, không phải "cho tiện".
- **`constrained` chỉ bật tầng 0 + 1**, tắt tầng 2: subagent là context thứ hai cùng sống trong một process, đúng thứ không được phép trên máy 1 GB khi chưa cần. Tầng 2 chỉ bật khi context cha đã vượt ~70% trần.
- **Handshake:** `initialize` → kiểm `protocolVersion`; không khớp thì server bị `disabled` + event, daemon vẫn sống (cùng mô hình `plugin.disabled`, AD-23).
- **Không hỗ trợ ở bất kỳ giai đoạn nào:** `sampling`, `roots`, `elicitation` → trả `-32601`. Đó là các method biến server thành bên điều khiển model/người dùng; cấp chúng là mở đường leo thang quyền ngoài AD-22.
- **Fail mode:** server chết giữa `tools/call` → `tool_result` lỗi mã `mcp_unavailable`, turn **tiếp tục** (tool MCP không nằm trên đường bắt buộc, khác `guard` fail-closed). Server chưa cài hoặc chưa chạy → `mcp_search` ghi rõ "server chưa khả dụng"; agent báo người dùng, **không** tự cài.
- **Sandbox:** `inherited` chạy trong cùng cây sandbox như tool `bash` (AD-19); `external` phải khai báo tường minh và được nêu trong `/api/meta`. Trên Android (`sandbox = "none"`) cả hai chỉ còn permission rules.
- **Event:** dùng lại `mcp.status_changed` (đã có trong protocol) cho lifecycle, thêm `mcp.tool_list_changed` và `mcp.tool_search` (query, số trúng, độ trễ index). `notifications/message` và `progress` **không** vào part — chỉ `mcp.log`: event log không phình trên flash nhỏ (AD-19), và tiến trình của server không chiếm context model.
- **Dữ liệu MCP là dữ liệu không tin cậy:** kết quả tool MCP không cấp quyền, không đổi phạm vi crate, không được đọc như instruction (cùng nguyên tắc AD-24).

**Lý do.** MCP chỉ có tool, không có hook — nó là **tập con** của mô hình plugin, nên tái dùng manifest/limits/lifecycle thay vì dựng hệ thống thứ hai (§15.3). Đặt dưới cùng chính sách nghĩa là một MCP server không có quyền nhiều hơn một plugin Lua. Còn việc giữ tool MCP ngoài tool list là để **chi phí context tỉ lệ với số capability đã dùng, không phải số capability đã khai báo** — điều kiện để "khai báo 3 server" và "chạy trên 1 GB" không loại trừ nhau.

**Đã loại.**
- Nạp tool MCP vào tool list của agent chính (mô hình Cline/Roo): 50–200 `inputSchema` mỗi lượt, vừa ăn token vừa giữ chuỗi JSON trong RAM suốt session.
- Bắt mọi lần gọi đi qua subagent: gấp đôi lượt model cho ca một-phát, và trên `constrained` là hai context cùng sống.
- Nhúng MCP SDK và giữ connection thường trực cho mọi server đã khai báo: ~40–60 MB × N, chết trước khi turn đầu tiên chạy.
- Cho tool MCP bỏ approval vì "server đã được người dùng cài": cài một server không phải là cấp quyền chạy lệnh trên mọi file.
- Tự cài hoặc tự tải MCP server hộ người dùng (`npx -y`, `pip install`, tải binary): biến Ananke thành bên phân phối code chưa ai kiểm chứng, và biến lỗi mạng thành lỗi của Ananke.

---

## 4. Kiến trúc tổng thể

```
                        ┌──────────────────────────────────────────┐
                        │   ananke (headless engine + service)     │
                        │   một daemon per user, không kèm UI      │
                        │                                          │
  TUI (TS/OpenTUI) ────►│  HTTP router (axum)                      │
  Desktop (Electron) ──►│    ├── /api/session                      │
  Web (browser) ───────►│    ├── /api/event    (SSE)               │
  Phone (LAN/remote) ──►│    ├── /api/pair     (device token)      │
  Script / CI ────────►│    ├── /api/limits                       │
                        │    ├── /api/plugin   (AD-20)             │
                        │    ├── /api/fs, /api/lsp, /api/mcp       │
                        │    ├── /api/tui  (chỉ khi TUI attach)    │
                        │    └── /api/meta, /api/openapi.json      │
  Embedded (in-proc) ──►│                                          │
                        │  Session manager (admission control)     │
                        │    └── 1 core session / session          │
                        │         ├── agent loop                   │
                        │         ├── tool registry                │
                        │         ├── provider adapter             │
                        │         └── sandbox policy               │
                        │                                          │
                        │  Store: SQLite (state + event log)       │
                        │  Plugin host (AD-20 … AD-23)             │
                        └──────────────────────────────────────────┘
                           │                                   │
                           ▼                                   ▼ JSON-RPC/stdio
        workspace / worktree / MCP servers / LSP     plugin process
                                                    (ananke-plugin-lua hoặc ngôn ngữ khác)
```

Ba đường biên tách biệt:

- **Engine ↔ UI** là HTTP/SSE. `ananke` (Rust) chỉ chứa engine, service, CLI — chạy được headless, không cần runtime JS (AD-14).
- **TUI là artifact cài rời** (`@ananke/tui`); điện thoại/desktop là client riêng trên cùng protocol (AD-18). Không có client nào là điều kiện để service hoạt động.
- **Engine ↔ plugin** là JSON-RPC/stdio. Plugin là process riêng (AD-20); Lua chỉ là runner mặc định chứ không phải đường duy nhất; plugin không tự chạm filesystem/shell mà gọi capability (AD-22).

Luồng một turn:

```
Client                Service                     Core               Tool/Model
  │  POST /turn         │                            │                    │
  ├────────────────────►│  tạo Turn, ghi DB          │                    │
  │                     ├──── Command::UserInput ───►│                    │
  │  202 {turnID}       │                            ├── model call ─────►│
  │◄────────────────────┤                            │◄── stream ─────────┤
  │                     │◄─── Event::PartDelta ──────┤                    │
  │  SSE: part.delta    │                            │                    │
  │◄────────────────────┤                            ├── tool call ──────►│
  │  SSE: permission.requested                       │                    │
  │◄────────────────────┤                            │  (turn tạm dừng)   │
  │  POST /permission   │                            │                    │
  ├────────────────────►├── Command::Permission ────►│                    │
  │  SSE: part.completed, turn.completed             │                    │
  │◄────────────────────┤                            │                    │
```

Turn được ghi DB **trước khi** dispatch tool (AD-17). Nếu service bị kill trong lúc chờ tool dài, lần khởi động sau thấy turn `awaiting_tool` và hoàn tất từ đó, không mất turn.

---

## 5. API surface (giai đoạn 1)

### 5.1 Meta & service

| Method | Path | Mô tả |
|---|---|---|
| GET | `/api/meta` | `protocolVersion`, `capabilities`, `serverVersion`, `deviceProfile` (`constrained`/`small`/`default`), `sandbox` (`landlock`/`seatbelt`/`restricted-token`/`none`) |
| GET | `/api/limits` | Giới hạn tài nguyên hiệu lực (AD-15) |
| PATCH | `/api/limits` | Ghi đè giới hạn; bị chặn nếu vượt ngưỡng an toàn của profile |
| GET | `/api/openapi.json` | Spec OpenAPI 3.1 |
| GET | `/api/doc` | Swagger UI |
| GET | `/healthz` `/readyz` | Liveness / readiness |
| POST | `/api/shutdown` | Chỉ khi bật `--allow-shutdown` |

### 5.2 Session & turn

| Method | Path | Mô tả |
|---|---|---|
| GET | `/api/session` | Danh sách session |
| POST | `/api/session` | Tạo session (`workspace`, `model`, `worktree?`) |
| GET | `/api/session/{id}` | Chi tiết + state snapshot |
| PATCH | `/api/session/{id}` | Đổi title, model, approval policy |
| DELETE | `/api/session/{id}` | Xoá kèm cascade |
| POST | `/api/session/{id}/fork` | Fork tại một turn |
| GET | `/api/session/{id}/state` | Snapshot để resync |
| GET | `/api/session/{id}/event` | SSE, replay từ `since` |
| POST | `/api/session/{id}/turn` | Gửi input, trả `202 {turnID}` |
| POST | `/api/session/{id}/turn/{turnID}/interrupt` | Huỷ turn |
| POST | `/api/session/{id}/turn/{turnID}/steer` | Bổ sung chỉ dẫn giữa turn |
| GET | `/api/session/{id}/message` | Lịch sử message + parts |
| POST | `/api/session/{id}/permission/{permID}` | Trả lời approval |
| POST | `/api/session/{id}/revert` | Rollback tới trước một turn |
| POST | `/api/session/{id}/compact` | Nén context thủ công |
| GET | `/api/session/{id}/diff` | Diff tích luỹ của session |

### 5.3 Workspace & môi trường

| Method | Path | Mô tả |
|---|---|---|
| GET | `/api/project` / `/api/project/current` | Project và workspace hiện tại |
| GET | `/api/path` / `/api/vcs` | Path, git status/branch |
| GET | `/api/file` / `/api/file/content` | Liệt kê / đọc file |
| GET | `/api/find` / `/api/find/file` / `/api/find/symbol` | Text search, fuzzy file, symbol |
| GET | `/api/lsp` | Trạng thái LSP |
| GET | `/api/mcp` / `POST /api/mcp` | Trạng thái và thêm MCP server động |
| GET | `/api/model` / `/api/provider` | Catalog model & provider |

### 5.4 Global

| Method | Path | Mô tả |
|---|---|---|
| GET | `/api/event` | SSE toàn cục |
| GET | `/api/config` / `PATCH /api/config` | Đọc/ghi config |
| POST | `/api/command` | Slash command |
| POST | `/api/log` | Client ghi log vào session log |

Giai đoạn 2 (chưa cam kết): ACP bridge cho editor.

### 5.5 TUI control (giai đoạn 1)

TUI đang chạy đăng ký với service; service chuyển tiếp các lệnh điều khiển (AD-13). Không có TUI nào attach thì trả `409 tui_not_attached`.

| Method | Path | Mô tả |
|---|---|---|
| POST | `/api/tui/append-prompt` | Chèn text vào prompt đang gõ |
| POST | `/api/tui/submit-prompt` | Gửi prompt hiện tại |
| POST | `/api/tui/clear-prompt` | Xoá prompt hiện tại |
| POST | `/api/tui/execute-command` | Chạy slash command trong TUI |
| POST | `/api/tui/open-help` | Mở help |
| POST | `/api/tui/open-sessions` | Mở danh sách session |
| POST | `/api/tui/open-themes` | Mở chọn theme |
| POST | `/api/tui/open-models` | Mở chọn model |
| POST | `/api/tui/show-toast` | Hiện toast (`level`, `message`) |
| GET | `/api/tui/control/next` | TUI long-poll lấy lệnh điều khiển kế tiếp |
| POST | `/api/tui/control/response` | TUI trả kết quả cho lệnh đó |

### 5.6 Pairing & thiết bị từ xa (AD-18)

Chỉ bật khi service chạy với `--listen` ngoài loopback. Pairing đổi one-time code lấy device token riêng cho từng thiết bị.

| Method | Path | Mô tả |
|---|---|---|
| POST | `/api/pair` | `{code, deviceName, platform}` → `{deviceID, deviceToken, expiresAt}` |
| GET | `/api/pair/device` | Danh sách thiết bị đã pair, lần dùng cuối |
| DELETE | `/api/pair/device/{deviceID}` | Thu hồi token của một thiết bị |

- One-time code sinh bởi `ananke pair`, hết hạn sau 5 phút, dùng một lần.
- Device token có scope (`session:*`, `permission:respond`, …) và hạn; không dùng chung với token loopback.
- Thu hồi một thiết bị không ảnh hưởng thiết bị khác hay TUI local.

### 5.7 Plugin (AD-20 … AD-23, giai đoạn 2)

| Method | Path | Mô tả |
|---|---|---|
| GET | `/api/plugin` | Danh sách plugin: `id`, `runtime`, `version`, trạng thái, lỗi gần nhất |
| POST | `/api/plugin` | Cài từ thư mục/archive cục bộ; engine đọc `ananke-plugin.json` |
| PATCH | `/api/plugin/{id}` | Bật/tắt, ghi đè `permissions`, `limits` |
| DELETE | `/api/plugin/{id}` | Gỡ plugin; `?purge=true` xoá luôn state KV |
| GET | `/api/plugin/{id}/log` | Log và lỗi gần nhất của plugin |

- Giai đoạn 1 **không** có endpoint cài từ URL/registry: chỉ cài từ đường dẫn cục bộ (marketplace ở §13).
- `POST /api/plugin` không tự chạy plugin; engine chỉ khởi động runner khi plugin được `PATCH` sang `enabled`, để cài đặt không kèm thực thi.

---

## 6. Event taxonomy

```
service.ready            service.health           service.stopping
session.created          session.updated          session.archived
session.unloaded         session.compacted
turn.started             turn.completed           turn.failed
turn.interrupted         turn.usage               turn.awaiting_tool
part.started             part.delta               part.completed
permission.requested     permission.resolved
todo.updated             plan.updated
file.changed             lsp.diagnostic
mcp.status_changed       model.catalog_updated
subagent.queued          subagent.started         subagent.completed
plugin.started           plugin.stopped           plugin.disabled
plugin.error
resource.throttled       event.resync_required
warning                  log
```

Ví dụ frame SSE:

```
id: 1043
event: part.delta
data: {"sessionID":"ses_01H...","turnID":"trn_01H...","partID":"prt_01H...",
       "kind":"text","delta":"Đang chạy "}
```

Quy ước:

- Client **bỏ qua** event type chưa biết (forward compatibility).
- Field mới chỉ được thêm, không đổi nghĩa; xoá field phải qua major version.
- `part.delta` được **batch theo 16–33ms** ở server để giảm overhead HTTP; client không được giả định mỗi token là một frame.
- `part.delta` **không được persist** (AD-08): chỉ broadcast cho subscriber đang sống. Event log chỉ ghi `part.started` / `part.completed` kèm snapshot. Client reconnect giữa một turn đang stream nhận `event.resync_required` rồi lấy `GET /api/session/{id}/state`.
- `subagent.*` phát trên `sessionID` của session con; client nhận biết quan hệ qua `session.created` có `parentID`.
- `resource.throttled` mang `{limit, current, action: "queued" | "rejected"}`; đây là tín hiệu điều tiết, không phải lỗi.
- `plugin.error` mang `{pluginID, phase, code, message}` và **không** đồng nghĩa turn thất bại: turn chỉ dừng khi hook `guard` từ chối hoặc khi lỗi nằm trong tool do plugin đăng ký (AD-23).
- Thứ tự trong một session là toàn phần theo `seq`; giữa các session không đảm bảo.

---

## 7. Bảo mật

### 7.1 Đường biên

- Mặc định chỉ UDS/pipe với quyền `0600`; không có bề mặt mạng.
- TCP loopback: bắt buộc bearer token sinh lúc start, ghi vào `service.json` với `0600`; từ chối request có `Origin` ngoài allowlist.
- TCP public: bắt buộc TLS (`--tls-cert`, `--tls-key`) + token/JWT; từ chối `alg: none`; bind `0.0.0.0` phải là lựa chọn tường minh.
- Thiết bị từ xa dùng **device token** lấy qua `/api/pair` (AD-18): scope hẹp, có hạn, thu hồi độc lập; không dùng lại token loopback.
- Không expose endpoint nào có thể đọc API key provider.
- Plugin không nằm trong đường biên quyền: nó không nhận token loopback/device token, và mọi capability đều qua RPC có scope (AD-22).

### 7.2 Sandbox thực thi tool

| Nền tảng | Cơ chế |
|---|---|
| macOS | Seatbelt profile sinh động theo policy |
| Linux ≥ 5.13 | Landlock + seccomp; ưu tiên `bwrap` nếu có |
| Linux < 5.13 / không user namespace | Không enforce được — báo `sandbox = "none"` |
| Android/Termux | Không Landlock, không userns → `sandbox = "none"`; permission rules là hàng rào duy nhất |
| Windows | Restricted token + AppContainer, hoặc WSL2 |

Policy: `read-only`, `workspace-write` (kèm `writable_roots`, `network`), `danger-full-access`, `external` (host tự lo, engine không enforce).

**Sandbox là theo năng lực, không theo lời hứa.** `/api/meta` trả `sandbox` thực tế đo được lúc start; client (kể cả điện thoại) phải hiển thị cảnh báo khi giá trị là `none` và không được giả định `workspace-write` có hiệu lực.

**Rủi ro đã biết.** MCP server là process con, có thể nằm ngoài sandbox của engine. Giai đoạn 1: chấp nhận và ghi rõ trong docs; mỗi MCP server khai báo `sandbox: inherited | external`. Giai đoạn 3: chạy MCP server trong cùng sandbox tree khi khả thi.

### 7.3 Permission rules

Thứ tự ưu tiên (sau ghi đè trước):

```
default → agent → session → project → user
```

Mỗi rule: `allow | deny | ask`, pattern theo tool. Với shell, dùng **arity** để phân biệt lệnh nguy hiểm:

```toml
[[permission]]
tool = "bash"
match = "git *"
action = "ask"

[[permission]]
tool = "bash"
match = "git status"
action = "allow"

[[permission]]
tool = "bash"
match = "git push*"
action = "deny"
```

Secret: API key lưu OS keychain; log redact theo pattern; data dir quyền `0700`, file `0600`.

---

## 8. Tích hợp từng surface

### 8.1 TUI (TypeScript + OpenTUI, giai đoạn 1)

- Package `packages/tui/`, phát hành **rời** (`@ananke/tui`) và cài bằng `ananke tui install`; **không** nằm trong binary `ananke` (AD-14). Viết bằng TypeScript, render bằng OpenTUI, dùng `@ananke/sdk`.
- Khởi động: `ananke run` → đảm bảo service tồn tại → `discover()` đọc `service.json` trong data dir → `GET /api/meta` kiểm tra `protocolVersion` → mở SSE (`since` cuối nếu resume) → render snapshot + stream.
- Không chứa logic agent. Phím tắt chỉ sinh intent: `POST /api/session/{id}/turn`, `interrupt`, `steer`, trả lời permission.
- Không có chế độ in-process: engine là process Rust riêng, TUI luôn qua UDS. Khác biệt transport (UDS local vs TCP remote) do SDK che, TUI không cần biết.
- TUI **host** control channel: long-poll `GET /api/tui/control/next`, thực thi, trả `POST /api/tui/control/response`. Đây là đường để IDE/script điều khiển TUI đang mở (AD-13).
- Reconnect sau khi mất kết nối: đọc `seq` cuối đã render, stream tiếp; nếu server trả `event.resync_required` thì gọi `/state` lấy snapshot.

Ranh giới cần giữ: nếu một tính năng TUI cần dữ liệu mà API chưa có, **bổ sung endpoint**, không đọc thẳng file/DB.

Trên profile `constrained`, TUI full bị tắt mặc định (AD-15): `ananke run` gợi ý `ananke run --lite` (text client trong binary Rust) hoặc dùng app điện thoại. Đây là lựa chọn của người dùng, không phải tải tự động.

### 8.2 Desktop app (Electron, giai đoạn 2)

```
React renderer
   │  IPC qua preload bridge (contextIsolation)
   ▼
Electron main
   │  service discovery + lifecycle + native (menu, tray, deep link, updater)
   ▼
ananke service (UDS)  ──►  SSE + REST
```

- App **không** spawn engine riêng. Nó đảm bảo service tồn tại, và là một client.
- Nhiều cửa sổ = nhiều SSE subscription trên cùng session, không nhân bản engine.
- Cache phía client (TanStack Query) khoá theo `(method, params)`; invalidate bằng event.
- Đóng app không dừng turn; mở lại thì replay từ `seq` cuối.

### 8.3 Web/API + SDK (giai đoạn 2)

- Service phục vụ luôn web UI tại `/` → same-origin, không cần CORS.
- Browser dùng `@ananke/sdk` (generated + wrapper SSE/retry).
- Script/CI dùng SDK hoặc `ananke api` (CLI proxy mỏng, không chứa logic).
- Auth: cookie session do service cấp, hoặc bearer token cho máy-máy.

### 8.4 IDE extension (giai đoạn 3)

Hai lựa chọn, quyết định sau khi có TUI ổn định:

- **HTTP client**: extension gọi thẳng service (đơn giản, đồng bộ với desktop).
- **ACP bridge**: `ananke-acp` nói Agent Client Protocol với Zed/JetBrains, chuyển tiếp vào service.

Không chọn "extension tự chạy agent".

### 8.5 Mobile client (điện thoại, giai đoạn 2)

Điện thoại là **UI chính** khi engine chạy headless trên máy nhỏ (Pi, TV box, NAS) — không cần TUI. Endpoint pairing và TLS có từ M1 để client bên thứ ba dựng được sớm; app chính thức ở M2.

- Pairing một lần: `ananke pair` in QR `{endpoint, cert_fingerprint, one-time code}` → app gọi `POST /api/pair` → lưu device token trong secure storage của HĐH.
- Kết nối: ưu tiên LAN qua mDNS `_ananke._tcp`; ngoài LAN thì qua VPN/tunnel do người dùng tự dựng (không có relay cloud — §13).
- Dùng đầy đủ: xem/stream turn, trả lời approval, interrupt, steer, tạo session, xem diff. Không có endpoint nào dành riêng cho TUI là điều kiện.
- Mất mạng giữa turn: SSE reconnect với `since`, hoặc `event.resync_required` → `GET /state`. Turn vẫn chạy trên engine.
- Hiển thị `sandbox` từ `/api/meta`: máy Android/Termux trả `none` thì app cảnh báo rõ trước khi cho chạy lệnh nguy hiểm.

Không chọn: app điều khiển TUI qua `/api/tui/*` — biến điện thoại thành "remote của terminal", hỏng ngay khi TUI không chạy.

---

## 9. Cấu trúc repo

```
ananke/
  Cargo.toml                     # workspace
  crates/
    ananke-core/                 # agent loop, context, instructions (AD-24), compaction, resource limits / admission
    ananke-provider/             # adapter: Responses API, Anthropic, OpenAI-compatible
    ananke-tools/                # bash, read, write, edit, grep, glob, task, skill
    ananke-sandbox/              # seatbelt / landlock / windows, phát hiện năng lực
    ananke-store/                # SQLite, migrations, event log, retention
    ananke-protocol/             # DTO + schema + codegen bin
    ananke-server/               # axum router, SSE, middleware, pairing
    ananke-service/              # daemon lifecycle, discovery, auth
    ananke-plugin-host/          # JSON-RPC host, manifest, lifecycle, hook budget (AD-20 … AD-23)
    ananke-plugin-lua/           # binary runner LuaJIT (mlua), stdlib bị cắt (AD-21)
    ananke-mcp/                  # MCP client: stdio + http, index tool, 3 tầng search/call/task (AD-25)
    ananke-client/               # Rust client cho third-party (chỉ phụ thuộc protocol)
    ananke-embedded/             # in-process transport
    ananke-cli/                  # binary: run / serve / service / api / pair / tui / plugin / upgrade / mcp
    ananke-acp/                  # (phase 3)
  packages/
    tui/                         # @ananke/tui, cài rời (TypeScript + OpenTUI)
    sdk-ts/                      # @ananke/sdk (generated + wrapper)
    sdk-py/                      # ananke-sdk (generated)
    plugin-sdk-ts/               # @ananke/plugin-sdk: wrapper protocol (không bắt buộc, AD-20)
    desktop/                     # Electron shell
    mobile/                      # app điện thoại (client trên cùng protocol)
    web/                         # web client
  plugins/
    examples/lua/                # plugin mẫu, dùng trong CI
  openapi.json                   # hợp đồng đã commit
  docs/
    ARCHITECTURE.md              # tài liệu này
```

---

## 10. Lộ trình

| Mốc | Nội dung | Tiêu chí hoàn thành |
|---|---|---|
| **M0 — Skeleton** | workspace crates, protocol type, router `/api/meta`, session CRUD, một provider, tool `bash`/`read`, SQLite, profile thiết bị + `/api/limits`, phát hiện năng lực sandbox, build `aarch64-linux-android` | `curl` tạo session và chạy được 1 turn; `openapi.json` sinh ra và commit; `/api/meta` báo đúng `deviceProfile` và `sandbox` trên máy test |
| **M1 — Turn & approval** | agent loop đầy đủ, streaming Part, permission rules, **file hướng dẫn + system slot (AD-24)**, SSE replay, interrupt/steer, persist turn trước khi dispatch tool (AD-17), retention event log, TLS + `/api/pair`, TUI TypeScript (OpenTUI + `@ananke/sdk`), `/api/tui/*` control channel | `AGENTS.md` ở root được nạp đúng một lần, sửa file thì turn sau thấy bản mới, vượt ngân sách có `instructions.truncated`; một `AGENTS.md` đòi bỏ approval không làm đổi permission rules; TUI hoàn thành một task sửa file có approval; kill TUI rồi mở lại vẫn thấy đúng lịch sử; script gọi `/api/tui/append-prompt` chèn được text vào TUI đang mở; RSS service idle + 2 session < 80 MB trên máy 1 GB; `kill -9` giữa turn rồi restart vẫn đúng lịch sử, turn `awaiting_tool` tự hoàn tất |
| **M2 — Đa client & plugin** | service discovery, desktop app, **app điện thoại + pairing QR**, SSE resync, SDK publish + versioning, web UI, **plugin host + `ananke-plugin-lua` + `/api/plugin` (AD-20, AD-21)** | TUI + desktop mở cùng session, thao tác trên một bên hiện tức thì bên kia; ghép điện thoại qua QR rồi hoàn tất một turn có approval khi máy không cài TUI; reload web không mất turn; cài một plugin Lua đăng ký 1 tool bằng `ananke plugin install`, tool đó chạy có approval như built-in; `kill -9` runner giữa hook không làm hỏng turn |
| **M3 — Mở rộng** | ACP/IDE, Python/Go SDK, `@ananke/plugin-sdk`, hook `transform`/`guard` có budget (AD-23), embedded mode, sandbox hardening, **MCP client `stdio` + `http`, 3 tầng `mcp_search`/`mcp_call`/`task` (AD-25)**, **sub-agent tuần tự theo AD-16** | Tool list của agent chính không chứa `mcp__*`; `mcp_search` trả đúng tool cho 3 câu hỏi mẫu; khai báo 2 MCP server, chỉ server được gọi mới spawn; `constrained` giữ tối đa 1 process sống và chỉ bật tầng 0+1; `kill -9` server giữa call trả `mcp_unavailable` mà turn vẫn xong; SDK Python chạy được một task headless trong CI; plugin viết bằng TS/Python chạy cùng protocol với plugin Lua; test boundary pass; trên profile `constrained`, cha spawn được 1 sub-agent `depth = 1`, vượt cap thì có `subagent.queued` thay vì OOM |
| **M4 — Nâng cao** | subagent nền song song, worktree song song, Code Mode-style tool composition | Hai subagent chạy nền trên hai worktree, tổng hợp về session cha |

---

## 11. Kiểm tra tuân thủ kiến trúc (Definition of Done)

Một PR được coi là đúng kiến trúc khi:

- [ ] Không thêm dependency từ `ananke-core` sang `protocol`/`server`.
- [ ] Không thêm import engine vào `packages/sdk-ts`, `packages/tui`, `packages/mobile`, hay `web`.
- [ ] Thay đổi protocol có cập nhật `openapi.json` và chạy lại codegen trong cùng PR.
- [ ] Endpoint mới có test ở mức HTTP (không test core trực tiếp thay cho test protocol).
- [ ] Event mới có `seq`, có test replay qua `since`.
- [x] Endpoint ghi dữ liệu có permission check nếu chạm filesystem/shell.
- [ ] Không log secret; không trả API key qua bất kỳ response nào.
- [ ] `part.delta` không được ghi vào event log; test khẳng định điều này.
- [ ] Mọi giới hạn trong AD-15 có test: vượt cap trả `429`/`resource.throttled`, không bị OOM.
- [ ] Turn được persist trước khi dispatch tool; test `kill -9` giữa turn không mất lịch sử.
- [ ] Build `aarch64-linux-android` (và `aarch64-unknown-linux-gnu`) chạy trong CI, không cần build trên thiết bị.
- [ ] Endpoint phục vụ client từ xa kiểm tra scope của device token, không chỉ "đã đăng nhập".
- [ ] Plugin chạy như process riêng, không nhúng runtime vào engine; test khẳng định engine sống sót khi runner `kill -9`.
- [ ] Hook `transform`/`guard` có budget và test timeout: quá budget không treo turn, 3 lần liên tiếp thì `plugin.disabled` + event (AD-23).
- [ ] Plugin không đọc/ghi file hay spawn process ngoài capability RPC; trên `sandbox = "none"` permission rules vẫn chặn (AD-22).
- [ ] Tool do plugin đăng ký ghi đủ input/output vào event log và đi qua approval như built-in tool.
- [ ] System message được dựng lại mỗi lượt từ nguồn ngoài session; `session.messages()` không chứa system; test khẳng định replay `since` không nhân bản instructions (AD-24).
- [ ] File hướng dẫn không cấp quyền: test một `AGENTS.md` yêu cầu bỏ approval không thay đổi permission rules (AD-24).
- [ ] Tool list của agent chính không chứa tên `mcp__*`; `inputSchema` của MCP không có trong request gửi provider; test khẳng định `mcp_search` không spawn process (AD-25).
- [ ] MCP server chỉ spawn khi tool được gọi; `constrained` không vượt `max_mcp_servers` process và không bật tầng 2; `kill -9` server giữa call chỉ trả `mcp_unavailable` (AD-25).
- [ ] Tool MCP ghi vào event log với `serverID` + tên gốc, không chỉ tên đã namespace (AD-25).
- [ ] `mcp_call` chỉ nhận tool đã có trong index của server đã khai báo; kết quả vượt budget bị cắt và có cờ `truncated`, không đẩy nguyên văn vào history (AD-25).

---

## 12. Rủi ro và đánh đổi

| Rủi ro | Ảnh hưởng | Giảm thiểu |
|---|---|---|
| HTTP/SSE overhead với delta tần suất cao | TUI giật, CPU cao | Batch delta 16–33ms; TUI local đi qua UDS, không qua TCP |
| SSE qua proxy/load balancer bị buffer | Remote streaming trễ | Hướng dẫn tắt buffering; tài liệu hoá; cho phép WebSocket sau này |
| Service version skew với client | Client gọi method không tồn tại | `protocolVersion` + `capabilities`; client degrade; major bump có migration |
| Orphan/zombie daemon, tranh chấp lockfile | Hai engine cùng workspace | Singleton lock; `service status`; kiểm tra `pid` + start time; crash recovery |
| MCP server ngoài sandbox | Thoát sandbox | `sandbox: inherited` là mặc định; `external` phải khai báo tường minh và hiện trong `/api/meta` (AD-25) |
| Mỗi MCP server là process Node/Python ~40–60 MB | Vượt ngân sách 1 GB khi khai báo nhiều server | Lazy spawn, idle stop 60 s, `max_mcp_servers = 1` trên `constrained`, index trong SQLite thay vì schema trong RAM (AD-25) |
| Search-first thêm một lượt model cho lần dùng đầu của mỗi capability | Chậm hơn tool built-in ở ca một-phát, nhất là trên máy yếu | Index BM25 trên tên + mô tả + alias trong `mcp.json`; sticky pin ≤ 3 tool cho N lượt; đo số lượt trung bình trước khi tối ưu thêm (AD-25) |
| Subagent MCP (tầng 2) là context thứ hai trong cùng process | Trên 1 GB, hai context cùng sống dễ vượt trần và bị LMK giết | Tầng 2 tắt trên `constrained`; chỉ bật khi context cha > 70% trần; context con drop sau khi trả kết quả (AD-25) |
| `AGENTS.md` trong repo độc hại bơm chỉ thị ("bỏ approval", "gửi ~/.ssh") | Model làm theo lệnh của repo thay vì của người dùng | Instructions là dữ liệu không tin cậy, không cấp quyền; approval vẫn chặn; ngân sách byte chặn nhồi text (AD-24) |
| Protocol công khai bị phụ thuộc | Khó breaking change | Chỉ thêm; deprecate có lịch; major version có tài liệu migration |
| TUI/desktop/web dùng chung `@ananke/sdk` → một bug lan ra cả ba surface | Hỏng đồng loạt | SDK generated + wrapper mỏng có test riêng; conformance test chạy cùng bộ case qua HTTP |
| TUI là client TS, engine là Rust → lệch cách hiểu protocol | Bug chỉ hiện ở một phía | `openapi.json` là hợp đồng duy nhất; test mức HTTP chạy từ cả Rust client và TS SDK |
| Máy người dùng không có runtime JS | `ananke run` không mở được TUI | Không tự tải TUI; in hướng dẫn `ananke tui install` hoặc dùng app điện thoại. Engine vẫn chạy headless (AD-14) |
| Android LMK giết daemon khi máy thiếu RAM | Turn đang chạy bị dừng đột ngột | Admission control (AD-15) + turn persist trước dispatch (AD-17); restart đọc lại SQLite |
| Event log tăng vô hạn trên flash nhỏ | Mòn flash, đầy đĩa | `part.delta` không persist; retention + checkpoint `TRUNCATE` bật từ M1 (AD-19) |
| Android/Termux không có sandbox | Tool chạy toàn quyền trong user Android | `/api/meta` báo `sandbox = "none"`; client cảnh báo; permission rules là hàng rào duy nhất (AD-19) |
| Device token của điện thoại bị lộ | Kẻ khác điều khiển agent từ xa | Scope hẹp + hạn + thu hồi từng thiết bị; bắt buộc TLS; `DELETE /api/pair/device/{id}` (AD-18) |
| Plugin treo hoặc chết giữa hook | Turn treo, daemon chết theo | Budget 100 ms; `observe` fail-open, `transform`/`guard` fail-closed; 3 timeout → `plugin.disabled` (AD-23) |
| LuaJIT 2.1 beta, ít được maintain | Runner mục ruỗng theo thời gian | Cô lập trong `ananke-plugin-lua`; API plugin không đổi khi chuyển sang PUC Lua 5.4 qua `mlua` (AD-21) |
| Plugin leo thang quyền qua capability RPC | Đọc/ghi ngoài phạm vi, lộ secret | Engine enforce sandbox + permission, không expose syscall; permission phải khai báo trong manifest (AD-22) |
| Plugin `binary` tự viết trên Android (không sandbox) | Toàn quyền user Android | Mặc định chỉ nhận runner do Ananke phát hành; bật `binary` phải tường minh + cảnh báo `sandbox = "none"` |

---

## 13. Những gì không làm ở giai đoạn 1

- Multi-tenant server / hosting cloud.
- Marketplace plugin, và cài plugin trực tiếp từ URL/registry (chỉ cài từ đường dẫn cục bộ — §5.7).
- Plugin chạy in-process hoặc nhúng runtime (dylib, WASM) — mọi plugin là process riêng (AD-20).
- Đồng bộ session qua cloud giữa nhiều máy.
- Relay/tunnel do Ananke cung cấp cho client ngoài LAN (người dùng tự dựng VPN/tunnel).
- Hỗ trợ Windows sandbox đầy đủ ngang macOS/Linux (M3).
- WebSocket transport (chỉ thêm nếu SSE không đủ).
- MCP client, cả `stdio` và HTTP/streamable — thuộc M3 (AD-25).
- Các method `sampling`/`roots`/`elicitation` — không hỗ trợ ở bất kỳ giai đoạn nào (AD-25).
- Cơ chế "trust" cho file hướng dẫn — mọi `AGENTS.md` đều là dữ liệu không tin cậy (AD-24).

---

## 14. Câu hỏi mở

1. Provider layer: dùng SDK chính thức của từng hãng, hay tự viết lớp HTTP như OpenCode V2 để kiểm soát streaming/usage?
2. Permission `allow_always` lưu ở scope nào là mặc định (project hay user)?
3. Có cần event log compact theo thời gian ngay từ M1, hay để M3?
4. Ngưỡng vào profile `constrained` (hiện `< 1.5 GB`) có nên nâng lên `< 2 GB` để chừa chỗ cho Android system?
5. Retention event log mặc định tính theo ngày (30 ngày) hay theo dung lượng (200 MB)?
6. Client ngoài LAN: có bao giờ cung cấp relay của Ananke, hay giữ nguyên "người dùng tự dựng VPN/tunnel"?
7. Hook budget 100 ms có nên tách làm hai ngưỡng — 100 ms cho hook thuần tính toán, dài hơn cho hook được phép gọi `http`/`exec` (vì chúng không nên nằm trên đường đồng bộ)?
8. Giai đoạn 1 có cho plugin khai báo `runtime: "binary"`, hay chỉ chấp nhận các runner do Ananke phát hành (Lua, Node, Python)?
9. Plugin có được đăng ký **slash command** và **message renderer** như Pi, hay chỉ tool + hook?
10. `AGENTS.md` có nên hỗ trợ `@import` trỏ tới file khác, hay giữ phẳng để ngân sách byte là tuyệt đối?
11. `AGENTS.md` ở thư mục con: nạp theo file mà tool vừa chạm (động, đúng ngữ cảnh) hay theo toàn bộ cây workspace (tĩnh, dễ đoán hơn nhưng tốn ngân sách)?
12. Sticky pin của `mcp_search` nên tính theo **số lượt** hay theo **turn**? Theo lượt thì nhả sớm khi turn dài, theo turn thì giữ lâu hơn nhưng dễ tích lại thành tool list thu nhỏ.
13. `mcp_search` dùng BM25 thuần trên tên + mô tả, hay thêm embedding? Embedding tăng độ trúng nhưng mang thêm model/vector store — thứ đáng ngờ trên máy 1 GB.

---

## 15. Hệ plugin

### 15.1 Ba đường biên mở rộng

| Đường biên | Ai dùng | Đổi được | Không đổi được |
|---|---|---|---|
| **Seam trong Rust** (trait lúc compile) | người fork Ananke | provider, sandbox, store, transport | — nhưng phải build lại |
| **Plugin process** (JSON-RPC, AD-20) | người dùng cuối, bên thứ ba | tool, hook, capability, UI phụ trợ | agent loop, thứ tự turn, prompt mặc định |
| **UI client** (HTTP/SSE) | TUI, desktop, điện thoại, editor | toàn bộ cách hiển thị | mọi thứ phía server |

Plugin **không** thay được agent loop. Đây là khác biệt có chủ ý với DeepSeek Harness: DSH cho plugin thay cả loop, nhưng mọi plugin chạy in-process với toàn quyền máy. Ananke chọn ranh giới process, nên loop phải là Rust cố định.

### 15.2 So sánh

| | DeepSeek Harness | OpenCode | Pi | Ananke |
|---|---|---|---|---|
| Đơn vị mở rộng | plugin Cordis (service + event + effect) | hook object TS | extension TS (`jiti`) | process JSON-RPC |
| Ngôn ngữ | TypeScript | TypeScript | TypeScript | bất kỳ; Lua có runner sẵn |
| Thay được lõi | có (kể cả agent loop) | không | không | không (tool + hook) |
| Cô lập | không | không | không | process + capability qua engine |
| Hook chặn tool | waterfall | `tool.execute.before` throw | `tool_call` | `guard` có budget |
| Phân phối | bundle/profile + patch | npm + thư mục | npm/git | thư mục + manifest |
| Cold start | ~100–300 ms | ~100–300 ms | ~100–300 ms | 2–10 ms (Lua) |

### 15.3 Khi nào dùng cái nào

- Thêm một tool, hoặc một kiểm tra trước khi chạy tool → **plugin**.
- Đổi cách gọi model, thêm provider → **plugin** nếu chỉ cần `http` + định dạng request; **seam** nếu cần streaming/usage riêng.
- Thay agent loop, cách nén context, thứ tự turn → **fork**, không phải plugin.
- Đã có MCP server → dùng **MCP** (AD-25); MCP chỉ có tool, không có hook, và tool của nó vẫn phải đi qua `mcp_search` → `mcp_call`/`task`, không vào tool list.
- Cần UI khác → **client** trên `/api`, không phải plugin.

### 15.4 Vì sao chưa WASM

WASM là ranh giới tốt nhất về quyền, nhưng ở giai đoạn 1 phải mang host runtime (~vài MB), bộ SDK per-language chưa có, và không giải quyết nhu cầu lớn nhất là "gọi `http` rồi đăng ký một tool". Khi plugin process đã ổn định, WASM có thể thành một giá trị `runtime` khác trong manifest mà không đổi giao kèo với người viết plugin.

### 15.5 Chi phí

Mỗi điểm hook là một round-trip JSON-RPC ~0,1–0,3 ms với payload nhỏ. Một turn điển hình có ~5 điểm hook × 2–3 plugin ≈ 15 round-trip ≈ 3 ms — không đáng kể so với latency model. Không hook nào chạy trên từng token (AD-23).

---

## Phụ lục A — Ví dụ client

```ts
import { Ananke, discover } from "@ananke/sdk";

const endpoint = await discover();          // đọc service.json, fallback spawn service
const client = Ananke.make({ endpoint });

const meta = await client.meta();
if (meta.protocolVersion !== 1) throw new Error("unsupported protocol");

const session = await client.session.create({
  workspace: process.cwd(),
  model: "default",
});

const events = client.session.events(session.id, { since: 0 });
events.on("part.delta", (e) => process.stdout.write(e.delta));
events.on("permission.requested", async (e) => {
  await client.session.permission.respond(session.id, e.permissionID, {
    decision: "allow",
    scope: "once",
  });
});

await client.session.turn.start(session.id, {
  input: [{ type: "text", text: "Chạy test và tóm tắt lỗi" }],
});
```

## Phụ lục B — Bảng đối chiếu

| Khái niệm | Codex | OpenCode | Ananke |
|---|---|---|---|
| Engine | `codex-core` (Rust lib + runtime) | `packages/core` (Effect, TS) | `ananke-core` (Rust lib) |
| Protocol | JSON-RPC 2.0 "lite" | REST + SSE, OpenAPI 3.1 | REST + SSE, OpenAPI 3.1 |
| Client chính | TUI, VS Code, desktop, web | TUI, web, desktop, Slack, ACP | TUI (TS), desktop, web/API, điện thoại |
| Headless (không UI) | `codex exec`, app-server | `opencode serve` | `ananke serve` / `run --headless`, không kèm UI (AD-14) |
| Transport | stdio (chính), WS, in-process | TCP HTTP, ACP stdio, in-memory | UDS (chính), TCP, in-process |
| Process | một app-server / client | một daemon dùng chung | một daemon dùng chung |
| Primitive | Thread / Turn / Item | Session / Message / Part | Session / Turn / Part |
| Approval | server→client request, pause | event + POST | event + POST |
| Codegen | TS + JSON Schema từ Rust | OpenAPI → TS | OpenAPI → TS/Python/Go + Rust types |
| Mở rộng | MCP, không có plugin runtime chung | hook TS in-process | extension TS in-process | process JSON-RPC, mọi ngôn ngữ; Lua runner tích hợp (AD-20, AD-21) |
| Cô lập plugin | — | không | không | process riêng + capability qua engine (AD-22, AD-23) |
| Ranh giới enforce | không rõ ràng công khai | import-boundary test | import-boundary test + cargo deny |