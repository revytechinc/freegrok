//! FreeBSD base/userland catalog for jail provisioning.
//!
//! Sources (HTTPS; `ftp.freebsd.org` redirects here):
//! - Releases: `https://download.freebsd.org/ftp/releases/{arch}/{arch}/`
//! - Snapshots: `https://download.freebsd.org/ftp/snapshots/{arch}/{arch}/`
//!
//! **Host rule:** jail userland major.minor must not exceed the host kernel
//! (e.g. FreeBSD 15.1-STABLE cannot run a 16.0-CURRENT jail). Older userland
//! on a newer host is OK (14.x on 15.1).

use std::fmt;

/// Canonical download root (ftp.freebsd.org → download.freebsd.org).
pub const FREEBSD_DOWNLOAD_ROOT: &str = "https://download.freebsd.org/ftp";

/// Parsed FreeBSD version (major.minor + branch label).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FreeBsdVersion {
    pub major: u32,
    pub minor: u32,
    /// RELEASE | STABLE | CURRENT | unknown
    pub branch: String,
    /// Original directory name, e.g. `15.1-RELEASE`
    pub raw: String,
}

impl fmt::Display for FreeBsdVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl FreeBsdVersion {
    /// Parse `15.1-STABLE`, `14.3-RELEASE`, `16.0-CURRENT`.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim().trim_end_matches('/');
        let (num, branch) = raw.split_once('-')?;
        let mut parts = num.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        Some(Self {
            major,
            minor,
            branch: branch.to_string(),
            raw: raw.to_string(),
        })
    }

    /// Numeric (major, minor) for ordering within the same branch family.
    pub fn mm(&self) -> (u32, u32) {
        (self.major, self.minor)
    }
}

/// Host FreeBSD version from `freebsd-version` / `uname -r` style strings.
pub fn parse_host_version(release: &str) -> Option<FreeBsdVersion> {
    FreeBsdVersion::parse(release)
}

/// Whether `jail` userland is allowed on `host` kernel/userland.
///
/// Policy (terse):
/// - Same or older major.minor only.
/// - CURRENT jails only when host is CURRENT at same or higher major.
/// - Never install a newer major than the host (16 on 15 → no).
pub fn jail_userland_compatible(host: &FreeBsdVersion, jail: &FreeBsdVersion) -> bool {
    if jail.major > host.major {
        return false;
    }
    if jail.major < host.major {
        // Older major always OK (14.x on 15.1).
        return true;
    }
    // Same major: minor must be <= host.
    if jail.minor > host.minor {
        return false;
    }
    // CURRENT jail on non-CURRENT host at same major.minor is risky; allow only
    // when host branch is also CURRENT.
    if jail.branch.eq_ignore_ascii_case("CURRENT")
        && !host.branch.eq_ignore_ascii_case("CURRENT")
    {
        return false;
    }
    true
}

/// Preferred default jail base for a host (newest compatible RELEASE, else STABLE).
pub fn preferred_jail_base<'a>(
    host: &FreeBsdVersion,
    catalog: &'a [FreeBsdVersion],
) -> Option<&'a FreeBsdVersion> {
    let mut compatible: Vec<&FreeBsdVersion> = catalog
        .iter()
        .filter(|j| jail_userland_compatible(host, j))
        .collect();
    if compatible.is_empty() {
        return None;
    }
    // Prefer RELEASE matching host major.minor, then any RELEASE, then STABLE.
    if let Some(v) = compatible.iter().find(|j| {
        j.major == host.major
            && j.minor == host.minor
            && j.branch.eq_ignore_ascii_case("RELEASE")
    }) {
        return Some(*v);
    }
    compatible.sort_by(|a, b| {
        let rank = |v: &FreeBsdVersion| {
            let br = if v.branch.eq_ignore_ascii_case("RELEASE") {
                2
            } else if v.branch.eq_ignore_ascii_case("STABLE") {
                1
            } else {
                0
            };
            (v.major, v.minor, br)
        };
        rank(a).cmp(&rank(b))
    });
    compatible.pop()
}

/// URL for `base.txz` for a release or snapshot entry.
pub fn base_txz_url(arch: &str, version: &FreeBsdVersion) -> String {
    let kind = if version.branch.eq_ignore_ascii_case("RELEASE") {
        "releases"
    } else {
        "snapshots"
    };
    format!(
        "{FREEBSD_DOWNLOAD_ROOT}/{kind}/{arch}/{arch}/{}/base.txz",
        version.raw
    )
}

/// Parse directory index HTML for version folder names.
pub fn parse_directory_index(html: &str) -> Vec<FreeBsdVersion> {
    let mut out = Vec::new();
    // href="15.1-RELEASE/" or href='14.3-RELEASE'
    for part in html.split("href=") {
        let rest = part.trim_start_matches(['"', '\'']);
        let name = rest.split(['"', '\'', '/']).next().unwrap_or("");
        if let Some(v) = FreeBsdVersion::parse(name) {
            if !out.iter().any(|x: &FreeBsdVersion| x.raw == v.raw) {
                out.push(v);
            }
        }
    }
    out.sort_by(|a, b| a.mm().cmp(&b.mm()).then_with(|| a.branch.cmp(&b.branch)));
    out
}

