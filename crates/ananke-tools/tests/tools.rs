//! Test tích hợp cho 6 tool tích hợp và policy tương ứng.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ananke_core::{Tool, ToolRegistry};
use ananke_tools::{
    register_builtin, BashTool, EditTool, GlobTool, GrepTool, ReadTool, ToolPolicy, WriteTool,
};
use serde_json::json;

fn workspace(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("đồng hồ hệ thống")
        .as_nanos();
    let base = std::env::temp_dir().canonicalize().expect("temp dir");
    let dir = base.join(format!(
        "ananke-tools-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("tạo workspace tạm");
    dir
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn policy(dir: &Path) -> Arc<ToolPolicy> {
    Arc::new(ToolPolicy::workspace_write(dir, [dir]))
}

#[tokio::test]
async fn ghi_doc_roi_sua_va_doc_lai() {
    let dir = workspace("round-trip");
    let policy = policy(&dir);
    let write = WriteTool::new(Arc::clone(&policy));
    let read = ReadTool::new(Arc::clone(&policy));
    let edit = EditTool::new(Arc::clone(&policy));

    let out = write
        .execute(json!({ "path": "src/main.txt", "content": "hello\nworld\n" }))
        .await;
    assert!(out.ok, "{}", out.content);
    assert_eq!(
        std::fs::read_to_string(dir.join("src/main.txt")).unwrap(),
        "hello\nworld\n"
    );

    let out = read.execute(json!({ "path": "src/main.txt" })).await;
    assert!(out.ok, "{}", out.content);
    assert!(out.content.contains("hello"));
    assert!(out.content.contains("world"));
    assert!(out.content.contains("main.txt"));

    let out = edit
        .execute(json!({
            "path": "src/main.txt",
            "old_string": "world",
            "new_string": "ananke"
        }))
        .await;
    assert!(out.ok, "{}", out.content);

    let out = read.execute(json!({ "path": "src/main.txt" })).await;
    assert!(out.content.contains("ananke"));
    assert!(!out.content.contains("world"));

    cleanup(&dir);
}

#[tokio::test]
async fn read_ton_trong_offset_va_limit() {
    let dir = workspace("offset");
    std::fs::write(dir.join("lines.txt"), "a\nb\nc\nd\ne\n").unwrap();
    let tool = ReadTool::new(policy(&dir));

    let out = tool
        .execute(json!({ "path": "lines.txt", "offset": 2, "limit": 2 }))
        .await;
    assert!(out.ok, "{}", out.content);
    assert!(out.content.contains("b"));
    assert!(out.content.contains("c"));
    assert!(!out.content.contains("\na\n"));
    assert!(!out.content.contains("\nd"));

    cleanup(&dir);
}

#[tokio::test]
async fn read_bao_loi_khi_file_khong_ton_tai() {
    let dir = workspace("missing");
    let tool = ReadTool::new(policy(&dir));
    let out = tool.execute(json!({ "path": "khong-co.txt" })).await;
    assert!(!out.ok);
    cleanup(&dir);
}

#[tokio::test]
async fn read_tu_choi_file_nhi_phan() {
    let dir = workspace("binary");
    std::fs::write(dir.join("blob.bin"), [0u8, 1, 2, 0, 3]).unwrap();
    let tool = ReadTool::new(policy(&dir));
    let out = tool.execute(json!({ "path": "blob.bin" })).await;
    assert!(!out.ok);
    assert!(out.content.contains("nhị phân"));
    cleanup(&dir);
}

#[tokio::test]
async fn edit_tu_choi_khi_khong_khop_hoac_khop_nhieu() {
    let dir = workspace("edit-errors");
    std::fs::write(dir.join("a.txt"), "foo foo\n").unwrap();
    let tool = EditTool::new(policy(&dir));

    let out = tool
        .execute(json!({ "path": "a.txt", "old_string": "zzz", "new_string": "bar" }))
        .await;
    assert!(!out.ok);

    let out = tool
        .execute(json!({ "path": "a.txt", "old_string": "foo", "new_string": "bar" }))
        .await;
    assert!(!out.ok);
    assert!(out.content.contains("2 lần"));

    let out = tool
        .execute(json!({
            "path": "a.txt",
            "old_string": "foo",
            "new_string": "bar",
            "replace_all": true
        }))
        .await;
    assert!(out.ok, "{}", out.content);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "bar bar\n");

    cleanup(&dir);
}

#[tokio::test]
async fn policy_read_only_tu_choi_ghi_va_chay_lenh() {
    let dir = workspace("read-only");
    let policy = Arc::new(ToolPolicy::read_only(&dir));

    let out = WriteTool::new(Arc::clone(&policy))
        .execute(json!({ "path": "a.txt", "content": "x" }))
        .await;
    assert!(!out.ok);
    assert!(!dir.join("a.txt").exists());

    let out = BashTool::new(Arc::clone(&policy))
        .execute(json!({ "command": "echo hi" }))
        .await;
    assert!(!out.ok);

    cleanup(&dir);
}

#[tokio::test]
async fn workspace_write_chan_duong_dan_ngoai_root() {
    let dir = workspace("escape");
    let policy = policy(&dir);

    let out = WriteTool::new(Arc::clone(&policy))
        .execute(json!({ "path": "../escape.txt", "content": "x" }))
        .await;
    assert!(!out.ok);
    assert!(out.content.contains("ngoài"), "{}", out.content);

    // Đường dẫn tuyệt đối ngoài root cũng bị chặn.
    let outside = std::env::temp_dir().join(format!("ananke-outside-{}.txt", std::process::id()));
    let out = WriteTool::new(policy)
        .execute(json!({ "path": outside.to_string_lossy(), "content": "x" }))
        .await;
    assert!(!out.ok);
    assert!(!outside.exists());

    cleanup(&dir);
}

#[tokio::test]
async fn bash_tra_ve_output_va_ma_thoat() {
    let dir = workspace("bash");
    let tool = BashTool::new(policy(&dir));

    let out = tool.execute(json!({ "command": "echo ananke" })).await;
    assert!(out.ok, "{}", out.content);
    assert!(out.content.contains("ananke"));

    let out = tool.execute(json!({ "command": "exit 3" })).await;
    assert!(!out.ok);
    assert!(out.content.contains('3'), "{}", out.content);

    cleanup(&dir);
}

#[tokio::test]
async fn grep_tim_theo_regex_va_glob() {
    let dir = workspace("grep");
    std::fs::write(dir.join("a.rs"), "fn main() {}\nlet x = 1;\n").unwrap();
    std::fs::write(dir.join("b.txt"), "fn main() {}\n").unwrap();
    let tool = GrepTool::new(policy(&dir));

    let out = tool
        .execute(json!({ "pattern": "fn main", "glob": "*.rs" }))
        .await;
    assert!(out.ok, "{}", out.content);
    assert!(out.content.contains("a.rs:1"));
    assert!(!out.content.contains("b.txt"));

    let out = tool.execute(json!({ "pattern": "fn main" })).await;
    assert!(out.content.contains("a.rs:1"));
    assert!(out.content.contains("b.txt:1"));

    let out = tool.execute(json!({ "pattern": "khong-bao-gio-khop" })).await;
    assert!(out.ok);
    assert!(out.content.contains("không tìm thấy"));

    cleanup(&dir);
}

#[tokio::test]
async fn glob_loc_theo_pattern() {
    let dir = workspace("glob");
    std::fs::create_dir_all(dir.join("src/nested")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "").unwrap();
    std::fs::write(dir.join("src/nested/lib.rs"), "").unwrap();
    std::fs::write(dir.join("README.md"), "").unwrap();
    let tool = GlobTool::new(policy(&dir));

    let out = tool.execute(json!({ "pattern": "*.rs", "max_results": 10 })).await;
    assert!(out.ok, "{}", out.content);
    assert!(out.content.contains("src/main.rs"));
    assert!(out.content.contains("src/nested/lib.rs"));
    assert!(!out.content.contains("README.md"));

    let out = tool.execute(json!({ "pattern": "src/nested/*.rs" })).await;
    assert!(out.content.contains("src/nested/lib.rs"));
    assert!(!out.content.contains("src/main.rs"));

    cleanup(&dir);
}

#[test]
fn register_builtin_dang_ky_du_sau_tool() {
    let dir = workspace("registry");
    let mut registry = ToolRegistry::new();
    register_builtin(&mut registry, ToolPolicy::workspace_write(&dir, [&dir])).unwrap();

    let names: Vec<&str> = registry.names().collect();
    assert_eq!(names, ["bash", "edit", "glob", "grep", "read", "write"]);
    assert_eq!(registry.len(), 6);

    // Đăng ký lại phải lỗi vì trùng tên.
    assert!(register_builtin(&mut registry, ToolPolicy::read_only(&dir)).is_err());

    cleanup(&dir);
}

#[test]
fn mutating_chi_dung_cho_tool_thay_doi_trang_thai() {
    let dir = workspace("mutating");
    let policy = policy(&dir);

    assert!(BashTool::new(Arc::clone(&policy)).mutating());
    assert!(WriteTool::new(Arc::clone(&policy)).mutating());
    assert!(EditTool::new(Arc::clone(&policy)).mutating());
    assert!(!ReadTool::new(Arc::clone(&policy)).mutating());
    assert!(!GrepTool::new(Arc::clone(&policy)).mutating());
    assert!(!GlobTool::new(policy).mutating());

    cleanup(&dir);
}

#[tokio::test]
async fn sandbox_cam_doc_ghi_va_shell_models_json() {
    let dir = workspace("sandbox_models_protect");
    let policy = policy(&dir);

    let read_tool = ReadTool::new(Arc::clone(&policy));
    let write_tool = WriteTool::new(Arc::clone(&policy));
    let bash_tool = BashTool::new(Arc::clone(&policy));

    // 1. Thử ghi models.json -> bị chặn
    let write_res = write_tool
        .execute(serde_json::json!({
            "path": "models.json",
            "content": "{}"
        }))
        .await;
    assert!(!write_res.ok);
    assert!(write_res.content.contains("chính sách sandbox"));

    // 2. Thử đọc models.json -> bị chặn
    let read_res = read_tool
        .execute(serde_json::json!({
            "path": "models.json"
        }))
        .await;
    assert!(!read_res.ok);
    assert!(read_res.content.contains("chính sách sandbox"));

    // 3. Thử qua shell command nhắc tới models.json -> bị chặn
    let bash_res = bash_tool
        .execute(serde_json::json!({
            "command": "cat models.json"
        }))
        .await;
    assert!(!bash_res.ok);
    assert!(bash_res.content.contains("chính sách sandbox"));

    cleanup(&dir);
}