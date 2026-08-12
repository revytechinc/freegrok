//! Factory that selects the sandbox backend for the current target OS.
//!
//! Selection is a pure function of `(os, enforce)` so unit tests can assert
//! FreeBSD / Linux / macOS / Windows behavior on any host. Compile-time
//! `for_host()` is a thin wrapper over `std::env::consts::OS`.

/// Which sandbox implementation this host should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackendKind {
    /// Landlock + bwrap (Linux).
    NonoLinux,
    /// Seatbelt via nono (macOS).
    NonoMacos,
    /// FreeBSD jail helper / degrade path.
    Jail,
    /// No kernel backend (Windows, unknown, or enforce off).
    Noop,
}

impl SandboxBackendKind {
    /// Stable doctor / telemetry label. Must not change without a migration note.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonoLinux => "nono-linux",
            Self::NonoMacos => "nono-macos",
            Self::Jail => "jail",
            Self::Noop => "noop",
        }
    }

    /// True when a kernel (or jail) backend is expected for this kind.
    pub fn is_enforcing(self) -> bool {
        !matches!(self, Self::Noop)
    }

    /// Human detail string for `grok doctor` sandbox.backend check.
    pub fn doctor_detail(self) -> &'static str {
        match self {
            Self::Jail => {
                "Isolation uses FreeBSD jails via an optional privileged helper (not Landlock/Seatbelt)."
            }
            Self::NonoLinux => {
                "Isolation uses Landlock and optional bubblewrap re-exec."
            }
            Self::NonoMacos => "Isolation uses Seatbelt profiles via nono.",
            Self::Noop => {
                "No OS sandbox backend for this target (enforce off or unsupported OS)."
            }
        }
    }
}

/// Constructs the host backend kind (and later trait objects).
pub struct SandboxBackendFactory;

impl SandboxBackendFactory {
    /// Select backend for an OS name (`std::env::consts::OS` values).
    ///
    /// FreeBSD must never select a nono/Landlock path. Bare `"unix"` is not a
    /// valid OS selector and maps to [`SandboxBackendKind::Noop`].
    pub fn for_os(os: &str, enforce: bool) -> SandboxBackendKind {
        if !enforce {
            return SandboxBackendKind::Noop;
        }
        match os {
            "linux" => SandboxBackendKind::NonoLinux,
            "macos" => SandboxBackendKind::NonoMacos,
            "freebsd" => SandboxBackendKind::Jail,
            // Never treat the family name as a backend — nono is linux|macos only.
            "unix" | "windows" | "openbsd" | "netbsd" | "dragonfly" | "illumos" | "solaris"
            | "android" | "ios" | _ => SandboxBackendKind::Noop,
        }
    }

    /// Intended backend for this OS when enforcement is available.
    ///
    /// Independent of the `enforce` cargo feature — dependents often pull
    /// `xai-grok-sandbox` with `default-features = false` and still need doctor
    /// / product labels for the host platform.
    pub fn for_platform() -> SandboxBackendKind {
        Self::for_os(std::env::consts::OS, true)
    }

