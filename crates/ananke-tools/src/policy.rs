//! Chính sách thực thi của tool (`ARCHITECTURE.md` §7.2).
//!
//! Bốn chế độ trong spec là `read-only`, `workspace-write`, `danger-full-access`
//! và `external`. `external` do sandbox bên ngoài (bubblewrap, container…) đảm
//! nhận nên crate này không dựng lại; ở đây chỉ có ba chế độ nội bộ.
//!
//! Giới hạn cần nhớ: trên Android/Termux kernel không cho phép dựng namespace
//! tin cậy, nên policy này chỉ chặn ở mức API (đường dẫn ghi, lệnh shell). Một
//! tiến trình con vẫn có thể vượt rào — vì vậy `/api/meta` phải công bố
//! `sandbox = "none"` và client phải cảnh báo (AD-19).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// Mức quyền mà `ananke-tools` tự áp trong tiến trình.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyMode {
    /// Chỉ đọc; mọi tool thay đổi trạng thái bị từ chối.
    ReadOnly,
    /// Đọc tự do, chỉ được ghi trong `writable_roots`.
    WorkspaceWrite,
    /// Không giới hạn đường dẫn (vẫn do permission rules §7.3 canh).
    DangerFullAccess,
}

impl PolicyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PolicyMode::ReadOnly => "read-only",
            PolicyMode::WorkspaceWrite => "workspace-write",
            PolicyMode::DangerFullAccess => "danger-full-access",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("policy `read-only` không cho phép ghi file hoặc chạy lệnh")]
    ReadOnly,
    #[error("đường dẫn `{}` nằm ngoài writable_roots", .0.display())]
    OutsideRoot(PathBuf),
    #[error("đường dẫn `{}` không hợp lệ", .0.display())]
    Invalid(PathBuf),
}

/// Policy bất biến, chia sẻ giữa các tool bằng `Arc`.
///
/// `base` là thư mục làm việc dùng để hiểu đường dẫn tương đối (thường là
/// workspace của session). `writable_roots` đã được canonicalize lúc tạo để so
/// khớp với đường dẫn đã resolve.
#[derive(Clone, Debug)]
pub struct ToolPolicy {
    mode: PolicyMode,
    base: PathBuf,
    writable_roots: Vec<PathBuf>,
    network: bool,
}

impl ToolPolicy {
    pub fn read_only(base: impl Into<PathBuf>) -> Self {
        Self {
            mode: PolicyMode::ReadOnly,
            base: base.into(),
            writable_roots: Vec::new(),
            network: false,
        }
    }

    pub fn workspace_write(
        base: impl Into<PathBuf>,
        writable_roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    ) -> Self {
        let base = base.into();
        let roots = writable_roots
            .into_iter()
            .map(|root| {
                let root: PathBuf = root.into();
                canonicalize_best_effort(&absolute(&base, &root))
            })
            .collect();
        Self {
            mode: PolicyMode::WorkspaceWrite,
            base,
            writable_roots: roots,
            network: false,
        }
    }

    pub fn danger_full_access(base: impl Into<PathBuf>) -> Self {
        Self {
            mode: PolicyMode::DangerFullAccess,
            base: base.into(),
            writable_roots: Vec::new(),
            network: true,
        }
    }

    /// Bật cờ mạng (chỉ có ý nghĩa khai báo; crate này không chặn được socket).
    pub fn with_network(mut self, network: bool) -> Self {
        self.network = network;
        self
    }

    pub fn mode(&self) -> PolicyMode {
        self.mode
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn writable_roots(&self) -> &[PathBuf] {
        &self.writable_roots
    }

    /// Policy có cho phép thay đổi trạng thái (ghi file, chạy lệnh) hay không.
    pub fn allows_mutation(&self) -> bool {
        self.mode != PolicyMode::ReadOnly
    }

    /// Chạy shell là hành vi thay đổi trạng thái nên `read-only` từ chối.
    pub fn allows_shell(&self) -> bool {
        self.allows_mutation()
    }

    pub fn allows_network(&self) -> bool {
        self.network || self.mode == PolicyMode::DangerFullAccess
    }

    /// Resolve đường dẫn đọc. Đọc không bị giới hạn theo root; permission rules
    /// (§7.3) chịu trách nhiệm chặn vùng nhạy cảm.
    pub fn resolve_read(&self, raw: &str) -> Result<PathBuf, PolicyError> {
        let abs = absolute(&self.base, Path::new(raw));
        let normalized = normalize(&abs).ok_or_else(|| PolicyError::Invalid(abs.clone()))?;
        Ok(canonicalize_best_effort(&normalized))
    }

    /// Resolve đường dẫn ghi: chuẩn hoá, giải symlink của phần đã tồn tại, rồi
    /// kiểm tra nằm trong `writable_roots`.
    pub fn resolve_write(&self, raw: &str) -> Result<PathBuf, PolicyError> {
        if !self.allows_mutation() {
            return Err(PolicyError::ReadOnly);
        }
        let abs = absolute(&self.base, Path::new(raw));
        let normalized = normalize(&abs).ok_or_else(|| PolicyError::Invalid(abs.clone()))?;
        let resolved = canonicalize_best_effort(&normalized);
        if self.mode == PolicyMode::WorkspaceWrite
            && !self
                .writable_roots
                .iter()
                .any(|root| resolved.starts_with(root))
        {
            return Err(PolicyError::OutsideRoot(resolved));
        }
        Ok(resolved)
    }
}

fn absolute(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Giải `.`/`..` thuần từ vựng. Trả `None` nếu `..` vượt quá gốc.
fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(segment) => out.push(segment),
        }
    }
    Some(out)
}

/// Canonicalize phần tồn tại gần nhất rồi nối lại phần chưa tồn tại.
///
/// Cần thiết vì `std::fs::canonicalize` đòi hỏi cả đường dẫn phải tồn tại, trong
/// khi tool `write` phải ghi được file mới.
fn canonicalize_best_effort(path: &Path) -> PathBuf {
    let mut rest: Vec<OsString> = Vec::new();
    let mut probe = path.to_path_buf();
    loop {
        if let Ok(canonical) = probe.canonicalize() {
            let mut result = canonical;
            for segment in rest.iter().rev() {
                result.push(segment);
            }
            return result;
        }
        let name = match probe.file_name() {
            Some(name) => name.to_os_string(),
            None => return path.to_path_buf(),
        };
        rest.push(name);
        if !probe.pop() {
            return path.to_path_buf();
        }
    }
}