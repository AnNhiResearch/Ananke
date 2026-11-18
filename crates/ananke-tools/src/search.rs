//! Tool `grep` (tìm theo regex) và `glob` (liệt kê theo mẫu đường dẫn).
//!
//! Hai tool này chỉ đọc nên không cần permission ghi. Việc duyệt cây thư mục
//! chạy trong `spawn_blocking` để không chặn executor, và có trần số file/dung
//! lượng để giữ RSS thấp trên thiết bị 1 GB RAM (AD-19).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::RegexBuilder;
use serde_json::{json, Value};

use ananke_core::{Tool, ToolOutput};

use crate::policy::ToolPolicy;
use crate::util::{display_path, opt_bool, opt_str, opt_u64, require_str};
use crate::walk::walk_files;

const MAX_WALK_FILES: usize = 20_000;
const MAX_SCAN_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_RESULTS_LIMIT: usize = 2_000;
const MAX_LINE_CHARS: usize = 400;

// ---------------------------------------------------------------------------
// grep
// ---------------------------------------------------------------------------

pub struct GrepTool {
    policy: Arc<ToolPolicy>,
}

impl GrepTool {
    pub fn new(policy: Arc<ToolPolicy>) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Tìm kiếm nội dung file bằng biểu thức chính quy. Có thể giới hạn theo \
         `glob` (ví dụ `*.rs`) và theo thư mục con. Bỏ qua thư mục nặng như \
         `.git`, `node_modules`, `target`."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Biểu thức chính quy cần tìm."
                },
                "path": {
                    "type": "string",
                    "description": "File hoặc thư mục cần tìm. Mặc định là workspace."
                },
                "glob": {
                    "type": "string",
                    "description": "Chỉ tìm trong file khớp mẫu, ví dụ `*.rs`."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Bỏ qua phân biệt hoa thường. Mặc định false."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Số dòng khớp tối đa. Mặc định 200."
                }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, arguments: Value) -> ToolOutput {
        let pattern = match require_str(&arguments, "pattern") {
            Ok(pattern) => pattern,
            Err(error) => return error,
        };
        let case_insensitive = opt_bool(&arguments, "case_insensitive").unwrap_or(false);
        let regex = match RegexBuilder::new(&pattern)
            .case_insensitive(case_insensitive)
            .build()
        {
            Ok(regex) => regex,
            Err(error) => return ToolOutput::error(format!("regex không hợp lệ: {error}")),
        };

        let base = match opt_str(&arguments, "path") {
            Some(raw) => match self.policy.resolve_read(&raw) {
                Ok(path) => path,
                Err(error) => return ToolOutput::error(error.to_string()),
            },
            None => self.policy.base().to_path_buf(),
        };
        let filter = match opt_str(&arguments, "glob") {
            Some(pattern) => match GlobMatcher::new(&pattern) {
                Ok(matcher) => Some(matcher),
                Err(message) => return ToolOutput::error(message),
            },
            None => None,
        };
        let max = opt_u64(&arguments, "max_results")
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .clamp(1, MAX_RESULTS_LIMIT as u64) as usize;
        let display_base = self.policy.base().to_path_buf();

        let job = tokio::task::spawn_blocking(move || {
            grep_sync(&base, &regex, filter.as_ref(), max, &display_base)
        });
        match job.await {
            Err(error) => ToolOutput::error(format!("tác vụ tìm kiếm bị huỷ: {error}")),
            Ok(Err(message)) => ToolOutput::error(message),
            Ok(Ok(text)) => ToolOutput::ok(text),
        }
    }
}

fn grep_sync(
    base: &Path,
    regex: &regex::Regex,
    filter: Option<&GlobMatcher>,
    max: usize,
    display_root: &Path,
) -> Result<String, String> {
    let files = candidates(base);
    let mut hits: Vec<String> = Vec::new();
    let mut reached_limit = false;

    'scan: for file in files {
        if let Some(filter) = filter {
            if !filter.is_match(&file, base) {
                continue;
            }
        }
        let metadata = match std::fs::metadata(&file) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.len() > MAX_SCAN_BYTES {
            continue;
        }
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if bytes.iter().take(512).any(|byte| *byte == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (index, line) in text.lines().enumerate() {
            if regex.is_match(line) {
                hits.push(format!(
                    "{}:{}: {}",
                    display_path(&file, display_root),
                    index + 1,
                    trim_line(line)
                ));
                if hits.len() >= max {
                    reached_limit = true;
                    break 'scan;
                }
            }
        }
    }

    if hits.is_empty() {
        return Ok("không tìm thấy kết quả phù hợp".to_owned());
    }
    let mut output = hits.join("\n");
    if reached_limit {
        output.push_str("\n… (đã đạt giới hạn max_results)");
    }
    Ok(output)
}

