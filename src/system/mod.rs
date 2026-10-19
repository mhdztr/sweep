//! System context: distro, package manager, root filesystem stats.
//! Spec: plan §2.5. Startup budget <100 ms: three tiny reads, no deps.

use crate::scan::fsinfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistroFamily {
    Arch,
    Debian,
    Fedora,
    Suse,
    Other,
}

impl DistroFamily {
    pub fn as_str(&self) -> &'static str {
        match self {
            DistroFamily::Arch => "arch",
            DistroFamily::Debian => "debian",
            DistroFamily::Fedora => "fedora",
            DistroFamily::Suse => "suse",
            DistroFamily::Other => "other",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemCtx {
    /// os-release ID ("arch"); "unknown" if unreadable.
    pub distro_id: String,
    /// os-release NAME ("Arch Linux"); "unknown".
    pub distro_name: String,
    pub family: DistroFamily,
    /// Arch→Some("pacman"), Debian→Some("apt"), Fedora→Some("dnf"),
    /// Suse→Some("zypper"), Other→None.
    pub pkgman: Option<String>,
    pub hostname: String,
    /// Filesystem type of "/".
    pub fstype: String,
    pub root_total: u64,
    pub root_used: u64,
}

impl SystemCtx {
    /// Reads /etc/os-release + /proc + statvfs only; all failures degrade
    /// (never panic).
    pub fn detect() -> anyhow::Result<Self> {
        let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let id = os_release_key(&os_release, "ID").unwrap_or_else(|| "unknown".to_string());
        let name = os_release_key(&os_release, "NAME").unwrap_or_else(|| "unknown".to_string());
        let id_like = os_release_key(&os_release, "ID_LIKE").unwrap_or_default();
        let family = classify(&id, &id_like);
        let pkgman = match family {
            DistroFamily::Arch => Some("pacman"),
            DistroFamily::Debian => Some("apt"),
            DistroFamily::Fedora => Some("dnf"),
            DistroFamily::Suse => Some("zypper"),
            DistroFamily::Other => None,
        }
        .map(str::to_string);
        let hostname = fsinfo::hostname().unwrap_or_else(|_| "unknown".to_string());
        let fstype = fsinfo::root_fstype().unwrap_or_else(|_| "unknown".to_string());
        let (root_total, root_used) =
            fsinfo::statvfs_totals(std::path::Path::new("/")).unwrap_or((0, 0));
        Ok(Self {
            distro_id: id,
            distro_name: name,
            family,
            pkgman,
            hostname,
            fstype,
            root_total,
            root_used,
        })
    }

    /// Deterministic test constructor (plan §2.5).
    pub fn for_test() -> Self {
        Self {
            distro_id: "arch".to_string(),
            distro_name: "Arch Linux".to_string(),
            family: DistroFamily::Arch,
            pkgman: Some("pacman".to_string()),
            hostname: "testhost".to_string(),
            fstype: "ext4".to_string(),
            root_total: 1_099_511_627_776, // 1 TiB
            root_used: 549_755_813_888,    // 512 GiB
        }
    }
}

fn os_release_key(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(key)
            && let Some(value) = rest.strip_prefix('=')
        {
            let unquoted = value
                .trim_start_matches('"')
                .trim_end_matches('"')
                .trim_start_matches('\'')
                .trim_end_matches('\'');
            return Some(unquoted.to_string());
        }
    }
    None
}

fn classify(id: &str, id_like: &str) -> DistroFamily {
    // Match ID first, then ID_LIKE tokens.
    for token in id.split_whitespace().chain(id_like.split_whitespace()) {
        match token {
            "arch" | "archlinux" | "manjaro" | "endeavouros" | "artix" => {
                return DistroFamily::Arch;
            }
            "debian" | "ubuntu" | "linuxmint" | "pop" | "raspbian" | "kali" => {
                return DistroFamily::Debian;
            }
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "nobara" => {
                return DistroFamily::Fedora;
            }
            "suse" | "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" | "sled" => {
                return DistroFamily::Suse;
            }
            _ => {}
        }
    }
    DistroFamily::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_test_is_deterministic() {
        let s = SystemCtx::for_test();
        assert_eq!(s.distro_id, "arch");
        assert_eq!(s.distro_name, "Arch Linux");
        assert_eq!(s.family, DistroFamily::Arch);
        assert_eq!(s.pkgman.as_deref(), Some("pacman"));
        assert_eq!(s.hostname, "testhost");
        assert_eq!(s.fstype, "ext4");
        assert_eq!(s.root_total, 1_099_511_627_776);
        assert_eq!(s.root_used, 549_755_813_888);
    }

    #[test]
    fn classify_maps_families() {
        assert_eq!(classify("arch", ""), DistroFamily::Arch);
        assert_eq!(classify("manjaro", ""), DistroFamily::Arch);
        assert_eq!(classify("ubuntu", "debian"), DistroFamily::Debian);
        assert_eq!(classify("pop", "debian ubuntu"), DistroFamily::Debian);
        assert_eq!(classify("fedora", ""), DistroFamily::Fedora);
        assert_eq!(
            classify("rocky", "rhel centos fedora"),
            DistroFamily::Fedora
        );
        assert_eq!(
            classify("opensuse-leap", "suse opensuse"),
            DistroFamily::Suse
        );
        assert_eq!(classify("gentoo", ""), DistroFamily::Other);
    }

    #[test]
    fn detect_degrades_never_panics() {
        // Live-host read-only probe: shape only.
        let s = SystemCtx::detect().expect("detect");
        assert!(!s.distro_id.is_empty());
        assert!(!s.distro_name.is_empty());
        assert!(!s.hostname.is_empty());
    }

    #[test]
    fn os_release_parsing_handles_quotes() {
        let text = "NAME=\"Arch Linux\"\nID=arch\nID_LIKE=\"\"\nPRETTY_NAME=\"Arch Linux\"\n";
        assert_eq!(os_release_key(text, "NAME").as_deref(), Some("Arch Linux"));
        assert_eq!(os_release_key(text, "ID").as_deref(), Some("arch"));
        assert_eq!(os_release_key(text, "ID_LIKE").as_deref(), Some(""));
        assert_eq!(os_release_key(text, "NOPE"), None);
    }
}
