use anyhow::{Context, Result};
use std::thread::sleep;
use std::time::Duration;

#[cfg(target_os = "linux")]
use evdev::{
    AttributeSet, EventType, InputEvent, Key,
    uinput::{VirtualDevice, VirtualDeviceBuilder},
};

pub struct TextInjector {
    #[cfg(target_os = "linux")]
    virtual_device: Option<VirtualDevice>,
}

impl Default for TextInjector {
    fn default() -> Self {
        Self::new()
    }
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
        // Navigation & control
        keys.insert(Key::KEY_LEFTCTRL);
        keys.insert(Key::KEY_LEFTSHIFT);
        keys.insert(Key::KEY_V);
        keys.insert(Key::KEY_INSERT);
        keys.insert(Key::KEY_SPACE);
        keys.insert(Key::KEY_ENTER);
        keys.insert(Key::KEY_TAB);
        keys.insert(Key::KEY_BACKSPACE);

        // Letters A-Z
        let letter_keys = [
            Key::KEY_A,
            Key::KEY_B,
            Key::KEY_C,
            Key::KEY_D,
            Key::KEY_E,
            Key::KEY_F,
            Key::KEY_G,
            Key::KEY_H,
            Key::KEY_I,
            Key::KEY_J,
            Key::KEY_K,
            Key::KEY_L,
            Key::KEY_M,
            Key::KEY_N,
            Key::KEY_O,
            Key::KEY_P,
            Key::KEY_Q,
            Key::KEY_R,
            Key::KEY_S,
            Key::KEY_T,
            Key::KEY_U,
            Key::KEY_V,
            Key::KEY_W,
            Key::KEY_X,
            Key::KEY_Y,
            Key::KEY_Z,
        ];
        for k in letter_keys {
            keys.insert(k);
        }

        // Numbers 0-9
        let digit_keys = [
            Key::KEY_0,
            Key::KEY_1,
            Key::KEY_2,
            Key::KEY_3,
            Key::KEY_4,
            Key::KEY_5,
            Key::KEY_6,
            Key::KEY_7,
            Key::KEY_8,
            Key::KEY_9,
        ];
        for k in digit_keys {
            keys.insert(k);
        }

        // Common punctuation & symbols
        let punct_keys = [
            Key::KEY_MINUS,
            Key::KEY_EQUAL,
            Key::KEY_LEFTBRACE,
            Key::KEY_RIGHTBRACE,
            Key::KEY_SEMICOLON,
            Key::KEY_APOSTROPHE,
            Key::KEY_GRAVE,
            Key::KEY_BACKSLASH,
            Key::KEY_COMMA,
            Key::KEY_DOT,
            Key::KEY_SLASH,
        ];
        for k in punct_keys {
            keys.insert(k);
        }

