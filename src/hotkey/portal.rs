#[cfg(target_os = "linux")]
use anyhow::Result;
#[cfg(target_os = "linux")]
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use tokio::sync::mpsc::Sender;
#[cfg(target_os = "linux")]
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
#[cfg(target_os = "linux")]
use zbus::{connection::Builder, proxy};

#[cfg(target_os = "linux")]
#[proxy(
    interface = "org.freedesktop.portal.GlobalShortcuts",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait GlobalShortcuts {
    fn create_session(
        &self,
        options: &HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;

    fn bind_shortcuts(
        &self,
        session_handle: &OwnedObjectPath,
        shortcuts: &[(&str, HashMap<&str, Value<'_>>)],
        parent_window: &str,
        options: &HashMap<&str, Value<'_>>,
    ) -> zbus::Result<HashMap<String, OwnedValue>>;

    #[zbus(signal)]
    fn activated(
        &self,
        session_handle: OwnedObjectPath,
        shortcut_id: String,
        timestamp: u64,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    fn deactivated(
        &self,
        session_handle: OwnedObjectPath,
        shortcut_id: String,
        timestamp: u64,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;
}

use super::ipc::IpcCommand;

pub struct PortalShortcutListener {
    cmd_tx: Sender<IpcCommand>,
}

impl PortalShortcutListener {
    pub fn new(cmd_tx: Sender<IpcCommand>) -> Self {
        Self { cmd_tx }
    }

    #[cfg(target_os = "linux")]
    pub async fn run(self) -> Result<()> {
        let connection = match Builder::session() {
            Ok(b) => match b.build().await {
                Ok(conn) => conn,
                Err(err) => {
                    tracing::warn!("Could not connect to D-Bus session: {err}. GlobalShortcuts portal disabled.");
                    return Ok(());
                }
            },
            Err(err) => {
                tracing::warn!("Failed to get D-Bus session builder: {err}");
                return Ok(());
            }
        };

        let proxy = match GlobalShortcutsProxy::new(&connection).await {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!("Failed to create GlobalShortcuts proxy: {err}");
                return Ok(());
            }
        };

        let mut session_opts: HashMap<&str, Value<'_>> = HashMap::new();
        let session_token = format!("openwhisper_{}", std::process::id());
        session_opts.insert("session_handle_token", Value::from(&session_token));

        let session_handle = match proxy.create_session(&session_opts).await {
            Ok(h) => h,
            Err(err) => {
                tracing::info!("GlobalShortcuts portal session creation not available: {err}. Using IPC shortcuts.");
                return Ok(());
            }
        };

        let mut shortcut_opts = HashMap::new();
        shortcut_opts.insert("description", Value::from("OpenWhisper Dictation Hotkey"));
        shortcut_opts.insert("preferred_trigger", Value::from("Control+space"));

        let shortcuts_to_bind = [("dictate", shortcut_opts)];
        let bind_opts: HashMap<&str, Value<'_>> = HashMap::new();

        if let Err(err) = proxy
            .bind_shortcuts(&session_handle, &shortcuts_to_bind, "", &bind_opts)
            .await
        {
            tracing::info!("Could not bind shortcuts via XDG portal: {err}. (KDE custom shortcut or IPC remains active)");
            return Ok(());
        }

        tracing::info!("Registered global shortcut via XDG Desktop Portal");

        use futures_util::StreamExt;
        let mut activated_stream = match proxy.receive_activated().await {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!("Failed to subscribe to Activated portal signal: {err}");
                return Ok(());
            }
        };
        let mut deactivated_stream = match proxy.receive_deactivated().await {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!("Failed to subscribe to Deactivated portal signal: {err}");
                return Ok(());
            }
        };

        let cmd_tx_act = self.cmd_tx.clone();
        let cmd_tx_deact = self.cmd_tx.clone();

        tokio::spawn(async move {
            while let Some(signal) = activated_stream.next().await {
                if let Ok(args) = signal.args() {
                    if args.shortcut_id == "dictate" {
                        let _ = cmd_tx_act.send(IpcCommand::PttDown).await;
                    }
                }
            }
        });

        tokio::spawn(async move {
            while let Some(signal) = deactivated_stream.next().await {
                if let Ok(args) = signal.args() {
                    if args.shortcut_id == "dictate" {
                        let _ = cmd_tx_deact.send(IpcCommand::PttUp).await;
                    }
                }
            }
        });

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn run(self) -> Result<()> {
        Ok(())
    }
}
