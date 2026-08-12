//! Stub mic capture for platforms without a voice backend (FreeBSD today).
//!
//! Voice dictation is optional on FreeBSD: the agent/TUI must still link and
//! run. Opening a session returns a clear config error instead of panicking or
//! pulling cpal/alsa into the FreeBSD package.

use tokio::sync::mpsc as async_mpsc;

use crate::error::VoiceError;

const UNSUPPORTED: &str =
    "microphone capture is not supported on this platform yet (FreeBSD voice backend pending)";

/// Stop handle — no-op on unsupported platforms.
#[derive(Debug)]
pub struct CaptureHandle;

impl CaptureHandle {
    pub fn stop(self) {}
}

/// Always fails: no capture backend on this OS.
pub fn spawn_pcm_capture(
    _sample_rate: u32,
    _pcm_tx: async_mpsc::Sender<Vec<u8>>,
) -> Result<CaptureHandle, VoiceError> {
    Err(VoiceError::Config(UNSUPPORTED.into()))
}

/// Always fails: no capture backend on this OS.
pub fn input_device_info() -> Result<crate::probe::InputDeviceInfo, VoiceError> {
    Err(VoiceError::Config(UNSUPPORTED.into()))
}

/// Always fails: no capture backend on this OS.
pub fn capture_pcm_for_duration(
    _sample_rate: u32,
    _seconds: u32,
) -> Result<(Vec<u8>, u32), VoiceError> {
    Err(VoiceError::Config(UNSUPPORTED.into()))
}

/// Hidden `__mic-capture` child is never spawned on FreeBSD; hand-invocation exits 1.
pub(crate) fn run_capture_child_cli(_args: Vec<String>) -> i32 {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr(),
        "error: {UNSUPPORTED}"
    );
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_ops_return_config_error() {
        let err = spawn_pcm_capture(16_000, async_mpsc::channel(1).0).unwrap_err();
        assert!(matches!(err, VoiceError::Config(_)));
        let msg = err.to_string().to_ascii_lowercase();
        assert!(msg.contains("not supported") || msg.contains("freebsd") || msg.contains("platform"));
        assert!(input_device_info().is_err());
        assert!(capture_pcm_for_duration(16_000, 1).is_err());
        assert_eq!(run_capture_child_cli(vec![]), 1);
    }
}
