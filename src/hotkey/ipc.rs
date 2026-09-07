use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcCommand {
    Toggle,
    PttDown,
    PttUp,
    Cancel,
    Status,
    ReloadConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub status: String,
    pub message: String,
}

pub struct IpcServer {
    socket_path: String,
    cmd_tx: Sender<IpcCommand>,
}

impl IpcServer {
    pub fn new(socket_path: String, cmd_tx: Sender<IpcCommand>) -> Self {
        Self {
            socket_path,
            cmd_tx,
        }
    }

    pub async fn run(self) -> Result<()> {
        let path = Path::new(&self.socket_path);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let listener = UnixListener::bind(path)
            .with_context(|| format!("Failed to bind IPC Unix socket at {}", self.socket_path))?;
        tracing::info!("OpenWhisper IPC server listening on {}", self.socket_path);

        loop {
            match listener.accept().await {
                Ok((mut stream, _)) => {
                    let cmd_tx = self.cmd_tx.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 512];
                        if let Ok(n) = stream.read(&mut buf).await {
                            if n > 0 {
                                if let Ok(cmd_str) = std::str::from_utf8(&buf[..n]) {
                                    let cmd_trim = cmd_str.trim();
                                    let cmd = parse_ipc_command(cmd_str);

                                    if let Some(c) = cmd {
                                        let is_reload = matches!(c, IpcCommand::ReloadConfig);
                                        let _ = cmd_tx.send(c).await;
                                        let message = if is_reload {
                                            "Configuration reloaded successfully".to_string()
                                        } else {
                                            "command processed".to_string()
                                        };
                                        let res = IpcResponse {
                                            status: "ok".to_string(),
                                            message,
                                        };
                                        if let Ok(res_bytes) = serde_json::to_vec(&res) {
                                            let _ = stream.write_all(&res_bytes).await;
                                        }
                                    } else {
                                        let res = IpcResponse {
                                            status: "error".to_string(),
                                            message: format!("unknown command: {}", cmd_trim),
                                        };
                                        if let Ok(res_bytes) = serde_json::to_vec(&res) {
                                            let _ = stream.write_all(&res_bytes).await;
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
                Err(err) => {
                    tracing::error!("IPC accept error: {err}");
                }
            }
        }
    }
}

/// Parses an incoming IPC command string either from JSON or from plain-text alias.
pub fn parse_ipc_command(input: &str) -> Option<IpcCommand> {
    let cmd_trim = input.trim();
    serde_json::from_str(cmd_trim).ok().or_else(|| match cmd_trim {
        "toggle" => Some(IpcCommand::Toggle),
        "ptt-down" | "down" | "press" => Some(IpcCommand::PttDown),
        "ptt-up" | "up" | "release" => Some(IpcCommand::PttUp),
        "cancel" => Some(IpcCommand::Cancel),
        "status" => Some(IpcCommand::Status),
        "reload" | "reload-config" | "reload_config" => Some(IpcCommand::ReloadConfig),
        _ => None,
    })
}

pub async fn send_ipc_command(socket_path: &str, cmd: IpcCommand) -> Result<IpcResponse> {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("OpenWhisper daemon is not running (cannot connect to {})", socket_path))?;

    let payload = serde_json::to_vec(&cmd)?;
    stream.write_all(&payload).await?;

    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let response: IpcResponse = serde_json::from_slice(&buf[..n])
        .context("Invalid response from OpenWhisper daemon")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipc_command_json() {
        assert!(matches!(parse_ipc_command(r#""toggle""#), Some(IpcCommand::Toggle)));
        assert!(matches!(parse_ipc_command(r#""ptt_down""#), Some(IpcCommand::PttDown)));
        assert!(matches!(parse_ipc_command(r#""ptt_up""#), Some(IpcCommand::PttUp)));
        assert!(matches!(parse_ipc_command(r#""cancel""#), Some(IpcCommand::Cancel)));
        assert!(matches!(parse_ipc_command(r#""status""#), Some(IpcCommand::Status)));
        assert!(matches!(parse_ipc_command(r#""reload_config""#), Some(IpcCommand::ReloadConfig)));
    }

    #[test]
    fn test_parse_ipc_command_aliases() {
        assert!(matches!(parse_ipc_command("toggle"), Some(IpcCommand::Toggle)));
        assert!(matches!(parse_ipc_command("ptt-down"), Some(IpcCommand::PttDown)));
        assert!(matches!(parse_ipc_command("press"), Some(IpcCommand::PttDown)));
        assert!(matches!(parse_ipc_command("ptt-up"), Some(IpcCommand::PttUp)));
        assert!(matches!(parse_ipc_command("release"), Some(IpcCommand::PttUp)));
        assert!(matches!(parse_ipc_command("cancel"), Some(IpcCommand::Cancel)));
        assert!(matches!(parse_ipc_command("status"), Some(IpcCommand::Status)));
        assert!(matches!(parse_ipc_command("reload"), Some(IpcCommand::ReloadConfig)));
        assert!(matches!(parse_ipc_command("reload-config"), Some(IpcCommand::ReloadConfig)));
        assert!(matches!(parse_ipc_command("reload_config"), Some(IpcCommand::ReloadConfig)));
        assert!(parse_ipc_command("invalid_xyz").is_none());
    }

    #[test]
    fn test_ipc_response_serialization() {
        let resp = IpcResponse {
            status: "ok".to_string(),
            message: "done".to_string(),
        };
        let serialized = serde_json::to_string(&resp).unwrap();
        assert!(serialized.contains(r#""status":"ok""#));
        assert!(serialized.contains(r#""message":"done""#));
    }
}
