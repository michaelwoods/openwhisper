use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
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
    PreviewHud,
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
                Ok((stream, _)) => {
                    let cmd_tx = self.cmd_tx.clone();
                    tokio::spawn(async move {
                        let (reader, mut writer) = stream.into_split();
                        let mut buf_reader = tokio::io::BufReader::new(reader);
                        let mut line = String::new();
                        while let Ok(n) = buf_reader.read_line(&mut line).await {
                            if n == 0 {
                                break;
                            }
                            let cmd_str = line.trim();
                            if !cmd_str.is_empty() {
                                let cmd = parse_ipc_command(cmd_str);
                                let res = if let Some(c) = cmd {
                                    let is_reload = matches!(c, IpcCommand::ReloadConfig);
                                    let _ = cmd_tx.send(c).await;
                                    let message = if is_reload {
                                        "Configuration reloaded successfully".to_string()
                                    } else {
                                        "command processed".to_string()
                                    };
                                    IpcResponse {
                                        status: "ok".to_string(),
                                        message,
                                    }
                                } else {
                                    IpcResponse {
                                        status: "error".to_string(),
                                        message: format!("unknown command: {}", cmd_str),
                                    }
                                };
                                if let Ok(mut res_bytes) = serde_json::to_vec(&res) {
                                    res_bytes.push(b'\n');
                                    let _ = writer.write_all(&res_bytes).await;
                                }
                            }
                            line.clear();
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
    if let Ok(cmd) = serde_json::from_str::<IpcCommand>(cmd_trim) {
        return Some(cmd);
    }

    #[derive(Deserialize)]
    struct CmdWrapper {
        #[serde(alias = "action")]
        command: String,
    }
    if let Ok(wrapper) = serde_json::from_str::<CmdWrapper>(cmd_trim) {
        return parse_ipc_command(&wrapper.command);
    }

    match cmd_trim {
        "toggle" => Some(IpcCommand::Toggle),
        "ptt-down" | "down" | "press" => Some(IpcCommand::PttDown),
        "ptt-up" | "up" | "release" => Some(IpcCommand::PttUp),
        "cancel" => Some(IpcCommand::Cancel),
        "status" => Some(IpcCommand::Status),
        "reload" | "reload-config" | "reload_config" => Some(IpcCommand::ReloadConfig),
        "preview-hud" | "preview_hud" | "preview" => Some(IpcCommand::PreviewHud),
        _ => None,
    }
}

pub async fn send_ipc_command(socket_path: &str, cmd: IpcCommand) -> Result<IpcResponse> {
    let stream = UnixStream::connect(socket_path).await.with_context(|| {
        format!(
            "OpenWhisper daemon is not running (cannot connect to {})",
            socket_path
        )
    })?;

    let (reader, mut writer) = stream.into_split();
    let mut payload = serde_json::to_vec(&cmd)?;
    payload.push(b'\n');
    writer.write_all(&payload).await?;

    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut line = String::new();
    buf_reader
        .read_line(&mut line)
        .await
        .context("Failed to read response line from OpenWhisper daemon")?;
    let response: IpcResponse =
        serde_json::from_str(line.trim()).context("Invalid response from OpenWhisper daemon")?;
    Ok(response)
}

/// Synchronous IPC client sending a command over Unix domain socket.
/// Safe to call from UI threads and non-async contexts without initiating a Tokio runtime.
#[cfg(target_os = "linux")]
pub fn send_ipc_command_sync(socket_path: &str, cmd: IpcCommand) -> Result<IpcResponse> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path).with_context(|| {
        format!(
            "OpenWhisper daemon is not running (cannot connect to {})",
            socket_path
        )
    })?;

    let mut payload = serde_json::to_vec(&cmd)?;
    payload.push(b'\n');
    stream.write_all(&payload)?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("Failed to read response line from OpenWhisper daemon")?;
    let response: IpcResponse =
        serde_json::from_str(line.trim()).context("Invalid response from OpenWhisper daemon")?;
    Ok(response)
}

#[cfg(not(target_os = "linux"))]
pub fn send_ipc_command_sync(_socket_path: &str, _cmd: IpcCommand) -> Result<IpcResponse> {
    anyhow::bail!("IPC not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipc_command_json() {
        assert!(matches!(
            parse_ipc_command(r#""toggle""#),
            Some(IpcCommand::Toggle)
        ));
        assert!(matches!(
            parse_ipc_command(r#""ptt_down""#),
            Some(IpcCommand::PttDown)
        ));
        assert!(matches!(
            parse_ipc_command(r#""ptt_up""#),
            Some(IpcCommand::PttUp)
        ));
        assert!(matches!(
            parse_ipc_command(r#""cancel""#),
            Some(IpcCommand::Cancel)
        ));
        assert!(matches!(
            parse_ipc_command(r#""status""#),
            Some(IpcCommand::Status)
        ));
        assert!(matches!(
            parse_ipc_command(r#""reload_config""#),
            Some(IpcCommand::ReloadConfig)
        ));
        assert!(matches!(
            parse_ipc_command(r#"{"command":"preview_hud"}"#),
            Some(IpcCommand::PreviewHud)
        ));
        assert!(matches!(
            parse_ipc_command(r#"{"action":"toggle"}"#),
            Some(IpcCommand::Toggle)
        ));
    }

    #[test]
    fn test_parse_ipc_command_aliases() {
        assert!(matches!(
            parse_ipc_command("toggle"),
            Some(IpcCommand::Toggle)
        ));
        assert!(matches!(
            parse_ipc_command("ptt-down"),
            Some(IpcCommand::PttDown)
        ));
        assert!(matches!(
            parse_ipc_command("press"),
            Some(IpcCommand::PttDown)
        ));
        assert!(matches!(
            parse_ipc_command("ptt-up"),
            Some(IpcCommand::PttUp)
        ));
        assert!(matches!(
            parse_ipc_command("release"),
            Some(IpcCommand::PttUp)
        ));
        assert!(matches!(
            parse_ipc_command("cancel"),
            Some(IpcCommand::Cancel)
        ));
        assert!(matches!(
            parse_ipc_command("status"),
            Some(IpcCommand::Status)
        ));
        assert!(matches!(
            parse_ipc_command("reload-config"),
            Some(IpcCommand::ReloadConfig)
        ));
        assert!(matches!(
            parse_ipc_command("reload_config"),
            Some(IpcCommand::ReloadConfig)
        ));
        assert!(matches!(
            parse_ipc_command("preview-hud"),
            Some(IpcCommand::PreviewHud)
        ));
        assert!(matches!(
            parse_ipc_command("preview"),
            Some(IpcCommand::PreviewHud)
        ));
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_ipc_roundtrip_newline_framed() {
        let temp_dir = std::env::temp_dir();
        let sock_path = temp_dir.join(format!("ow_test_ipc_{}.sock", std::process::id()));
        let sock_str = sock_path.to_string_lossy().to_string();

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let server = IpcServer::new(sock_str.clone(), tx);
        let server_handle = tokio::spawn(server.run());

        // Wait briefly for server to bind
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        tokio::spawn(async move { while let Some(_cmd) = rx.recv().await {} });

        let resp = send_ipc_command(&sock_str, IpcCommand::Toggle)
            .await
            .unwrap();
        assert_eq!(resp.status, "ok");

        #[cfg(target_os = "linux")]
        {
            let sock_str_clone = sock_str.clone();
            let resp_sync = tokio::task::spawn_blocking(move || {
                send_ipc_command_sync(&sock_str_clone, IpcCommand::Status)
            })
            .await
            .unwrap()
            .unwrap();
            assert_eq!(resp_sync.status, "ok");
        }

        server_handle.abort();
        let _ = std::fs::remove_file(sock_path);
    }
}
