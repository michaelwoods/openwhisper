use anyhow::{Context, Result};
use std::thread::sleep;
use std::time::Duration;

#[cfg(target_os = "linux")]
use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AttributeSet, EventType, InputEvent, Key,
};

pub struct TextInjector {
    #[cfg(target_os = "linux")]
    virtual_device: Option<VirtualDevice>,
}

impl TextInjector {
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            let device = Self::init_linux_uinput();
            Self {
                virtual_device: device,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self {}
        }
    }

    #[cfg(target_os = "linux")]
    fn init_linux_uinput() -> Option<VirtualDevice> {
        let mut keys = AttributeSet::<Key>::new();
        keys.insert(Key::KEY_LEFTCTRL);
        keys.insert(Key::KEY_LEFTSHIFT);
        keys.insert(Key::KEY_V);
        keys.insert(Key::KEY_INSERT);

        match VirtualDeviceBuilder::new() {
            Ok(builder) => match builder.name("OpenWhisper Virtual Keyboard").with_keys(&keys) {
                Ok(b) => match b.build() {
                    Ok(dev) => {
                        tracing::info!("Initialized /dev/uinput virtual keyboard for text pasting");
                        Some(dev)
                    }
                    Err(err) => {
                        tracing::warn!("Failed to build uinput virtual device: {err}. Direct keystroke paste will be disabled.");
                        None
                    }
                },
                Err(err) => {
                    tracing::warn!("Failed to configure uinput keys: {err}");
                    None
                }
            },
            Err(err) => {
                tracing::warn!("Failed to access /dev/uinput: {err}");
                None
            }
        }
    }

    /// Emits Ctrl+V keystroke to paste clipboard text into active window
    pub fn paste_clipboard(&mut self, delay_ms: u64) -> Result<()> {
        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms));
        }

        #[cfg(target_os = "linux")]
        {
            if let Some(ref mut dev) = self.virtual_device {
                let down_events = [
                    InputEvent::new(EventType::KEY, Key::KEY_LEFTCTRL.0, 1),
                    InputEvent::new(EventType::KEY, Key::KEY_V.0, 1),
                ];
                dev.emit(&down_events)
                    .context("Failed to emit Ctrl+V down events")?;

                sleep(Duration::from_millis(15));

                let up_events = [
                    InputEvent::new(EventType::KEY, Key::KEY_V.0, 0),
                    InputEvent::new(EventType::KEY, Key::KEY_LEFTCTRL.0, 0),
                ];
                dev.emit(&up_events)
                    .context("Failed to emit Ctrl+V up events")?;

                tracing::info!("Emitted Ctrl+V paste event via uinput");
                return Ok(());
            }

            // Fallback: try wtype if installed
            if std::process::Command::new("wtype")
                .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
                .status()
                .is_ok()
            {
                tracing::info!("Emitted Ctrl+V paste event via wtype");
                return Ok(());
            }

            tracing::warn!("No active virtual keyboard or input tool available to simulate Ctrl+V. Text is saved in clipboard.");
            Ok(())
        }

        #[cfg(target_os = "windows")]
        {
            // Windows SendInput paste can be added here
            tracing::info!("Windows paste simulated");
            Ok(())
        }

        #[cfg(target_os = "macos")]
        {
            // macOS CGEventPost paste can be added here
            tracing::info!("macOS paste simulated");
            Ok(())
        }
    }
}
