# Kế hoạch & Theo dõi triển khai TUI (Dựa trên kiến trúc Pi TUI)

> **Mục tiêu**: Nâng cấp `packages/tui` từ giao diện dòng lệnh `readline` đơn giản sang **High-Performance Differential Terminal UI** (kế thừa kiến trúc từ `pi-main/packages/tui`), giao tiếp độc lập với `ananke-server` qua HTTP REST và SSE (`X-Ananke-Protocol: 1`).

---

## 1. Trạng thái tổng quan

- [x] **Giai đoạn 0: Phân tích kiến trúc Pi TUI**
  - Cơ chế Synchronized Output (`CSI ? 2026 h/l`) chống nhấp nháy (zero flicker).
  - Differential rendering (chỉ render lại các dòng thay đổi, bảo toàn terminal scrollback).
  - Main screen vs Alternate screen buffer.
  - Component hierarchy: Terminal -> Tui -> Editor / Markdown / Loader / Modal.

- [x] **Giai đoạn 1: Nền tảng Terminal & Differential Engine (`packages/tui/src/tui-core/`)**
  - [x] Terminal abstraction: `process.stdin` / `process.stdout` raw mode, đo đạc kích thước terminal, xử lý `SIGWINCH` (`terminal.ts`).
  - [x] ANSI formatting & Color utilities: strip ANSI, đo visual width (hỗ trợ full-width/CJK/emoji), highlight styles (`colors.ts`).
  - [x] Differential renderer: Buffer so sánh dòng, atomic screen updates (`differential.ts`).
  - [x] Input loop: Dispatch phím, xử lý escape sequences, paste bracketed blocks (`keys.ts`).

- [x] **Giai đoạn 2: Bộ UI Components chuyên dụng**
  - [x] `Editor`: Input nhiều dòng, cursor navigation, phím tắt (Enter gửi, Shift+Enter xuống dòng, history) (`components/editor.ts`).
  - [x] `StreamingMarkdown`: Render markdown mượt theo từng token, syntax code block, inline formatting (`components/markdown.ts`).
  - [x] `Spinner / Loader`: Chỉ báo trạng thái model suy nghĩ / đang thực thi tool kèm elapsed time (`components/status.ts`).
  - [x] `ApprovalDialog`: Hộp thoại tương tác nổi bật khi nhận event `permission.requested` (phím chọn Allow / Deny / Always) (`components/approval.ts`).
  - [x] `Header & StatusBar`: Thanh thông tin session, model đang chọn, cảnh báo sandbox và permission mode (`components/status.ts`).

- [x] **Giai đoạn 3: Kết nối Runtime SSE + Slash Commands**
  - [x] Đăng ký lắng nghe SSE (`part.delta`, `turn.started`, `turn.completed`, `turn.failed`, `permission.requested`).
  - [x] Xử lý ngắt kết nối & tự động reconnect SSE với `since`/`Last-Event-ID`.
  - [x] Phím tắt ngắt lượt xử lý: `/interrupt` gọi endpoint `POST /turn/{id}/interrupt`.
  - [x] Hỗ trợ slash commands:
    - [x] `/help`: Hiển thị bảng trợ giúp.
    - [x] `/clear`: Xóa màn hình tin nhắn hiện tại.
    - [x] `/interrupt`: Dừng lượt suy nghĩ / tool hiện hành.
    - [x] `/model <tên>`: Đổi model runtime cho session hiện tại.
    - [x] `/perm <chế_độ>`: Thay đổi `permission_mode` trực tiếp (`ReadOnly`, `WorkspaceWrite`, `DangerFullAccess`).
    - [x] `/exit` hoặc `Ctrl+C`: Thoát giao diện TUI an toàn.

- [x] **Giai đoạn 4: Kiểm thử & Đóng gói**
  - [x] Kiểm thử hiển thị trên Windows Terminal, PowerShell, Linux/macOS terminal (9 unit tests pass).
  - [x] Biên dịch TypeScript (`tsc`), đảm bảo 0 lỗi type.
  - [x] Cập nhật tài liệu hướng dẫn sử dụng trong `packages/tui/README.md`.

---

## 2. Nhật ký tiến độ chi tiết

| Ngày | Hạng mục | Chi tiết thực hiện | Trạng thái |
|---|---|---|---|
| 2026-11-19 | Khởi tạo tài liệu | Phân tích `pi-main/packages/tui` và lập kế hoạch theo dõi | Đã hoàn thành |
| 2026-11-19 | Terminal Abstraction | Triển khai `ProcessTerminal` raw mode, bracketed paste, đo size & resize event | Đã hoàn thành |
| 2026-11-19 | Colors & Visual Width | Triển khai bảng màu ANSI, truecolor RGB, `stripAnsi`, đo visual width CJK/Emoji | Đã hoàn thành |
| 2026-11-19 | Differential Renderer | Triển khai buffer dòng, atomic flush qua Synchronized Output (`differential.ts`) | Đã hoàn thành |
| 2026-11-19 | Input Parser & Keys | Parse ANSI escape sequences, mouse/keys, bracketed paste (`keys.ts`) | Đã hoàn thành |
| 2026-11-19 | UI Components Suite | Hoàn thành Editor, StreamingMarkdown, Spinner, StatusBar, ApprovalDialog | Đã hoàn thành |
| 2026-11-19 | Runtime SSE & Commands | Kết nối SSE reconnect, modal Approval, full slash commands (`/model`, `/perm`, `/interrupt`) | Đã hoàn thành |
| 2026-11-19 | Testing & Packaging | 9/9 unit tests pass, TypeScript 0 lỗi type, hoàn tất README.md tài liệu hướng dẫn | Đã hoàn thành |

---

## 3. Quy tắc kiến trúc (Bắt buộc tuân thủ)
1. **Độc lập tuyệt đối**: `packages/tui` chạy trên Bun/Node.js, chỉ giao tiếp với server qua HTTP REST (`http://127.0.0.1:9876`) và SSE, không nhúng hoặc liên kết trực tiếp vào crate Rust.
2. **Khả năng phản hồi tức thì**: Không block event loop khi model streaming; xử lý phím ngắt (`Escape`/`Ctrl+C`) lập tức gửi `POST /interrupt` tới server.
3. **Hiển thị sandbox rõ ràng**: Luôn hiển thị cảnh báo nếu `sandbox = "none"` theo quy tắc bảo mật AD-10.
