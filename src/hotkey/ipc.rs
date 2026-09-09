use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonState {
    Idle,
    Recording { device: Option<String> },
    Transcribing,
    Degraded { reason: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum IpcEvent {
    StateChanged {
        state: DaemonState,
    },
    AudioLevel {
        rms: f32,
    },
    TranscriptionCompleted {
        text: String,
        duration_secs: f32,
    },
    Diagnostics {
        server_url: String,
        active_device: Option<String>,
        last_latency: Option<f32>,
        last_error: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpcCommand {
    Toggle,
    PttDown,
    PttUp,
    Cancel,
    Status,
    ReloadConfig,
    PreviewHud,
    Subscribe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub status: String,
    pub message: String,
}

pub struct IpcServer {
    socket_path: String,
    cmd_tx: Sender<IpcCommand>,
    event_tx: Option<tokio::sync::broadcast::Sender<IpcEvent>>,
}

impl IpcServer {
    pub fn new(
        socket_path: String,
        cmd_tx: Sender<IpcCommand>,
        event_tx: Option<tokio::sync::broadcast::Sender<IpcEvent>>,
    ) -> Self {
        Self {
            socket_path,
            cmd_tx,
            event_tx,
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
                    let event_tx = self.event_tx.clone();
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
                                match cmd {
                                    Some(IpcCommand::Subscribe) => {
                                        // Acknowledge subscription
                                        let res = IpcResponse {
                                            status: "ok".to_string(),
                                            message: "subscribed".to_string(),
                                        };
                                        if let Ok(mut res_bytes) = serde_json::to_vec(&res) {
                                            res_bytes.push(b'\n');
                                            let _ = writer.write_all(&res_bytes).await;
                                        }

                                        // If an event broadcaster is provided, stream events until client disconnects
                                        if let Some(ref tx) = event_tx {
                                            let mut rx = tx.subscribe();
                                            line.clear();
                                            loop {
                                                tokio::select! {
                                                    read_res = buf_reader.read_line(&mut line) => {
                                                        match read_res {
                                                            Ok(0) | Err(_) => break,
                                                            Ok(_) => {
                                                                let text = line.trim();
                                                                if !text.is_empty()
                                                                    && let Some(c) = parse_ipc_command(text)
                                                                {
                                                                    let _ = cmd_tx.send(c).await;
                                                                }
                                                                line.clear();
                                                            }
                                                        }
                                                    }
                                                    event_res = rx.recv() => {
                                                        match event_res {
                                                            Ok(event) => {
                                                                if let Ok(mut bytes) = serde_json::to_vec(&event) {
                                                                    bytes.push(b'\n');
                                                                    if writer.write_all(&bytes).await.is_err() {
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        break;
                                    }
                                    Some(c) => {
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
                                        if let Ok(mut res_bytes) = serde_json::to_vec(&res) {
                                            res_bytes.push(b'\n');
                                            let _ = writer.write_all(&res_bytes).await;
                                        }
                                    }
                                    None => {
                                        let res = IpcResponse {
                                            status: "error".to_string(),
                                            message: format!("unknown command: {}", cmd_str),
                                        };
                                        if let Ok(mut res_bytes) = serde_json::to_vec(&res) {
                                            res_bytes.push(b'\n');
                                            let _ = writer.write_all(&res_bytes).await;
                                        }
                                    }
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
        "subscribe" | "events" | "listen" => Some(IpcCommand::Subscribe),
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

/// Bidirectional streaming IPC client connected to the OpenWhisper daemon.
/// Subscribes to daemon events (state changes, audio RMS levels, transcriptions)
/// and allows sending commands back over the same persistent connection.
pub struct EventStream {
    reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl EventStream {
    pub async fn connect(socket_path: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket_path).await.with_context(|| {
            format!("Failed to connect to OpenWhisper daemon at {}", socket_path)
        })?;
        let (reader, mut writer) = stream.into_split();
        let mut req = serde_json::to_vec(&IpcCommand::Subscribe)?;
        req.push(b'\n');
        writer.write_all(&req).await?;

        let mut buf_reader = tokio::io::BufReader::new(reader);
        let mut first_line = String::new();
        buf_reader
            .read_line(&mut first_line)
            .await
            .context("Failed to receive subscription confirmation from OpenWhisper daemon")?;

        Ok(Self {
            reader: buf_reader,
            writer,
        })
    }

    pub async fn next_event(&mut self) -> Result<Option<IpcEvent>> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let event = serde_json::from_str::<IpcEvent>(trimmed)
            .with_context(|| format!("Failed to parse IpcEvent from line: {trimmed}"))?;
        Ok(Some(event))
    }

    pub async fn send_command(&mut self, cmd: IpcCommand) -> Result<()> {
        let mut payload = serde_json::to_vec(&cmd)?;
        payload.push(b'\n');
        self.writer.write_all(&payload).await?;
        Ok(())
    }
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
        let server = IpcServer::new(sock_str.clone(), tx, None);
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_ipc_event_stream_subscription() {
        let temp_dir = std::env::temp_dir();
        let sock_path = temp_dir.join(format!("ow_test_events_{}.sock", std::process::id()));
        let sock_str = sock_path.to_string_lossy().to_string();

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(10);
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let server = IpcServer::new(sock_str.clone(), cmd_tx, Some(event_tx.clone()));
        let server_handle = tokio::spawn(server.run());

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        tokio::spawn(async move { while let Some(_cmd) = cmd_rx.recv().await {} });

        // Connect subscriber stream
        let mut stream = EventStream::connect(&sock_str)
            .await
            .expect("connect stream");

        // Broadcast test events
        event_tx
            .send(IpcEvent::StateChanged {
                state: DaemonState::Recording {
                    device: Some("Mic 1".into()),
                },
            })
            .expect("send state event");

        event_tx
            .send(IpcEvent::AudioLevel { rms: 0.42 })
            .expect("send audio level");

        // Receive events on subscriber
        let ev1 = stream.next_event().await.unwrap().expect("event 1");
        assert_eq!(
            ev1,
            IpcEvent::StateChanged {
                state: DaemonState::Recording {
                    device: Some("Mic 1".into())
                }
            }
        );

        let ev2 = stream.next_event().await.unwrap().expect("event 2");
        assert_eq!(ev2, IpcEvent::AudioLevel { rms: 0.42 });

        server_handle.abort();
        let _ = std::fs::remove_file(sock_path);
    }
}