/// Build catalog URLs for arch (e.g. `amd64`).
pub fn catalog_index_urls(arch: &str) -> [String; 2] {
    [
        format!("{FREEBSD_DOWNLOAD_ROOT}/releases/{arch}/{arch}/"),
        format!("{FREEBSD_DOWNLOAD_ROOT}/snapshots/{arch}/{arch}/"),
    ]
}

/// Local-only jail network policy for agent isolation (console / jexec first).
///
/// Linux Landlock/bwrap does **not** need a network namespace for FS isolation.
/// FreeBSD agent jails default the same: no VNET, no public IP, optional
/// loopback-only later for localhost SSH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailNetworkMode {
    /// No IP stack for the jail (preferred for tool sandbox / console).
    None,
    /// Loopback only (127.0.0.1) — for local sshd if enabled.
    LoopbackOnly,
}

impl Default for JailNetworkMode {
    fn default() -> Self {
        Self::None
    }
}

/// Dry-run plan for provisioning (no privilege, no download).
#[derive(Debug, Clone)]
pub struct JailProvisionPlan {
    pub host: FreeBsdVersion,
    pub selected_base: FreeBsdVersion,
    pub base_url: String,
    pub network: JailNetworkMode,
    pub root_path: String,
    pub jail_name: String,
    pub create_user: String,
    pub enable_local_ssh: bool,
    pub privilege_reason: String,
}

impl JailProvisionPlan {
    pub fn for_host(
        host: FreeBsdVersion,
        selected: FreeBsdVersion,
        arch: &str,
    ) -> Result<Self, String> {
        if !jail_userland_compatible(&host, &selected) {
            return Err(format!(
                "base {} is not compatible with host {} (userland must not exceed host)",
                selected.raw, host.raw
            ));
        }
        let base_url = base_txz_url(arch, &selected);
        Ok(Self {
            host: host.clone(),
            selected_base: selected,
            base_url,
            network: JailNetworkMode::None,
            root_path: "/usr/local/grok/jails/agent".into(),
            jail_name: "grok-agent".into(),
            create_user: "grok".into(),
            enable_local_ssh: false,
            privilege_reason: PRIVILEGE_REASON.into(),
        })
    }
}

/// Shown in TUI modal / CLI before any doas/sudo.
pub const PRIVILEGE_REASON: &str = "\
Grok needs temporary administrator rights to create a FreeBSD jail for agent \
isolation (same role as bubblewrap on Linux). This will:

  • download a FreeBSD base.txz matching or older than this host
  • extract it under /usr/local/grok/jails/ (local disk only)
  • create a jail with no public network (console/jexec; optional localhost only)
  • create a dedicated user and optional localhost SSH keys
  • install a narrow doas/sudo rule for the grok-jail-helper only

Nothing is exposed on external interfaces. You can refuse; Grok continues \
without OS sandbox (degraded isolation).";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_versions() {
        let v = FreeBsdVersion::parse("15.1-STABLE").unwrap();
        assert_eq!(v.major, 15);
        assert_eq!(v.minor, 1);
        assert_eq!(v.branch, "STABLE");
    }

    #[test]
    fn host_15_1_rejects_16_current() {
        let host = FreeBsdVersion::parse("15.1-STABLE").unwrap();
        let jail = FreeBsdVersion::parse("16.0-CURRENT").unwrap();
        assert!(!jail_userland_compatible(&host, &jail));
    }

    #[test]
    fn host_15_1_allows_15_1_release_and_14() {
        let host = FreeBsdVersion::parse("15.1-STABLE").unwrap();
        assert!(jail_userland_compatible(
            &host,
            &FreeBsdVersion::parse("15.1-RELEASE").unwrap()
        ));
        assert!(jail_userland_compatible(
            &host,
            &FreeBsdVersion::parse("14.4-RELEASE").unwrap()
        ));
        assert!(!jail_userland_compatible(
            &host,
            &FreeBsdVersion::parse("15.2-RELEASE").unwrap()
        ));
    }

    #[test]
    fn preferred_picks_matching_release() {
        let host = FreeBsdVersion::parse("15.1-STABLE").unwrap();
        let catalog = vec![
            FreeBsdVersion::parse("14.3-RELEASE").unwrap(),
            FreeBsdVersion::parse("15.0-RELEASE").unwrap(),
            FreeBsdVersion::parse("15.1-RELEASE").unwrap(),
            FreeBsdVersion::parse("15.1-STABLE").unwrap(),
            FreeBsdVersion::parse("16.0-CURRENT").unwrap(),
        ];
        let p = preferred_jail_base(&host, &catalog).unwrap();
        assert_eq!(p.raw, "15.1-RELEASE");
    }

    #[test]
    fn parse_index_html() {
        let html = r#"href="14.3-RELEASE/" href="15.1-RELEASE/" href="../""#;
        let v = parse_directory_index(html);
        assert!(v.iter().any(|x| x.raw == "15.1-RELEASE"));
    }

    #[test]
    fn plan_rejects_incompatible() {
        let host = FreeBsdVersion::parse("15.1-STABLE").unwrap();
        let bad = FreeBsdVersion::parse("16.0-CURRENT").unwrap();
        assert!(JailProvisionPlan::for_host(host, bad, "amd64").is_err());
    }
}
