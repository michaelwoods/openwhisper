use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(target_os = "linux")]
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use tokio::sync::mpsc::Sender;

#[cfg(target_os = "linux")]
use evdev::{Device, EventType, Key};

use super::ipc::IpcCommand;

pub struct EvdevListenerHandle {
    running: Arc<AtomicBool>,
}

impl EvdevListenerHandle {
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for EvdevListenerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "linux")]
pub fn start_evdev_listener(key_name: &str, cmd_tx: Sender<IpcCommand>) -> Option<EvdevListenerHandle> {
    let target_key = match crate::config::parse_evdev_key(key_name) {
        Some(k) => k,
        None => {
            tracing::warn!(
                "Could not resolve evdev key '{}'. Supported examples: 'KEY_HELP', 'KEY_MICMUTE', 'KEY_F20'. Hardware hotkey listener disabled.",
                key_name
            );
            return None;
        }
    };

    tracing::info!(
        "Starting Linux evdev hardware hotkey monitor for key {:?} (scancode {})",
        target_key,
        target_key.code()
    );

    let running = Arc::new(AtomicBool::new(true));
    let monitored_paths: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));

    let sup_running = running.clone();
    let sup_monitored = monitored_paths.clone();
    let sup_cmd_tx = cmd_tx.clone();

    // Supervisor thread: periodically enumerates /dev/input to find all devices supporting the target key
    // This immediately picks up internal keyboards and dynamically supports hotplugged USB/Bluetooth keyboards
    let builder = thread::Builder::new().name("openwhisper-evdev-sup".into());
    let _ = builder.spawn(move || {
        while sup_running.load(Ordering::Relaxed) {
            scan_and_attach_devices(target_key, &sup_monitored, &sup_running, &sup_cmd_tx);

            // Re-scan every 4 seconds for hotplugged input devices
            for _ in 0..40 {
                if !sup_running.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        tracing::debug!("evdev supervisor loop terminated");
    });

    Some(EvdevListenerHandle { running })
}

#[cfg(target_os = "linux")]
fn scan_and_attach_devices(
    target_key: Key,
    monitored_paths: &Arc<Mutex<HashSet<PathBuf>>>,
    running: &Arc<AtomicBool>,
    cmd_tx: &Sender<IpcCommand>,
) {
    let devices = match evdev::enumerate() {
        iter => iter,
    };

    for (path, device) in devices {
        // Exclude our own virtual keyboard injector to avoid loopback
        if let Some(name) = device.name() {
            if name.contains("OpenWhisper") {
                continue;
            }
        }

        let is_candidate = device
            .supported_keys()
            .map_or(false, |keys| keys.contains(target_key));

        if !is_candidate {
            continue;
        }

        let mut lock = match monitored_paths.lock() {
            Ok(l) => l,
            Err(_) => return,
        };

        if lock.contains(&path) {
            continue;
        }

        // Mark as actively monitored
        lock.insert(path.clone());
        drop(lock);

        let dev_name = device.name().unwrap_or("Unknown").to_string();
        tracing::info!(
            "Attaching evdev Push-to-Talk listener to '{}' at {:?}",
            dev_name,
            path
        );

        let thread_running = running.clone();
        let thread_monitored = monitored_paths.clone();
        let thread_cmd_tx = cmd_tx.clone();
        let thread_path = path.clone();

        let builder = thread::Builder::new().name(format!("evdev-{}", thread_path.file_name().and_then(|f| f.to_str()).unwrap_or("kbd")));
        let spawn_res = builder.spawn(move || {
            read_device_events(
                device,
                target_key,
                thread_path.clone(),
                thread_running,
                thread_monitored,
                thread_cmd_tx,
            );
        });

        if let Err(e) = spawn_res {
            tracing::warn!("Failed to spawn evdev worker thread for {:?}: {e}", path);
            if let Ok(mut l) = monitored_paths.lock() {
                l.remove(&path);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn read_device_events(
    mut device: Device,
    target_key: Key,
    path: PathBuf,
    running: Arc<AtomicBool>,
    monitored_paths: Arc<Mutex<HashSet<PathBuf>>>,
    cmd_tx: Sender<IpcCommand>,
) {
    while running.load(Ordering::Relaxed) {
        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if ev.event_type() == EventType::KEY && ev.code() == target_key.code() {
                        match ev.value() {
                            1 => {
                                tracing::debug!("evdev hotkey pressed on {:?}", path);
                                let _ = cmd_tx.blocking_send(IpcCommand::PttDown);
                            }
                            0 => {
                                tracing::debug!("evdev hotkey released on {:?}", path);
                                let _ = cmd_tx.blocking_send(IpcCommand::PttUp);
                            }
                            2 => {
                                // Key repeat event from kernel, ignore to avoid re-triggering
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!("evdev device {:?} read ended or disconnected: {e}", path);
                break;
            }
        }
    }

    if let Ok(mut l) = monitored_paths.lock() {
        l.remove(&path);
    }
    tracing::debug!("evdev reader thread for {:?} exited", path);
}

#[cfg(not(target_os = "linux"))]
pub fn start_evdev_listener(_key_name: &str, _cmd_tx: tokio::sync::mpsc::Sender<IpcCommand>) -> Option<EvdevListenerHandle> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_lifecycle() {
        let running = Arc::new(AtomicBool::new(true));
        let handle = EvdevListenerHandle { running: running.clone() };
        assert!(handle.is_running());
        handle.stop();
        assert!(!handle.is_running());
    }
}
