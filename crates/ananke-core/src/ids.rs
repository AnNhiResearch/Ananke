use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Sinh id có tiền tố, đơn điệu trong một process và đủ ngắn để đọc bằng mắt.
fn generate(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{millis:x}{n:04x}")
}

macro_rules! id_type {
    ($name:ident, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(generate($prefix))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

id_type!(SessionId, "ses", "Định danh session.");
id_type!(TurnId, "trn", "Định danh một turn trong session.");
id_type!(PartId, "prt", "Định danh một part bên trong message/turn.");
id_type!(MessageId, "msg", "Định danh một message.");
id_type!(ToolCallId, "call", "Định danh một lời gọi tool, dùng để ghép call với result.");
id_type!(PermissionId, "perm", "Định danh một yêu cầu approval.");