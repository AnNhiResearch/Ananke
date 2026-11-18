//! Đo năng lực sandbox theo nền tảng (ARCHITECTURE §7.2).
//!
//! Mỗi nhánh `cfg` chỉ chạy trên đúng nền tảng của nó; phần phân loại Linux
//! được tách khỏi syscall để test được trên mọi máy build.

use crate::SandboxReport;

/// Đo năng lực sandbox của máy đang chạy.
pub fn detect() -> SandboxReport {
    #[cfg(target_os = "android")]
    {
        SandboxReport::none(
            "Android/Termux không có Landlock và không cho user namespace; permission rules là hàng rào duy nhất",
        )
    }

    #[cfg(target_os = "linux")]
    {
        linux::detect()
    }

    #[cfg(target_os = "macos")]
    {
        SandboxReport::available(
            crate::SandboxKind::Seatbelt,
            vec!["seatbelt".to_owned()],
            None,
        )
    }

    #[cfg(windows)]
    {
        SandboxReport::available(
            crate::SandboxKind::RestrictedToken,
            vec!["restricted-token".to_owned()],
            None,
        )
    }

    #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "macos", windows)))]
    {
        SandboxReport::none(format!(
            "nền tảng `{}` chưa có cơ chế sandbox được hỗ trợ",
            std::env::consts::OS
        ))
    }
}

#[cfg(any(target_os = "linux", test))]
mod linux {
    use crate::{SandboxKind, SandboxReport};

    /// Landlock có từ kernel 5.13.
    const MIN_LANDLOCK: (u32, u32) = (5, 13);

    /// Dữ liệu đo được, tách khỏi syscall để test được trên mọi nền tảng.
    #[derive(Clone, Debug)]
    pub(crate) struct LinuxProbe {
        pub kernel: Option<(u32, u32)>,
        /// `None` khi không đọc được `/sys/kernel/security/lsm`.
        pub landlock: Option<bool>,
        pub seccomp: bool,
        pub bwrap: bool,
        pub termux: bool,
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn detect() -> SandboxReport {
        classify(&LinuxProbe {
            kernel: read_kernel_version(),
            landlock: landlock_listed(),
            seccomp: std::path::Path::new("/proc/sys/kernel/seccomp/actions_avail").exists(),
            bwrap: find_in_path("bwrap").is_some(),
            termux: is_termux(),
        })
    }

    /// Phân loại thuần từ dữ liệu đo — không đụng syscall.
    pub(crate) fn classify(probe: &LinuxProbe) -> SandboxReport {
        if probe.termux {
            return SandboxReport::none(
                "Termux: kernel Android không có Landlock và không cho user namespace",
            );
        }

        let Some(kernel) = probe.kernel else {
            return SandboxReport::none("không đọc được phiên bản kernel");
        };

        if kernel < MIN_LANDLOCK {
            return SandboxReport::none(format!(
                "kernel {}.{} < 5.13 nên không có Landlock",
                kernel.0, kernel.1
            ));
        }

        if probe.landlock == Some(false) {
            return SandboxReport::none("Landlock không nằm trong danh sách LSM đang bật");
        }

        let mut mechanisms = Vec::with_capacity(3);
        if probe.bwrap {
            mechanisms.push("bwrap".to_owned());
        }
        mechanisms.push("landlock".to_owned());
        if probe.seccomp {
            mechanisms.push("seccomp".to_owned());
        }

        let reason = probe
            .bwrap
            .then(|| "có `bwrap`, được ưu tiên thay vì gọi Landlock trực tiếp".to_owned());

        SandboxReport::available(SandboxKind::Landlock, mechanisms, reason)
    }

    /// Đọc `major.minor` từ chuỗi kiểu `6.1.75-android14` hoặc `5.15.0-91-generic`.
    pub(crate) fn parse_kernel_version(raw: &str) -> Option<(u32, u32)> {
        let mut parts = raw.trim().split('.');
        let major = leading_number(parts.next()?)?;
        let minor = leading_number(parts.next()?)?;
        Some((major, minor))
    }

    fn leading_number(part: &str) -> Option<u32> {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            None
        } else {
            digits.parse().ok()
        }
    }