        match VirtualDeviceBuilder::new() {
            Ok(builder) => match builder
                .name("OpenWhisper Virtual Keyboard")
                .with_keys(&keys)
            {
                Ok(b) => match b.build() {
                    Ok(dev) => {
                        tracing::info!(
                            "Initialized /dev/uinput virtual keyboard for direct text typing & pasting"
                        );
                        Some(dev)
                    }
                    Err(err) => {
                        tracing::warn!(
                            "Failed to build uinput virtual device: {err}. Direct keystroke paste will be disabled."
                        );
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

            tracing::warn!(
                "No active virtual keyboard or input tool available to simulate Ctrl+V. Text is saved in clipboard."
            );
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

    /// Types text directly into the focused window character-by-character using
    /// kernel uinput virtual keyboard (fastest, zero-fork), Wayland virtual-keyboard
    /// protocol (wtype), or kernel uinput (ydotool).
    pub fn type_text(&mut self, text: &str) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            // 1. Direct /dev/uinput virtual keyboard typing (Fastest: ~2ms per char, zero process forks)
            if let Some(ref mut dev) = self.virtual_device {
                if can_type_with_uinput(text) {
                    tracing::info!(
                        "Typing {} characters directly via /dev/uinput virtual keyboard",
                        text.len()
                    );
                    for c in text.chars() {
                        if let Some((key, shift_needed)) = char_to_evdev_key(c) {
                            if shift_needed {
                                dev.emit(&[InputEvent::new(
                                    EventType::KEY,
                                    Key::KEY_LEFTSHIFT.0,
                                    1,
                                )])
                                .context("Failed to emit Shift down")?;
                            }
                            dev.emit(&[InputEvent::new(EventType::KEY, key.0, 1)])
                                .context("Failed to emit key down")?;
                            sleep(Duration::from_millis(2));
                            dev.emit(&[InputEvent::new(EventType::KEY, key.0, 0)])
                                .context("Failed to emit key up")?;
                            if shift_needed {
                                dev.emit(&[InputEvent::new(
                                    EventType::KEY,
                                    Key::KEY_LEFTSHIFT.0,
                                    0,
                                )])
                                .context("Failed to emit Shift up")?;
                            }
                            sleep(Duration::from_millis(2));
                        }
                    }
                    // Ensure modifier is cleared
                    let _ = dev.emit(&[InputEvent::new(EventType::KEY, Key::KEY_LEFTSHIFT.0, 0)]);
                    return Ok(());
                } else {
                    tracing::info!(
                        "Text contains non-ASCII or unmapped characters; delegating to Wayland wtype"
                    );
                }
            }

            // 2. Try Wayland native wtype (virtual-keyboard-v1)
            if crate::output::clipboard::has_command_in_path("wtype")
                && let Ok(status) = std::process::Command::new("wtype")
                    .args(["--", text])
                    .status()
                && status.success()
            {
                tracing::info!("Emitted direct keystroke text input via wtype");
                return Ok(());
            }

            // 3. Try ydotool type
            if crate::output::clipboard::has_command_in_path("ydotool")
                && let Ok(status) = std::process::Command::new("ydotool")
                    .args(["type", "--", text])
                    .status()
                && status.success()
            {
                tracing::info!("Emitted direct keystroke text input via ydotool");
                return Ok(());
            }

            // 4. Fallback to clipboard paste
            tracing::warn!(
                "Direct typing tool (wtype / ydotool) unavailable or failed; falling back to clipboard paste"
            );
            self.paste_clipboard(0)
        }

        #[cfg(not(target_os = "linux"))]
        {
            self.paste_clipboard(0)
        }
    }
}

/// Checks whether all characters in `text` can be directly emitted via standard evdev uinput keys.
pub fn can_type_with_uinput(text: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        !text.is_empty() && text.chars().all(|c| char_to_evdev_key(c).is_some())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = text;
        false
    }
}

/// Maps a character to its corresponding evdev Key and whether Shift must be held.
#[cfg(target_os = "linux")]
pub fn char_to_evdev_key(c: char) -> Option<(Key, bool)> {
    match c {
        'a'..='z' => {
            let key = match c {
                'a' => Key::KEY_A,
                'b' => Key::KEY_B,
                'c' => Key::KEY_C,
                'd' => Key::KEY_D,
                'e' => Key::KEY_E,
                'f' => Key::KEY_F,
                'g' => Key::KEY_G,
                'h' => Key::KEY_H,
                'i' => Key::KEY_I,
                'j' => Key::KEY_J,
                'k' => Key::KEY_K,
                'l' => Key::KEY_L,
                'm' => Key::KEY_M,
                'n' => Key::KEY_N,
                'o' => Key::KEY_O,
                'p' => Key::KEY_P,
                'q' => Key::KEY_Q,
                'r' => Key::KEY_R,
                's' => Key::KEY_S,
                't' => Key::KEY_T,
                'u' => Key::KEY_U,
                'v' => Key::KEY_V,
                'w' => Key::KEY_W,
                'x' => Key::KEY_X,
                'y' => Key::KEY_Y,
                'z' => Key::KEY_Z,
                _ => unreachable!(),
            };
            Some((key, false))
        }
        'A'..='Z' => {
            let key = match c {
                'A' => Key::KEY_A,
                'B' => Key::KEY_B,
                'C' => Key::KEY_C,
                'D' => Key::KEY_D,
                'E' => Key::KEY_E,
                'F' => Key::KEY_F,
                'G' => Key::KEY_G,
                'H' => Key::KEY_H,
                'I' => Key::KEY_I,
                'J' => Key::KEY_J,
                'K' => Key::KEY_K,
                'L' => Key::KEY_L,
                'M' => Key::KEY_M,
                'N' => Key::KEY_N,
                'O' => Key::KEY_O,
                'P' => Key::KEY_P,
                'Q' => Key::KEY_Q,
                'R' => Key::KEY_R,
                'S' => Key::KEY_S,
                'T' => Key::KEY_T,
                'U' => Key::KEY_U,
                'V' => Key::KEY_V,
                'W' => Key::KEY_W,
                'X' => Key::KEY_X,
                'Y' => Key::KEY_Y,
                'Z' => Key::KEY_Z,
                _ => unreachable!(),
            };
            Some((key, true))
        }
        '0'..='9' => {
            let key = match c {
                '0' => Key::KEY_0,
                '1' => Key::KEY_1,
                '2' => Key::KEY_2,
                '3' => Key::KEY_3,
                '4' => Key::KEY_4,
                '5' => Key::KEY_5,
                '6' => Key::KEY_6,
                '7' => Key::KEY_7,
                '8' => Key::KEY_8,
                '9' => Key::KEY_9,
                _ => unreachable!(),
            };
            Some((key, false))
        }
        ' ' => Some((Key::KEY_SPACE, false)),
        '\t' => Some((Key::KEY_TAB, false)),
        '\n' | '\r' => Some((Key::KEY_ENTER, false)),
        '-' => Some((Key::KEY_MINUS, false)),
        '_' => Some((Key::KEY_MINUS, true)),
        '=' => Some((Key::KEY_EQUAL, false)),
        '+' => Some((Key::KEY_EQUAL, true)),
        '[' => Some((Key::KEY_LEFTBRACE, false)),
        '{' => Some((Key::KEY_LEFTBRACE, true)),
        ']' => Some((Key::KEY_RIGHTBRACE, false)),
        '}' => Some((Key::KEY_RIGHTBRACE, true)),
        ';' => Some((Key::KEY_SEMICOLON, false)),
        ':' => Some((Key::KEY_SEMICOLON, true)),
        '\'' => Some((Key::KEY_APOSTROPHE, false)),
        '"' => Some((Key::KEY_APOSTROPHE, true)),
        '`' => Some((Key::KEY_GRAVE, false)),
        '~' => Some((Key::KEY_GRAVE, true)),
        '\\' => Some((Key::KEY_BACKSLASH, false)),
        '|' => Some((Key::KEY_BACKSLASH, true)),
        ',' => Some((Key::KEY_COMMA, false)),
        '<' => Some((Key::KEY_COMMA, true)),
        '.' => Some((Key::KEY_DOT, false)),
        '>' => Some((Key::KEY_DOT, true)),
        '/' => Some((Key::KEY_SLASH, false)),
        '?' => Some((Key::KEY_SLASH, true)),
        '!' => Some((Key::KEY_1, true)),
        '@' => Some((Key::KEY_2, true)),
        '#' => Some((Key::KEY_3, true)),
        '$' => Some((Key::KEY_4, true)),
        '%' => Some((Key::KEY_5, true)),
        '^' => Some((Key::KEY_6, true)),
        '&' => Some((Key::KEY_7, true)),
        '*' => Some((Key::KEY_8, true)),
        '(' => Some((Key::KEY_9, true)),
        ')' => Some((Key::KEY_0, true)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_to_evdev_key_letters_and_digits() {
        assert_eq!(char_to_evdev_key('a'), Some((Key::KEY_A, false)));
        assert_eq!(char_to_evdev_key('Z'), Some((Key::KEY_Z, true)));
        assert_eq!(char_to_evdev_key('5'), Some((Key::KEY_5, false)));
        assert_eq!(char_to_evdev_key(' '), Some((Key::KEY_SPACE, false)));
        assert_eq!(char_to_evdev_key('\n'), Some((Key::KEY_ENTER, false)));
    }

    #[test]
    fn test_char_to_evdev_key_symbols_and_shifts() {
        assert_eq!(char_to_evdev_key('!'), Some((Key::KEY_1, true)));
        assert_eq!(char_to_evdev_key('@'), Some((Key::KEY_2, true)));
        assert_eq!(char_to_evdev_key('-'), Some((Key::KEY_MINUS, false)));
        assert_eq!(char_to_evdev_key('_'), Some((Key::KEY_MINUS, true)));
        assert_eq!(char_to_evdev_key('?'), Some((Key::KEY_SLASH, true)));
        assert_eq!(char_to_evdev_key(':'), Some((Key::KEY_SEMICOLON, true)));
    }

    #[test]
    fn test_can_type_with_uinput() {
        assert!(can_type_with_uinput("Hello, World! 123"));
        assert!(can_type_with_uinput("cargo build --release\n"));
        // Empty text returns false
        assert!(!can_type_with_uinput(""));
        // Non-ASCII (emojis or accented characters) return false
        assert!(!can_type_with_uinput("Café"));
        assert!(!can_type_with_uinput("Hello 🚀"));
    }
}
