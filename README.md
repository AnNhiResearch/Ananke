# Ananke

Ananke is an open-source coding-agent runtime written in Rust. It follows a **headless engine + shared service** model, allowing multiple clients to connect through HTTP/JSON and SSE.

Developed by **AnNhi Research**, **AnNhi AI**, and **contributors of Ananke**.

## Highlights

- UI-independent Rust agent loop.
- Explicit session, turn, and part state.
- Tool calling, permission approval, and turn interrupt/steer.
- HTTP REST + SSE with OpenAPI 3.1.
- Ordered event log with replay through `since` or `Last-Event-ID`.
- SQLite persistence for sessions, turns, parts, and events.
- Device profiles based on system memory.
- Runtime sandbox capability reporting: Landlock, Seatbelt, Restricted Token, or `none`.
- Built-in tools: `bash`, `read`, `write`, `edit`, `grep`, and `glob`.
- OpenAI-compatible provider with SSE streaming.
- Headless-first design: TUI, desktop, web, and mobile are independent clients.

## Architecture

```text
TUI / Desktop / Web / Mobile / Script
              │ HTTP + SSE
              ▼
       ananke-server / service
              │
              ▼
          ananke-core
       ┌──────┼────────┐
       ▼      ▼        ▼
 provider  tools     store
                         │
                       SQLite
```

Core principles:

1. `ananke-core` does not know about UI, HTTP, or terminals.
2. `ananke-protocol` is the single DTO/OpenAPI contract for clients.
3. State belongs to the server; clients send intent and render state.
4. Events have monotonically increasing `seq` values and support replay/resync.
5. `part.delta` is streamed directly and is not written to the event log.
6. Permissions are requested through events and answered through HTTP POST.
7. Sandbox capability is reported from actual runtime detection, not platform assumptions.

## Workspace crates

| Crate | Role |
|---|---|
| `ananke-core` | Agent engine, session, turn, part, event, tool, and provider traits |
| `ananke-protocol` | DTOs, RFC 9457 errors, event schema, and OpenAPI |
| `ananke-server` | Axum router, HTTP endpoints, middleware, and SSE |
| `ananke-sandbox` | Sandbox capability detection and device profiling |
| `ananke-tools` | Built-in tools and policy enforcement |
| `ananke-store` | SQLite persistence and event log |
| `ananke-provider` | Model provider abstraction and OpenAI-compatible streaming |

Dependency boundary: clients only need `ananke-protocol`; clients do not import the engine directly.

## API

Every API request requires this header:

```http
X-Ananke-Protocol: 1
```

Main endpoints:

| Method | Endpoint | Purpose |
|---|---|---|
| `GET` | `/healthz` | Liveness |
| `GET` | `/readyz` | Readiness |
| `GET` | `/api/meta` | Protocol, capabilities, device profile, and sandbox |
| `GET` | `/api/openapi.json` | OpenAPI schema |
| `GET` | `/api/session` | List sessions |
| `POST` | `/api/session` | Create a session |
| `GET` | `/api/session/{id}/state` | Session state snapshot |
| `GET` | `/api/session/{id}/event` | SSE stream and replay |
| `POST` | `/api/session/{id}/turn` | Create a turn |
| `POST` | `/api/session/{id}/turn/{turnID}/interrupt` | Interrupt a turn |
| `POST` | `/api/session/{id}/turn/{turnID}/steer` | Add guidance during a turn |
| `POST` | `/api/session/{id}/permission/{permissionID}` | Respond to an approval request |

API errors use `application/problem+json` according to RFC 9457.

## Checks

```bash
cargo check --workspace
cargo test --workspace
```

Run server tests only:

```bash
cargo test -p ananke-server
```

Regenerate OpenAPI:

```bash
cargo run -p ananke-protocol -- gen openapi
```

`openapi.json` is the API contract committed with the source code.

## Request flow

```text
1. The client creates a session with POST /api/session.
2. The client opens SSE /api/session/{id}/event.
3. The client submits input with POST /api/session/{id}/turn.
4. The server runs the agent loop and emits part/turn events.
5. If permission is required, the server emits permission.requested.
6. The client responds through the permission POST endpoint.
7. The client reconnects with since or Last-Event-ID when needed.
```

## Development status

- **M0 — Skeleton:** workspace, protocol, router, `/api/meta`, session/turn API, SQLite, provider, built-in tools, device profiling, and sandbox detection.
- **M1 — Turn & approval:** complete agent loop, streaming, permission rules, SSE replay, interrupt/steer, and persistence before tool dispatch.
- **M2 — Multiple clients & plugins:** service discovery, desktop/mobile/web clients, pairing, and plugin host.
- **M3 — Expansion:** ACP/IDE, multi-language SDKs, embedded mode, and sandbox hardening.

Some surfaces and plugins remain on the roadmap and are not included in the default headless binary.

## Security

- Local transport prefers UDS/named pipes.
- Public TCP requires TLS and authentication.
- Provider API keys must never be returned through API responses.
- Mutating tools are subject to permission policies.
- Clients must clearly display `sandbox = "none"`.
- Planned plugins run as separate processes; capabilities are provided through the engine rather than direct syscalls.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — architecture decisions and roadmap.
- [ARCHITECTURE_CONSOLIDATED.md](ARCHITECTURE_CONSOLIDATED.md) — consolidated crate structure and data flow.
- [openapi.json](openapi.json) — API contract.
- [LICENSE](LICENSE) — MIT License.

## License

Ananke is released under the [MIT License](LICENSE).