    #[cfg(target_os = "linux")]
    fn read_kernel_version() -> Option<(u32, u32)> {
        let raw = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
        parse_kernel_version(&raw)
    }

    /// `None` khi không đọc được file LSM (container tối giản) — khi đó dựa vào
    /// phiên bản kernel.
    #[cfg(target_os = "linux")]
    fn landlock_listed() -> Option<bool> {
        let lsm = std::fs::read_to_string("/sys/kernel/security/lsm").ok()?;
        if lsm.trim().is_empty() {
            return None;
        }
        Some(lsm.split(',').any(|m| m.trim() == "landlock"))
    }

    #[cfg(target_os = "linux")]
    fn is_termux() -> bool {
        if std::env::var_os("PREFIX")
            .is_some_and(|prefix| prefix.to_string_lossy().contains("com.termux"))
        {
            return true;
        }
        std::path::Path::new("/data/data/com.termux/files/usr").is_dir()
    }

    #[cfg(target_os = "linux")]
    fn find_in_path(bin: &str) -> Option<std::path::PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(bin))
            .find(|candidate| candidate.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::linux::{classify, parse_kernel_version, LinuxProbe};
    use crate::SandboxKind;

    fn probe() -> LinuxProbe {
        LinuxProbe {
            kernel: Some((6, 1)),
            landlock: Some(true),
            seccomp: true,
            bwrap: false,
            termux: false,
        }
    }

    #[test]
    fn parse_kernel_version_doc_dung_chuoi_thuong_gap() {
        assert_eq!(parse_kernel_version("6.1.75-android14\n"), Some((6, 1)));
        assert_eq!(parse_kernel_version("5.15.0-91-generic"), Some((5, 15)));
        assert_eq!(parse_kernel_version("5.13"), Some((5, 13)));
        assert_eq!(parse_kernel_version("không phải số"), None);
    }

    #[test]
    fn termux_luon_la_none() {
        let report = classify(&LinuxProbe {
            termux: true,
            ..probe()
        });
        assert_eq!(report.kind(), SandboxKind::None);
        assert!(report.needs_warning());
        assert!(report.reason().is_some_and(|r| r.contains("Termux")));
    }

    #[test]
    fn kernel_cu_hon_5_13_la_none() {
        let report = classify(&LinuxProbe {
            kernel: Some((5, 10)),
            ..probe()
        });
        assert_eq!(report.kind(), SandboxKind::None);
        assert!(report.reason().is_some_and(|r| r.contains("5.10")));
    }

    #[test]
    fn khong_doc_duoc_kernel_la_none() {
        let report = classify(&LinuxProbe {
            kernel: None,
            ..probe()
        });
        assert_eq!(report.kind(), SandboxKind::None);
    }

    #[test]
    fn kernel_du_5_13_va_co_landlock_tra_ve_landlock() {
        let report = classify(&probe());
        assert_eq!(report.kind(), SandboxKind::Landlock);
        assert!(report.is_enforced());
        assert!(!report.needs_warning());
        assert_eq!(report.mechanisms(), ["landlock", "seccomp"]);
    }

    #[test]
    fn landlock_bi_tat_trong_lsm_la_none() {
        let report = classify(&LinuxProbe {
            landlock: Some(false),
            ..probe()
        });
        assert_eq!(report.kind(), SandboxKind::None);
        assert!(report.reason().is_some_and(|r| r.contains("LSM")));
    }

    #[test]
    fn khong_doc_duoc_lsm_thi_dua_vao_kernel() {
        let report = classify(&LinuxProbe {
            landlock: None,
            ..probe()
        });
        assert_eq!(report.kind(), SandboxKind::Landlock);
    }

    #[test]
    fn bwrap_duoc_uu_tien_trong_danh_sach_co_che() {
        let report = classify(&LinuxProbe {
            bwrap: true,
            ..probe()
        });
        assert_eq!(report.mechanisms().first().map(String::as_str), Some("bwrap"));
        assert!(report.mechanisms().contains(&"landlock".to_owned()));
        assert!(report.reason().is_some_and(|r| r.contains("bwrap")));
    }
}