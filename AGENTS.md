# AGENTS.md — Quy tắc truy cập mã nguồn

Áp dụng cho mọi agent (và mọi công cụ AI) làm việc trong repo này.

## 1. Nguyên tắc gốc

**Scope-first.** Nhận yêu cầu xong **không được** đọc toàn bộ mã nguồn, không được
grep/glob toàn workspace, không được "khảo sát repo cho chắc". Trước khi đọc hay ghi
bất kỳ file `.rs` nào, phải **xin phép người dùng** và được cấp một danh sách crate
cụ thể.

Ngoài các crate được cấp: **không đọc, không ghi, không sửa**.

## 2. Quy trình bắt buộc cho mỗi yêu cầu

1. **Phân loại yêu cầu** — thuộc crate nào (xem bảng §6). Chỉ dùng những gì có
   trong yêu cầu của người dùng; không suy ra bằng cách đọc code trước.
2. **Đề xuất phạm vi tối thiểu** — nêu crate cần, crate không cần, và lý do một dòng.
   Đây là **gợi ý**, người dùng quyết định. Không được tự quyết, không được ép,
   không được mô tả phạm vi đề xuất như thể đã được duyệt.
3. **Chờ duyệt.** Nếu người dùng không trả lời hoặc trả lời mơ hồ, hỏi lại đúng một
   câu — không tự mở rộng, không "đọc tạm một file cho chắc".
4. **Làm trong phạm vi đã cấp.** Mọi file đọc/ghi phải nằm trong crate được cấp
   (hoặc trong danh sách file lẻ được cấp riêng).

## 3. Mẫu yêu cầu cấp quyền

Dùng đúng dạng ngắn gọn này, kèm một dòng "vì sao":

```
Phạm vi tôi cần:
- crates/ananke-core            (sửa agent loop + context budget)
- crates/ananke-protocol         (thêm field vào event)

Không cần: ananke-server, ananke-store, ananke-tools, ananke-provider, ananke-sandbox.

Duyệt phạm vi này, hay bạn muốn thu hẹp/mở rộng?
```

Nếu yêu cầu có thể giải quyết ở nhiều mức, đưa **2 phương án** (tối thiểu / đầy đủ)
và để người dùng chọn. Không tự chọn phương án rộng.

## 4. Ranh giới "ngoài phạm vi"

Kể cả khi đã được cấp crate, những thứ sau vẫn **ngoài phạm vi** trừ khi được cấp
riêng bằng tên:

- `Cargo.toml` (root), `Cargo.lock`, `.cargo/config.toml`
- `ARCHITECTURE.md`, `ARCHITECTURE_CONSOLIDATED.md`, `M1_IMPLEMENTATION_PLAN.md`
- `openapi.json`
- `.freebuff/`, `target/`, `.screenshots/`, `graphify-out/`
- Mọi crate không có trong danh sách được cấp

`Cargo.toml` **của crate trong phạm vi** thì được đọc/ghi (vì cần thêm dep hoặc feature
cho chính crate đó).

## 5. Khi cần mở rộng giữa chừng

Gặp lỗi biên dịch, thiếu kiểu, hoặc cần đọc một định nghĩa nằm ở crate khác:

1. **Dừng.** Không tự mở file ngoài phạm vi để "xem thử".
2. Báo cụ thể: `file:dòng` và lý do cần.
3. Xin mở rộng đúng mức tối thiểu — thường chỉ 1 file, không phải cả crate.

Ví dụ hợp lệ:

> `crates/ananke-tools/src/bash.rs:41` cần chữ ký `ToolResult` trong
> `crates/ananke-core/src/tool.rs:12`. Cho tôi đọc riêng file đó (99 dòng)?

## 6. Gợi ý phạm vi theo loại yêu cầu

Dựa trên đồ thị phụ thuộc thật: `ananke-provider`, `ananke-store`, `ananke-tools` chỉ
phụ thuộc `ananke-core`; `ananke-protocol` độc lập; `ananke-server` là composition
root duy nhất phụ thuộc nhiều crate.

