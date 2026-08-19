//! The registry is the source of truth and the operator tables are
//! hand-maintained mirrors with no compile-time tripwire of their own. This test
//! is theirs.
//!
//! xAI internal docs (`docs/internal/…`) are not shipped in the FreeGrok OSS
//! tree. Missing files skip this tripwire instead of failing to compile.

use std::path::PathBuf;
use xai_grok_shell::agent::config::FEATURES;

fn internal_doc(name: &str) -> Option<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs/internal")
        .join(name);
    std::fs::read_to_string(path).ok()
}

#[test]
fn every_registered_feature_reaches_the_operator() {
    let Some(enterprise) = internal_doc("25-enterprise.md") else {
        eprintln!("skip: docs/internal/25-enterprise.md not in this tree (OSS)");
        return;
    };
    let Some(env_vars) = internal_doc("22-environment-variables.md") else {
        eprintln!("skip: docs/internal/22-environment-variables.md not in this tree (OSS)");
        return;
    };
    for spec in FEATURES {
        assert!(
            enterprise.contains(&format!("`{}`", spec.key)),
            "{} has no row in the 25-enterprise.md pinning table",
            spec.key,
        );
        assert!(
            env_vars.contains(&format!("`{}`", spec.env)),
            "{} is undocumented in 22-environment-variables.md",
            spec.env,
        );
    }
}
