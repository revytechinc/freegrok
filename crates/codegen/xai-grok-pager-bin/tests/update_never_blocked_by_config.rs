//! `grok update` is a recovery command: a config failure must not block it.
//!
//! Hermetic: a local server serves the binary's own version as the channel
//! pointer, so a healthy run exits 0 ("already up to date") and a corrupt
//! config must too — reintroducing a config `?` fails exactly that run.
//! The pointer must equal the current version: the installer converges in
//! both directions, so an older pointer triggers a downgrade attempt.

use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Resolve the pager binary like the PTY harness: `PAGER_BINARY` under
/// Bazel (runfiles-relative), else cargo's compile-time constant.
fn pager_binary() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("PAGER_BINARY") {
        return std::path::absolute(&p)
            .unwrap_or_else(|e| panic!("failed to absolutize PAGER_BINARY {p}: {e}"));
    }
    option_env!("CARGO_BIN_EXE_freegrok")
        .or(option_env!("CARGO_BIN_EXE_xai-grok-pager"))
        .map(std::path::PathBuf::from)
        .expect("PAGER_BINARY is unset and this build is not `cargo test`")
}

/// Local base answering every request with the channel pointer body.
fn spawn_pointer_server(body: Arc<Mutex<String>>) -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let serving = listener.try_clone().unwrap();
    std::thread::spawn(move || {
        for stream in serving.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let version = body.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    version.len(),
                    version
                )
                .as_bytes(),
            );
        }
    });
    (listener, base)
}

/// Run `grok update` in an isolated home against the local pointer base.
fn run_update(base: &str, config_toml: &str, extra_args: &[&str]) -> std::process::Output {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), config_toml).unwrap();
    Command::new(pager_binary())
        .arg("update")
        .args(extra_args)
        .env_clear()
        .env("HOME", home.path())
        .env("GROK_HOME", home.path())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GROK_CLI_BASE_URL", base)
        .output()
        .expect("spawn grok update")
}

/// FreeGrok must not contact the xAI Grok update channel. A loopback
/// pointer that counts hits proves `--check` does not fetch.
#[test]
fn freegrok_update_check_does_not_contact_channel() {
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let serving = listener.try_clone().unwrap();
    let hits_thread = hits.clone();
    std::thread::spawn(move || {
        for stream in serving.incoming() {
            hits_thread.fetch_add(1, Ordering::SeqCst);
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 256];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n0.0.1");
        }
    });

    let check = run_update(&base, "[cli]\n", &["--check", "--json"]);
    assert!(
        check.status.success(),
        "disabled updater must still exit 0\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap_or_else(|e| {
        panic!(
            "update --check --json must emit JSON: {e}\nstdout:\n{}",
            String::from_utf8_lossy(&check.stdout)
        )
    });
    assert_eq!(
        status["disabled"], true,
        "FreeGrok updater must report disabled, got {status}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "must not contact GROK_CLI_BASE_URL when the FreeGrok updater is off"
    );

    let install = run_update(&base, "[cli]\n", &[]);
    assert!(
        install.status.success(),
        "disabled `freegrok update` must exit 0\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(
        text.to_ascii_lowercase().contains("off")
            || text.to_ascii_lowercase().contains("disabled"),
        "human update output must say the updater is off, got: {text:?}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

/// The valid run proves the environment resolves to success, so a nonzero
/// corrupt run can only mean a config failure aborted the update.
/// With the FreeGrok updater off, both paths must still exit 0 (disabled).
#[test]
fn corrupt_config_never_changes_update_outcome() {
    let body = Arc::new(Mutex::new("0.0.1".to_owned()));
    let (_listener, base) = spawn_pointer_server(body.clone());

    let valid = run_update(&base, "[cli]\n", &["--check", "--json"]);
    assert!(
        valid.status.success(),
        "healthy grok update against the local base must exit 0\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&valid.stdout),
        String::from_utf8_lossy(&valid.stderr)
    );

    let corrupt = run_update(&base, "this is not toml {{{[[[", &["--check", "--json"]);
    assert!(
        corrupt.status.success(),
        "a corrupt config.toml must not block grok update\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&corrupt.stdout),
        String::from_utf8_lossy(&corrupt.stderr)
    );
    let valid_json: serde_json::Value = serde_json::from_slice(&valid.stdout).unwrap();
    let corrupt_json: serde_json::Value = serde_json::from_slice(&corrupt.stdout).unwrap();
    assert_eq!(valid_json["disabled"], true);
    assert_eq!(corrupt_json["disabled"], true);
}
