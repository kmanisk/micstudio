//! IPC Protocol and Client/Server Socket Definitions.
//! Simple JSON-framed communication over a local Unix domain socket.

use crate::dsp::pipeline::AudioTelemetry;
use crate::presets::Preset;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    ApplyPreset(Preset),
    GetStatus,
    StopEngine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Status {
        active_preset: String,
        telemetry: AudioTelemetry,
    },
    Success,
    Error(String),
}

pub fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    PathBuf::from(runtime_dir).join("micstudio.sock")
}

pub fn send_command(req: &IpcRequest) -> Result<IpcResponse> {
    let path = socket_path();
    let stream = UnixStream::connect(&path)
        .with_context(|| format!("Engine socket not found at {}", path.display()))?;

    let mut writer = stream.try_clone()?;
    let data = serde_json::to_string(req)?;
    writer.write_all(data.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let resp: IpcResponse = serde_json::from_str(&line)?;
    Ok(resp)
}

pub fn is_engine_running() -> bool {
    let path = socket_path();
    if !path.exists() {
        return false;
    }
    UnixStream::connect(&path).is_ok()
}