| Yêu cầu | Phạm vi tối thiểu |
|---|---|
| Agent loop, context/compaction, dispatch | `ananke-core` |
| Thêm/sửa tool cụ thể | `ananke-tools` (+ `ananke-core` nếu phải đổi trait `Tool`) |
| Kiểu event, DTO, OpenAPI schema | `ananke-protocol` (+ `ananke-server` nếu đổi route) |
| Route HTTP, SSE, auth, wiring | `ananke-server` (+ `ananke-protocol`) |
| Provider/model adapter | `ananke-provider` |
| Schema, migration, persistence | `ananke-store` |
| Sandbox detect, device profile theo RAM | `ananke-sandbox` |
| Thêm crate mới / đổi cấu trúc workspace | cần cấp root `Cargo.toml` + duyệt rõ ràng |

Nếu một yêu cầu chạm từ 4 crate trở lên, **nói rõ với người dùng là phạm vi lớn** và
đề nghị tách thành nhiều lượt. Không mặc định coi đó là bình thường.

## 7. Đọc tiết kiệm trong phạm vi

Đã được cấp crate **không** có nghĩa là được đọc hết crate đó.

- Tìm trước, đọc sau: `code_search`/`grep` để lấy `file:dòng`, rồi đọc **cửa sổ
  vài chục dòng** quanh đó.
- Không đọc toàn file > 400 dòng khi chưa biết cần chỗ nào; không đọc cả crate
  "để hiểu bối cảnh".
- Không đọc lại file vừa ghi để xác nhận nội dung.
- Khi báo cáo, dẫn `file:dòng` thay vì dán lại code.

## 8. Build/test trong phạm vi

Chỉ sửa một crate thì kiểm tra bằng `cargo check -p <crate>` / `cargo test -p <crate>`.
Không chạy `cargo build --workspace` / `--all-targets` khi chưa được cấp toàn workspace —
lệnh đó đọc và biên dịch mọi crate.

## 9. Ghi nhận phạm vi trong câu trả lời

Mỗi câu trả lời có sửa code phải mở đầu bằng một dòng:

```
Phạm vi đã dùng: ananke-core (được duyệt ở lượt này)
```

Nếu đã xin mở rộng giữa chừng, ghi cả phần mở rộng và lý do.

## 10. Ngoại lệ

Chỉ những trường hợp sau được bỏ qua §2, và **vẫn phải xác nhận trước**:

- Người dùng nói rõ "đọc toàn bộ", "khảo sát cả repo", "audit toàn dự án".
- Yêu cầu là chẩn đoán lỗi liên crate mà người dùng đã mô tả là như vậy.

Không có ngoại lệ nào cho việc agent tự quyết định mở rộng phạm vi.

## 11. Dùng graphify khi có thể

Repo này có graph tri thức cục bộ do `graphify` dựng (`graphify-out/graph.json`, AST
bằng tree-sitter, deterministic, không cần API key). **Câu hỏi về cấu trúc thì hỏi graph
trước, đừng grep.**

- Ai gọi ai / kiểu này ở đâu / hai module có liên hệ không:
  `graphify explain "X"`, `graphify path "A" "B"`, `graphify query "..."`.
- Hub kiến trúc (thứ mọi thứ đi qua): `graphify god-nodes`.
- Cần một mục trong tài liệu dài (§4): `graphify section "ARCHITECTURE.md#AD-20"`
  thay vì đọc cả file.
- Quan hệ crate→crate: `graphify path "ananke-tools" "ananke-store"` (phải rỗng —
  xem §6).
- Trước khi sửa code, kiểm tra graph có tươi không: `graphify check-update .`.

**Giới hạn.** Graph trả lời câu hỏi *về* code; nó **không** cấp quyền đọc file ngoài
phạm vi (§1, §4). Trả lời từ graph là hợp lệ; mở file của crate chưa được cấp thì vẫn
phải xin phép.

**Graph phải được cập nhật.** Sau khi sửa code: `graphify update .`. Repo không có git
hook tự động, nên graph cũ sẽ chỉ sai `file:dòng` — tệ hơn grep. Sửa code mà không
update thì đừng dùng graph để dẫn đường.

**`graphify-out/` là artifact**, nằm ngoài phạm vi đọc/ghi theo §4 trừ khi được cấp riêng.

**Phần semantic** (đặt tên community, `--mode deep`, docs/PDF) mới cần LLM; cấu hình
qua `OPENAI_BASE_URL`, `OPENAI_API_KEY`, `OPENAI_MODEL`. Trích code thuần AST thì không
cần. Không đưa key vào file trong repo.
