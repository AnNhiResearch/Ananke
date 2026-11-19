//! Tool `bash`: chạy một lệnh shell trong workspace.
//!
//! `mutating() == true` nên engine phải hỏi permission trước khi gọi (§7.3).
//! Timeout mặc định ngắn để một lệnh treo không giữ session mãi; tiến trình con
//! bị kill khi future bị huỷ nhờ `kill_on_drop`.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use ananke_core::{Tool, ToolOutput};

use crate::policy::ToolPolicy;
use crate::util::{opt_u64, require_str, truncate_text};

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const DEFAULT_MAX_OUTPUT: usize = 64_000;

pub struct BashTool {
    policy: Arc<ToolPolicy>,
    timeout: Duration,
    max_output: usize,
}

impl BashTool {
    pub fn new(policy: Arc<ToolPolicy>) -> Self {
        Self {
            policy,
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
            max_output: DEFAULT_MAX_OUTPUT,
        }
    }

    /// Đặt trần timeout và trần dung lượng output (dùng cho thiết bị 1 GB RAM).
    pub fn with_limits(mut self, timeout: Duration, max_output: usize) -> Self {
        self.timeout = timeout;
        self.max_output = max_output;
        self
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Chạy một lệnh shell trong workspace và trả về stdout/stderr. \
         Dùng cho build, test, git, liệt kê file. Lệnh chạy quá timeout sẽ bị huỷ."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Lệnh shell cần chạy, ví dụ `cargo test`."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout riêng cho lệnh này (mili giây). Mặc định 60000, tối đa 600000."
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn mutating(&self) -> bool {
        true
    }

    async fn execute(&self, arguments: Value) -> ToolOutput {
        let command = match require_str(&arguments, "command") {
            Ok(command) => command,
            Err(error) => return error,
        };
        if !self.policy.allows_shell() {
            return ToolOutput::error(format!(
                "policy `{}` không cho phép chạy lệnh shell",
                self.policy.mode().as_str()
            ));
        }

        if command.to_ascii_lowercase().contains("models.json") {
            return ToolOutput::error(
                "chính sách sandbox nghiêm cấm truy cập hoặc thao tác trên file `models.json` qua shell"
                    .to_string(),
            );
        }

        let requested = opt_u64(&arguments, "timeout_ms")
            .map(Duration::from_millis)
            .unwrap_or(self.timeout);
        let limit = requested.min(Duration::from_millis(MAX_TIMEOUT_MS));

        let mut process = shell_command(&command);
        process
            .current_dir(self.policy.base())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .env("PAGER", "cat")
            .env("GIT_PAGER", "cat");

        let run = async {
            let child = process
                .spawn()
                .map_err(|error| format!("không khởi chạy được lệnh: {error}"))?;
            child
                .wait_with_output()
                .await
                .map_err(|error| format!("lỗi chờ lệnh: {error}"))
        };

        let output = match tokio::time::timeout(limit, run).await {
            Err(_) => {
                return ToolOutput::error(format!(
                    "lệnh vượt quá timeout {} ms và đã bị huỷ",
                    limit.as_millis()
                ))
            }
            Ok(Err(message)) => return ToolOutput::error(message),
            Ok(Ok(output)) => output,
        };

        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.stderr.is_empty() {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("[stderr]\n");
            text.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        let text = truncate_text(&text, self.max_output);

        if output.status.success() {
            if text.trim().is_empty() {
                ToolOutput::ok("(lệnh thành công, không có output)")
            } else {
                ToolOutput::ok(text)
            }
        } else {
            let reason = match output.status.code() {
                Some(code) => format!("lệnh thất bại (mã thoát {code})"),
                None => "lệnh bị kết thúc bởi signal".to_owned(),
            };
            ToolOutput::error(format!("{reason}\n{text}"))
        }
    }
}

fn shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut process = Command::new("cmd");
        process.arg("/C").arg(command);
        process
    } else {
        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        process
    }
}