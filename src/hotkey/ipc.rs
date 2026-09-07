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
                                    let cmd: Option<IpcCommand> = serde_json::from_str(cmd_trim)
                                        .ok()
                                        .or_else(|| match cmd_trim {
                                            "toggle" => Some(IpcCommand::Toggle),
                                            "ptt-down" | "down" | "press" => Some(IpcCommand::PttDown),
                                            "ptt-up" | "up" | "release" => Some(IpcCommand::PttUp),
                                            "cancel" => Some(IpcCommand::Cancel),
                                            "status" => Some(IpcCommand::Status),
                                            _ => None,
                                        });

                                    if let Some(c) = cmd {
                                        let _ = cmd_tx.send(c).await;
                                        let res = IpcResponse {
                                            status: "ok".to_string(),
                                            message: "command processed".to_string(),
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
