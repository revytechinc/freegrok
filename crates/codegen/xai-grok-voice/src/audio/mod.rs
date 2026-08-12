//! Microphone capture (optional `audio` feature).
//!
//! Backends share one interface (`spawn_pcm_capture`,
//! `capture_pcm_for_duration`, `input_device_info`, `CaptureHandle`):
//!
//! - **Linux**: a subprocess recorder (`pw-record`/`parec`/`arecord`) — the
//!   static-musl release binary cannot link `cpal` → `alsa-sys`; see
//!   [`capture_linux`].
//! - **macOS**: a subprocess too — the self-exec `__mic-capture` helper —
//!   because in-process CoreAudio memory is never returned after the stream
//!   drops; see [`capture_subprocess`].
//! - **Windows**: `cpal` (WASAPI) in-process; its memory cost is modest.
//! - **FreeBSD / other**: stub that returns a clear config error (no cpal);
//!   see [`capture_unsupported`]. Agent/TUI still builds and runs.

// cpal-based capture: Windows backend, macOS fallback / `__mic-capture` child.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod capture;
// Wire protocol shared by the `__mic-capture` child (writer, in `capture`)
// and the macOS parent (parser, in `capture_subprocess`).
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod protocol;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use capture::capture_pcm_for_duration;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use capture::run_capture_child_cli;
#[cfg(target_os = "windows")]
pub use capture::{CaptureHandle, input_device_info, spawn_pcm_capture};

// Shared PCM-over-pipe plumbing for the two subprocess backends.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod pipe;

#[cfg(target_os = "macos")]
mod capture_subprocess;
#[cfg(target_os = "macos")]
pub use capture_subprocess::{CaptureHandle, input_device_info, spawn_pcm_capture};

#[cfg(target_os = "linux")]
mod capture_linux;
#[cfg(target_os = "linux")]
pub use capture_linux::{
    CaptureHandle, capture_pcm_for_duration, input_device_info, spawn_pcm_capture,
};

// FreeBSD and other OS: no mic backend yet; fail closed with a clear error.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod capture_unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub use capture_unsupported::{
    CaptureHandle, capture_pcm_for_duration, input_device_info, spawn_pcm_capture,
};
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) use capture_unsupported::run_capture_child_cli;
