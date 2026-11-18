//! Duyệt cây thư mục cho `grep`/`glob`.
//!
//! Không đi theo symlink để tránh vòng lặp và tránh đọc ra ngoài workspace.
//! Bỏ qua các thư mục nặng không bao giờ cần cho coding agent.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(crate) const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".ananke",
];

/// Thu tối đa `max_files` file thường dưới `root`, sắp xếp để kết quả ổn định.
pub(crate) fn walk_files(root: &Path, max_files: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if files.len() >= max_files {
                files.sort();
                return files;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                let name = entry.file_name();
                if SKIP_DIRS.iter().any(|skip| name.as_os_str() == OsStr::new(skip)) {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}