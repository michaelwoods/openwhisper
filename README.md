# OpenWhisper 🎙️

A fast, lightweight, privacy-focused, cross-platform speech-to-text dictation assistant written in Rust. Designed as an open-source, multi-platform alternative to Superwhisper, tailored for Linux on Wayland (KDE Plasma, Fedora) and architected for cross-platform support across macOS, Windows, and mobile.

Transcriptions are powered by any OpenAI-compatible speech-to-text endpoint, including your local [OpenVINO Model Server (OVMS)](https://docs.openvino.ai/2026/model-server/ovms_demos_audio.html), Whisper.cpp, vLLM, or cloud OpenAI/Groq endpoints.

---

## Features

- ⚡ **Dual-Mode Hotkey Engine**:
  - **Push-To-Talk (PTT)**: Press and hold key to speak, release to immediately transcribe and paste.
  - **Toggle Mode**: Brief tap to start recording hands-free, tap again to finish and paste.
  - Configurable hold threshold (default: `350ms`).
- ✍️ **Immediate Text Injection & Clipboard**:
  - Automatically copies transcribed text to clipboard (`wl-copy` / `arboard`).
  - Emits virtual keyboard keystroke (`Ctrl+V`) via `/dev/uinput` to paste directly into whatever input cursor or application is currently active.
- 🚀 **Local OpenVINO Model Server Integration**:
  - Built-in support for OVMS `/v1/audio/transcriptions` and `/v3/audio/transcriptions`.
  - In-memory 16kHz mono 16-bit WAV encoding via `hound` — zero disk writes for high speed and privacy.
- 🌐 **Wayland & KDE Plasma 6 Native**:
  - XDG Desktop Portal `org.freedesktop.portal.GlobalShortcuts` (v2 press & release signals).
  - Unix domain socket IPC daemon for zero-latency shortcut triggering via KDE custom shortcuts or any window manager (Hyprland, Sway, i3).
- 🔔 **Desktop Notifications**:
  - Shows friendly status alerts: `🎙️ Listening...`, `⏳ Transcribing...`, `✍️ Transcribed: ...`.

---

## Architecture Overview

```
                          ┌───────────────────────────┐
                          │  KDE / Compositor Hotkey  │
                          │   or XDG Shortcut Portal  │
                          └─────────────┬─────────────┘
                                        │
                         [openwhisper toggle / ptt]
                                        │
                                        ▼
    ┌───────────────────────────────────────────────────────────────────┐
    │                       OpenWhisper Daemon                          │
    │                                                                   │
    │  ┌──────────────────┐    ┌─────────────────┐    ┌──────────────┐  │
    │  │  Hotkey Engine   │───▶│ Audio Capture   │───▶│  Resampler   │  │
    │  │  (PTT vs Toggle) │    │ (cpal / PipeWire│    │  (16kHz mono │  │
    │  └──────────────────┘    └─────────────────┘    │  WAV in-mem) │  │
    │                                                 └──────┬───────┘  │
    │                                                        │          │
    │  ┌──────────────────┐    ┌─────────────────┐           │          │
    │  │  Text Injector   │◀───│ Clipboard Mgr   │◀──────────┘          │
    │  │ (/dev/uinput     │    │ (wl-copy /      │     POST /v1/audio/  │
    │  │  virtual Ctrl+V) │    │  arboard)       │     transcriptions   │
    │  └──────────────────┘    └─────────────────┘           │          │
    └────────────────────────────────────────────────────────┼──────────┘
                                                             │
                                                             ▼
                                              ┌────────────────────────┐
                                              │  OpenVINO Model Server │
                                              │    (local whisper)     │
                                              └────────────────────────┘
```

---

## Installation & Setup on Fedora / KDE Plasma

### 1. Build the Binary
```bash
cargo build --release
```
The optimized release binary is generated at `target/release/openwhisper`.

You can install it to your user binaries directory:
```bash
mkdir -p ~/.local/bin
cp target/release/openwhisper ~/.local/bin/
```

### 2. Configure OpenWhisper
Generate the default configuration file:
```bash
openwhisper init-config
```
Configuration file location: `~/.config/openwhisper/config.toml`:
```toml
# Local OpenVINO Model Server or OpenAI-compatible endpoint
server_url = "http://localhost:8000/v1/audio/transcriptions"
model = "whisper"

# Optional language code (e.g. "en")
# language = "en"

# PTT threshold in milliseconds (hold >= 350ms for PTT, tap < 350ms for toggle)
ptt_threshold_ms = 350

# Output mode: "paste" (Ctrl+V into active cursor + clipboard), "clipboard_only", or "type"
output_mode = "paste"
paste_delay_ms = 60

# Desktop notifications
show_notifications = true

# Path for daemon IPC socket
socket_path = "/run/user/1000/openwhisper.sock"
```

### 3. Verify Local OpenVINO Connection
Ensure OpenVINO Model Server is running with Whisper, then verify connectivity:
```bash
openwhisper test-ovms
```

---

## Configuring Global Keybindings in KDE Plasma 6

### Method 1: KDE System Settings (Recommended)
1. Open **System Settings** -> **Keyboard** -> **Shortcuts** (or search "Shortcuts").
2. Click **Add New** -> **Command or Script**.
3. Set Name: `OpenWhisper Dictate (Toggle)`
4. Set Command: `openwhisper toggle` (or full path `~/.local/bin/openwhisper toggle`).
5. Click **Add Custom Shortcut** and press your desired key (for example `Meta+Space` or `Ctrl+Alt+Space` or `Pause/Break`).
6. Click **Apply**.

Now pressing your shortcut will toggle recording on and off, pasting your spoken text directly wherever your cursor is!

### Method 2: Push-to-Talk (Hold to record)
In KDE Plasma 6 or Wayland compositors that support key release events (or mouse button tools):
- Key Down: `openwhisper ptt-down`
- Key Up: `openwhisper ptt-up`

---

## Running as a Background Service

### User systemd Service
To start OpenWhisper automatically on desktop login:
```bash
mkdir -p ~/.config/systemd/user
cp systemd/openwhisper.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now openwhisper.service
```

Check status:
```bash
systemctl --user status openwhisper.service
```

---

## CLI Usage Reference

| Command | Description |
|---|---|
| `openwhisper daemon` | Starts the background listening daemon |
| `openwhisper toggle` | Toggles dictation on/off via IPC |
| `openwhisper ptt-down` | Triggers key down (hold to talk) |
| `openwhisper ptt-up` | Triggers key up (release to transcribe) |
| `openwhisper cancel` | Cancels current recording |
| `openwhisper status` | Queries daemon status |
| `openwhisper record` | One-shot terminal recording (press Enter to stop) |
| `openwhisper test-ovms` | Tests connectivity and latency to OpenVINO whisper |
| `openwhisper list-devices` | Lists available audio input devices (microphones) |
| `openwhisper init-config` | Initializes `~/.config/openwhisper/config.toml` |

---

## Multi-Platform Roadmap

- **Linux (Wayland & X11)**: Implemented via `cpal` + `evdev`/`/dev/uinput` + `wl-copy`/`arboard` + XDG Desktop Portal.
- **macOS**: `cpal` (CoreAudio) + `rdev` / `CGEventPost` for immediate text injection + `arboard` clipboard.
- **Windows**: `cpal` (WASAPI) + `SendInput` for typing/pasting + Windows Global Hotkey API.
- **iOS**: Audio capture via AVFoundation / CoreAudio audio unit with background audio transcription.