// ---------------------------------------------------------------------------
// glob
// ---------------------------------------------------------------------------

pub struct GlobTool {
    policy: Arc<ToolPolicy>,
}

impl GlobTool {
    pub fn new(policy: Arc<ToolPolicy>) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Liệt kê file khớp mẫu glob, ví dụ `src/**/*.rs` hoặc `*.toml`. \
         Mặc định tìm trong toàn workspace."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Mẫu glob cần khớp."
                },
                "path": {
                    "type": "string",
                    "description": "Thư mục bắt đầu tìm. Mặc định là workspace."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Số đường dẫn tối đa. Mặc định 200."
                }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, arguments: Value) -> ToolOutput {
        let pattern = match require_str(&arguments, "pattern") {
            Ok(pattern) => pattern,
            Err(error) => return error,
        };
        let matcher = match GlobMatcher::new(&pattern) {
            Ok(matcher) => matcher,
            Err(message) => return ToolOutput::error(message),
        };
        let base = match opt_str(&arguments, "path") {
            Some(raw) => match self.policy.resolve_read(&raw) {
                Ok(path) => path,
                Err(error) => return ToolOutput::error(error.to_string()),
            },
            None => self.policy.base().to_path_buf(),
        };
        let max = opt_u64(&arguments, "max_results")
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .clamp(1, MAX_RESULTS_LIMIT as u64) as usize;
        let display_base = self.policy.base().to_path_buf();

        let job = tokio::task::spawn_blocking(move || {
            glob_sync(&base, &matcher, max, &display_base)
        });
        match job.await {
            Err(error) => ToolOutput::error(format!("tác vụ tìm kiếm bị huỷ: {error}")),
            Ok(result) => ToolOutput::ok(result),
        }
    }
}

fn glob_sync(base: &Path, matcher: &GlobMatcher, max: usize, display_root: &Path) -> String {
    let mut matches: Vec<String> = candidates(base)
        .into_iter()
        .filter(|file| matcher.is_match(file, base))
        .map(|file| display_path(&file, display_root))
        .collect();
    matches.sort();

    if matches.is_empty() {
        return "không tìm thấy file khớp pattern".to_owned();
    }
    let reached_limit = matches.len() > max;
    matches.truncate(max);
    let mut output = matches.join("\n");
    if reached_limit {
        output.push_str("\n… (đã đạt giới hạn max_results)");
    }
    output
}

// ---------------------------------------------------------------------------
// dùng chung
// ---------------------------------------------------------------------------

/// Danh sách file ứng viên: một file cụ thể, hoặc duyệt cây thư mục.
fn candidates(base: &Path) -> Vec<PathBuf> {
    if base.is_file() {
        return vec![base.to_path_buf()];
    }
    walk_files(base, MAX_WALK_FILES)
}

fn trim_line(line: &str) -> String {
    let line = line.trim_end();
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_owned();
    }
    let cut: String = line.chars().take(MAX_LINE_CHARS).collect();
    format!("{cut}…")
}

/// Bọc `globset` để mẫu không có dấu `/` được so với tên file (thói quen của
/// `*.rs`) còn mẫu có `/` được so với đường dẫn tương đối.
pub(crate) struct GlobMatcher {
    set: GlobSet,
    match_file_name: bool,
}

impl GlobMatcher {
    pub(crate) fn new(pattern: &str) -> Result<Self, String> {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|error| format!("glob không hợp lệ `{pattern}`: {error}"))?;
        let mut builder = GlobSetBuilder::new();
        builder.add(glob);
        let set = builder
            .build()
            .map_err(|error| format!("glob không hợp lệ `{pattern}`: {error}"))?;
        Ok(Self {
            set,
            match_file_name: !pattern.contains('/'),
        })
    }

    fn is_match(&self, path: &Path, base: &Path) -> bool {
        if self.match_file_name {
            if let Some(name) = path.file_name() {
                if self.set.is_match(name) {
                    return true;
                }
            }
        }
        let relative = path.strip_prefix(base).unwrap_or(path);
        self.set.is_match(relative)
    }
}