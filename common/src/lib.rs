//! mindgate-common
//!
//! Shared types used by the CLI, daemon, and browser extension.
//! This crate defines the IPC protocol and socket framing.
//! It deliberately knows nothing about blocking rules.

use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

/// Represents the current lock state of the extension.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LockState {
    pub locked: bool,
    /// Milliseconds since epoch. None means locked forever.
    pub unlock_at: Option<u64>,
}

/// Requests sent to the daemon via the Unix domain socket.
/// Note: `content = "args"` was removed to allow the browser extension 
/// to send a flat JSON object: { "cmd": "ExtensionHeartbeat", "lockState": ... }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum Request {
    /// Simple connectivity check from CLI.
    Ping,
    /// Request daemon version info.
    Version,
    /// Sent by the browser extension to prove it is alive.
    ExtensionHeartbeat {
        #[serde(default, rename = "lockState")]
        lock_state: Option<LockState>,
    },
    /// Request current daemon and extension status (for `mindgate status`/`doctor`).
    Status,
    /// Instruct the daemon to shut down gracefully (for `mindgate stop`).
    Shutdown,
}

/// Status information returned by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInfo {
    pub daemon_running: bool,
    pub extension_connected: bool,
    #[serde(default)]
    pub lock_state: Option<LockState>,
}

/// Responses sent from the daemon to the CLI or extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", content = "data")]
pub enum Response {
    Ok,
    Pong,
    Version {
        daemon: String,
        protocol: u16,
    },
    Status(StatusInfo),
    Error {
        message: String,
    },
}

/// Returns the path to the Unix domain socket.
/// Can be overridden via the `MINDGATE_SOCKET` environment variable,
/// which is useful for testing or non-standard installations.
pub fn socket_path() -> PathBuf {
    if let Ok(path) = env::var("MINDGATE_SOCKET") {
        return PathBuf::from(path);
    }
    // Default path for a root/systemd-managed daemon
    PathBuf::from("/run/mindgate/mindgate.sock")
}

/// Wire protocol utilities for length-prefixed JSON framing.
pub mod wire {
    use serde::{de::DeserializeOwned, Serialize};

    /// Encodes a message with a 4-byte big-endian length prefix.
    pub fn encode<T: Serialize>(msg: &T) -> anyhow::Result<Vec<u8>> {
        let body = serde_json::to_vec(msg)?;
        let len = (body.len() as u32).to_be_bytes();

        let mut out = Vec::with_capacity(4 + body.len());
        out.extend_from_slice(&len);
        out.extend_from_slice(&body);

        Ok(out)
    }

    /// Decodes a message from a byte slice (excluding the 4-byte length prefix).
    pub fn decode<T: DeserializeOwned>(body: &[u8]) -> anyhow::Result<T> {
        Ok(serde_json::from_slice(body)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_roundtrip() {
        // Updated to match the new struct variant
        let req = Request::ExtensionHeartbeat { lock_state: None };
        let bytes = wire::encode(&req).unwrap();
        
        // Skip the first 4 bytes (length prefix) for decoding
        let decoded: Request = wire::decode(&bytes[4..]).unwrap();

        match decoded {
            Request::ExtensionHeartbeat { lock_state: None } => {}
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn default_socket_path_when_no_env_override() {
        // Note: std::env::remove_var can affect other tests if run in parallel,
        // but for a simple MVP check, this is acceptable.
        let _ = std::env::remove_var("MINDGATE_SOCKET");
        assert_eq!(socket_path(), PathBuf::from("/run/mindgate/mindgate.sock"));
    }
}