    /// Backend for the compile-time target with the crate's `enforce` feature.
    ///
    /// When `enforce` is off, this is always [`SandboxBackendKind::Noop`].
    pub fn for_host() -> SandboxBackendKind {
        Self::for_os(std::env::consts::OS, cfg!(feature = "enforce"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_enforce_selects_nono_linux() {
        assert_eq!(
            SandboxBackendFactory::for_os("linux", true),
            SandboxBackendKind::NonoLinux
        );
        assert!(SandboxBackendKind::NonoLinux.is_enforcing());
        assert_eq!(SandboxBackendKind::NonoLinux.as_str(), "nono-linux");
    }

    #[test]
    fn macos_enforce_selects_nono_macos() {
        assert_eq!(
            SandboxBackendFactory::for_os("macos", true),
            SandboxBackendKind::NonoMacos
        );
        assert_eq!(SandboxBackendKind::NonoMacos.as_str(), "nono-macos");
    }

    #[test]
    fn freebsd_enforce_selects_jail_never_nono() {
        let kind = SandboxBackendFactory::for_os("freebsd", true);
        assert_eq!(kind, SandboxBackendKind::Jail);
        assert_eq!(kind.as_str(), "jail");
        assert!(kind.is_enforcing());
        // Critical FreeBSD port invariant: never Landlock/Seatbelt backends.
        assert_ne!(kind, SandboxBackendKind::NonoLinux);
        assert_ne!(kind, SandboxBackendKind::NonoMacos);
    }

    #[test]
    fn bare_unix_never_selects_landlock_or_seatbelt() {
        // Historical bug class: cfg(unix) pulled nono on FreeBSD. Family name
        // must not unlock a Linux/macOS backend.
        let kind = SandboxBackendFactory::for_os("unix", true);
        assert_eq!(kind, SandboxBackendKind::Noop);
        assert!(!kind.is_enforcing());
    }

    #[test]
    fn enforce_off_is_always_noop_even_on_linux() {
        for os in ["linux", "macos", "freebsd", "windows", "unix"] {
            assert_eq!(
                SandboxBackendFactory::for_os(os, false),
                SandboxBackendKind::Noop,
                "os={os}"
            );
        }
    }

    #[test]
    fn windows_and_unknown_os_are_noop_when_enforce_on() {
        assert_eq!(
            SandboxBackendFactory::for_os("windows", true),
            SandboxBackendKind::Noop
        );
        assert_eq!(
            SandboxBackendFactory::for_os("haiku", true),
            SandboxBackendKind::Noop
        );
        assert_eq!(
            SandboxBackendFactory::for_os("openbsd", true),
            SandboxBackendKind::Noop
        );
    }

    #[test]
    fn stable_labels_are_unique() {
        let labels = [
            SandboxBackendKind::NonoLinux.as_str(),
            SandboxBackendKind::NonoMacos.as_str(),
            SandboxBackendKind::Jail.as_str(),
            SandboxBackendKind::Noop.as_str(),
        ];
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "duplicate label {a}");
                }
            }
        }
    }

    #[test]
    fn for_host_matches_for_os_on_this_target() {
        let host = SandboxBackendFactory::for_host();
        let expected = SandboxBackendFactory::for_os(
            std::env::consts::OS,
            cfg!(feature = "enforce"),
        );
        assert_eq!(host, expected);
        assert!(!host.as_str().is_empty());
    }

    #[test]
    fn for_platform_ignores_enforce_feature() {
        let platform = SandboxBackendFactory::for_platform();
        let expected = SandboxBackendFactory::for_os(std::env::consts::OS, true);
        assert_eq!(platform, expected);
        // Platform label is always the OS backend (never force-Noop solely because
        // a dependent disabled the enforce feature).
        match std::env::consts::OS {
            "linux" => assert_eq!(platform, SandboxBackendKind::NonoLinux),
            "macos" => assert_eq!(platform, SandboxBackendKind::NonoMacos),
            "freebsd" => assert_eq!(platform, SandboxBackendKind::Jail),
            _ => assert_eq!(platform, SandboxBackendKind::Noop),
        }
    }

    #[test]
    fn doctor_detail_covers_all_kinds_and_freebsd_avoids_landlock() {
        for kind in [
            SandboxBackendKind::NonoLinux,
            SandboxBackendKind::NonoMacos,
            SandboxBackendKind::Jail,
            SandboxBackendKind::Noop,
        ] {
            assert!(!kind.doctor_detail().is_empty(), "{kind:?}");
            assert_eq!(kind.as_str().is_empty(), false);
        }
        let jail = SandboxBackendKind::Jail;
        assert_eq!(jail.as_str(), "jail");
        let detail = jail.doctor_detail().to_ascii_lowercase();
        assert!(detail.contains("jail"));
        // FreeBSD path must not claim Landlock/Seatbelt as the mechanism.
        // (Negated mentions like "not landlock" are fine.)
        assert!(
            !detail.contains("uses landlock")
                && !detail.contains("via nono")
                && !detail.contains("seatbelt profiles"),
            "jail detail must not advertise nono backends: {detail}"
        );
        assert!(
            detail.contains("not landlock") || detail.contains("not") && detail.contains("landlock"),
            "jail detail should explicitly disclaim Landlock: {detail}"
        );
    }
}
