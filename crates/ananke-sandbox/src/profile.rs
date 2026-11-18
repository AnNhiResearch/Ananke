//! Profile thiết bị theo RAM (AD-15) và đọc `MemTotal`.

use serde::{Deserialize, Serialize};

const GIB: u64 = 1024 * 1024 * 1024;

/// Profile chọn theo `MemTotal` để đặt ngân sách tài nguyên mặc định (AD-15).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceProfile {
    /// `< 1.5 GiB`: tắt parallel subagent, LSP nền, watch file, TUI full.
    Constrained,
    /// `< 4 GiB`.
    Small,
    /// Còn lại.
    Default,
}

impl DeviceProfile {
    /// Ngưỡng đúng AD-15: `< 1.5 GB` → `constrained`, `< 4 GB` → `small`.
    pub fn from_mem_total(bytes: u64) -> Self {
        if bytes < GIB * 3 / 2 {
            Self::Constrained
        } else if bytes < GIB * 4 {
            Self::Small
        } else {
            Self::Default
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Constrained => "constrained",
            Self::Small => "small",
            Self::Default => "default",
        }
    }
}

/// `MemTotal` (byte) của máy. Chỉ đọc được trên Linux/Android — nơi AD-19
/// thực sự cần; nền tảng khác trả `None`.
pub fn mem_total_bytes() -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|raw| parse_mem_total(&raw))
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        None
    }
}

/// Profile hiệu lực lúc start. Config có thể ghi đè ở tầng server (AD-15).
pub fn device_profile() -> DeviceProfile {
    match mem_total_bytes() {
        Some(bytes) => DeviceProfile::from_mem_total(bytes),
        // Không đo được RAM: chọn profile rộng rãi thay vì đoán thấp.
        None => DeviceProfile::Default,
    }
}

#[cfg(any(target_os = "linux", target_os = "android", test))]
fn parse_mem_total(meminfo: &str) -> Option<u64> {
    meminfo.lines().find_map(|line| {
        let rest = line.strip_prefix("MemTotal:")?;
        let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        Some(kb * 1024)
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_mem_total, DeviceProfile, GIB};

    #[test]
    fn parse_mem_total_tu_dong_meminfo() {
        let meminfo = "MemFree:  100 kB\nMemTotal:       995832 kB\nBuffers: 1 kB\n";
        assert_eq!(parse_mem_total(meminfo), Some(995_832 * 1024));
        assert_eq!(parse_mem_total("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn nguong_profile_dung_ad_15() {
        assert_eq!(DeviceProfile::from_mem_total(0), DeviceProfile::Constrained);
        assert_eq!(
            DeviceProfile::from_mem_total(GIB),
            DeviceProfile::Constrained
        );
        assert_eq!(
            DeviceProfile::from_mem_total(GIB * 3 / 2),
            DeviceProfile::Small
        );
        assert_eq!(
            DeviceProfile::from_mem_total(3 * GIB),
            DeviceProfile::Small
        );
        assert_eq!(
            DeviceProfile::from_mem_total(4 * GIB),
            DeviceProfile::Default
        );
        assert_eq!(
            DeviceProfile::from_mem_total(16 * GIB),
            DeviceProfile::Default
        );
    }
}