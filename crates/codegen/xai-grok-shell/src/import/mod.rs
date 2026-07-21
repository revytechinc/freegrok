//! Multi-source config import / export.
//!
//! - [`bundle`]: native Grok config export + re-import
//! - [`opencode`]: OpenCode config scan
//! - [`ssh`]: OpenSSH transport (`~/.ssh/config`, keys, password via askpass)
//! - [`dedup`] / [`findings`] / [`remote_paths`]: remote SSH import foundation

pub mod bundle;
pub mod dedup;
pub mod findings;
pub mod opencode;
pub mod paths;
pub mod remote_paths;
pub mod ssh;

pub use bundle::{
    export_config, export_summary, import_config, read_manifest, ExportOptions, ExportResult,
    ImportOptions, ImportReport, BUNDLE_FORMAT, BUNDLE_VERSION,
};
pub use dedup::{
    collapse_findings, equivalence_endpoint, equivalence_mcp_stdio, equivalence_mcp_url,
    equivalence_model, equivalence_skill, is_local_inference_url, normalize_base_url,
};
pub use findings::{
    DedupStats, FindReport, FindingKind, LocalDiff, RemoteFinding, SecretPolicy, SourceProduct,
};
pub use opencode::{scan_opencode, OpenCodeImport};
pub use paths::ImportSource;
pub use remote_paths::{
    project_catalog, resolve_posix_catalog, resolve_windows_catalog, RemoteOs, ResolvedPath,
};
pub use ssh::{
    auth_diagnostics, build_ssh_invocation, default_ssh_config_path, default_ssh_key_candidates,
    remote_home, remote_paths_exist, remote_read_file, remote_uname, ssh_agent_present, ssh_exec,
    ssh_exec_async, ssh_resolve_config, SshAuth, SshError, SshInvocation, SshResolvedConfig,
    SshSession, SshTarget,
};
