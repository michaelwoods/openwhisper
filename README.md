# OpenWhisper 🎙️

A fast, lightweight, privacy-focused, cross-platform speech-to-text dictation assistant written in Rust. Designed as an open-source, multi-platform alternative to Superwhisper, tailored for Linux on Wayland (Fedora, KDE Plasma 6) and architected for cross-platform support across macOS, Windows, and mobile.

Transcriptions are powered by any OpenAI-compatible speech-to-text endpoint, including your local [OpenVINO Model Server (OVMS)](https://docs.openvino.ai/2026/model-server/ovms_demos_audio.html), Whisper.cpp, vLLM, or cloud OpenAI/Groq endpoints.

---

## Shipped Features & Capabilities

- ⚡ **Dual-Mode Hotkey State Machine**:
  - **Push-To-Talk (PTT)**: Press and hold key to speak, release to immediately transcribe and paste.
  - **Hands-Free Toggle Mode**: Brief tap to start recording hands-free, tap again to finish and paste.
  - Seamlessly unified state machine with configurable threshold (`ptt_threshold_ms`, default: `350ms`).
- 🖥️ **Minimal Floating Status Overlay (HUD)**:
  - Frameless, translucent always-on-top pill widget rendered via `eframe` (Wayland + Glow).
  - Non-focus-stealing (`with_active(false)`) to maintain uninterrupted keyboard focus in your active application.
  - **Dynamic Real-Time Equalizer**: 5-bar animated audio visualizer responding dynamically to speech RMS amplitude in real time.
  - **Elapsed Recording Timer**: Monospaced duration counter (`00:03.2`).
  - **State Transitions**: Instant visual feedback for Recording (pulsing red dot), Transcribing (cyan acoustic orbit), Done (green checkmark and text snippet), and Error.
  - **Smooth Auto-Hide**: Automatically fades out smoothly when dictation completes.
  - Interactive standalone demo available via `openwhisper hud-demo`.
- 🖥️ **StatusNotifierItem System Tray**:
  - Native KDE Plasma / Wayland D-Bus system tray item via `ksni`.
  - Dynamic state icons: Idle (slate mic), Recording (pulsing red indicator), Transcribing (cyan acoustic orbit), and Error (amber alert).
  - Embedded in-memory ARGB fallback pixmaps for instant rendering regardless of system icon indexing.
  - Context menu: Instant Toggle Dictation, Open Settings..., and Quit.
- ⚙️ **Native GUI Configuration Panel (`openwhisper config-gui`)**:
  - Pure-Rust graphical window built with `egui` / `eframe` (Glow + Wayland).
  - **Live Connection & Latency Tester**: Tests STT endpoint responsiveness and model inference in real time.
  - **Interactive Audio Feedback**: Volume slider with live preview tone playback.
  - **Floating HUD Settings**: Toggle HUD overlay and select screen position (`BottomCenter`, `TopCenter`, etc.).
  - **Formatting & Vocabulary Manager**: Configure casing modes and add/remove custom bias words.
  - **In-Memory IPC Reload**: Clicking "Save & Apply" persists `config.toml` and instantly reloads the running background daemon via IPC without restarting processes or dropping D-Bus/audio streams.
- 🔊 **In-Memory Audio Feedback (Earcons)**:
  - Pure mathematical sine-wave PCM audio synthesis with smooth attack/decay envelopes (zero disk files).
  - Acoustic state cues: Start recording (rising chime), Stop recording (descending tone), Transcribed (soft confirmation blip), and Error.
  - Fully adjustable volume or toggleable in settings.
- 🎙️ **Voice Activity Detection (VAD)**:
  - Real-time RMS silence gating running on a concurrent monitor thread without interrupting audio capture.
  - Automatically finalizes dictation and transcribes when trailing silence exceeds timeout (default: `1800ms`), preventing runaway recordings.
- ✍️ **Formatting Modes & Vocabulary Biasing**:
  - **Formatting Modes**:
    - `Standard`: Natural language with punctuation and proper capitalization.
    - `snake_case`: Auto-converts spoken words to programming identifiers (`user_auth_token`).
    - `camelCase`: Auto-converts to camelCase (`parseApiResponse`).
    - `kebab-case`: Auto-converts to kebab-case (`k8s-pod-deployment`).
    - `Raw`: Verbatim transcription without automated punctuation.
  - **Domain Vocabulary Biasing**: Injects specialized technical terms (e.g. `Rust`, `Wayland`, `OpenVINO`, `cpal`) directly into Whisper's decoding prompt.
- 📋 **Wayland Text Injection & Clipboard**:
  - Automatically populates the clipboard (`wl-copy` with `arboard` fallback).
  - Emits virtual keyboard keystroke (`Ctrl+V`) via `/dev/uinput` to paste directly into the active cursor position.
  - Pure-Rust system PATH resolution (`std::env::split_paths`) with zero external `which` subprocess overhead.
- 🚀 **Local Inference & Zero-Disk Audio Pipeline**:
  - Audio captured via `cpal`, resampled in-memory to 16kHz mono 16-bit PCM, and packaged into WAV via `hound` inside a `Cursor<Vec<u8>>`.
  - Zero disk writes to `/tmp` for maximum throughput (<100ms latency overhead) and absolute user privacy.
- 🛠️ **Automated Setup Phase**:
  - One-command setup via `openwhisper setup`, `make install`, or `scripts/install.sh`.
  - Automates binary installation, Freedesktop icon theme deployment, `.desktop` menu registration, and user systemd service management.

---

## Architecture Overview

```
                          ┌───────────────────────────┐
                          │  KDE / Compositor Hotkey  │
                          │   or XDG Shortcut Portal  │
                          └─────────────┬─────────────┘
                                        │
                         [openwhisper toggle / ptt / reload]
                                        │
                                        ▼
    ┌───────────────────────────────────────────────────────────────────┐
    │                       OpenWhisper Daemon                          │
    │                                                                   │
    │  ┌──────────────────┐    ┌─────────────────┐    ┌──────────────┐  │
    │  │  Hotkey Engine   │───▶│ Audio Capture   │───▶│  Resampler   │  │
    │  │  (PTT vs Toggle) │    │ (cpal / PipeWire│    │  (16kHz mono │  │
    │  └──────────────────┘    └─────────────────┘    │  WAV in-mem) │  │
    │            │                                    └──────┬───────┘  │
    │            ▼                                           │          │
    │  ┌──────────────────┐    ┌─────────────────┐           │          │
    │  │   VAD Detector   │    │ Clipboard Mgr   │◀──────────┘          │
    │  │   (RMS Gating)   │    │ (wl-copy /      │     POST /v1/audio/  │
    │  └─────────┬────────┘    │  arboard)       │     transcriptions   │
    │            │             └────────┬────────┘           │          │
    │            ▼                      ▼                    │          │
    │  ┌──────────────────┐    ┌─────────────────┐           │          │
    │  │ Floating HUD &   │    │  Text Injector  │           │          │
    │  │ Tray & Earcons   │    │ (/dev/uinput    │           │          │
    │  └──────────────────┘    │  virtual Ctrl+V)│           │          │
    │                          └─────────────────┘           │          │
    └────────────────────────────────────────────────────────┼──────────┘
                                                             │
                                                             ▼
                                              ┌────────────────────────┐
                                              │  OpenVINO Model Server │
                                              │   or OpenAI STT API    │
                                              └────────────────────────┘
```

---

## Installation & Quick Start

### 1. One-Step Automated Setup (Recommended)
Clone the repository and run:
```bash
make install
```
*(Or run `cargo run --release -- setup`)*.

This automatically:
1. Builds an optimized release binary.
2. Installs the binary to `/usr/local/bin/openwhisper` and `~/.local/bin/openwhisper`.
3. Deploys scalable vector icons to `~/.local/share/icons/hicolor/`.
4. Registers `.desktop` launcher and shortcut entries.
5. Deploys, enables, and starts the background `openwhisper.service` under `systemd --user`.

---

## Configuration Reference

OpenWhisper loads configuration from `~/.config/openwhisper/config.toml`. Generate the default template anytime with:
```bash
openwhisper init-config
```

### Complete `config.toml` Options

```toml
# Local OpenVINO Model Server or any OpenAI-compatible STT endpoint
server_url = "http://localhost:8000/v1/audio/transcriptions"
model = "whisper"

# Optional language code (e.g. "en", "de", "es", "fr")
# language = "en"

# Optional prompt guidance passed to Whisper
# prompt = "Software engineering discussion with Rust and Wayland"

# Optional API key (required for cloud endpoints like Groq or OpenAI)
# api_key = "sk-..."

# PTT threshold in milliseconds (hold >= 350ms for PTT, tap < 350ms for toggle)
ptt_threshold_ms = 350

# Output mode: "paste" (Ctrl+V into active cursor + clipboard), "clipboard_only", or "type"
output_mode = "paste"
paste_delay_ms = 60

# Desktop notifications via libnotify
show_notifications = true

# Pure in-memory acoustic feedback (earcons)
sound_feedback = true
sound_volume = 0.50

# Floating on-screen status HUD overlay
hud_enabled = true
hud_position = "bottom_center" # Options: "bottom_center", "top_center", "bottom_right", "top_right"

# Voice Activity Detection (RMS silence gating)
vad_enabled = true
vad_silence_timeout_ms = 1800
vad_energy_threshold = 0.015

# Output text formatting mode: "standard", "snake_case", "camel_case", "kebab_case", or "raw"
formatting_mode = "standard"

# Domain vocabulary biasing (injected into Whisper decoding context)
vocabulary = [
    "Rust",
    "Wayland",
    "KDE",
    "OpenVINO",
    "cpal",
    "tokio",
    "uinput",
    "Fedora",
]

# Optional specific microphone name (leave unset for system default)
# audio_device = "USB Audio Device"

# Path for daemon IPC socket
socket_path = "/run/user/1000/openwhisper.sock"
```

### In-Flight Configuration Reloading
When updating settings via the GUI panel or editing `config.toml` manually, apply changes live to the running daemon without restarting:
```bash
openwhisper reload
```
The daemon reloads its configuration, updates STT endpoints, adjusts sound volumes, updates HUD settings, toggles VAD, and updates PTT thresholds in-memory immediately.

---

## Desktop Integration & Hotkeys

### Floating HUD & System Tray
- **Floating HUD Overlay**: OpenWhisper displays a minimalist floating pill at the bottom of the screen during dictation. As you speak, a live 5-bar audio visualizer reacts to your voice volume, accompanied by an elapsed timer and state notifications. When transcription completes, the HUD smoothly fades away. Test the visual layout anytime with `openwhisper hud-demo`.
- **System Tray**: OpenWhisper runs as a StatusNotifierItem in your KDE Plasma panel or system tray. Left-click the microphone icon to toggle dictation or right-click to open Settings or Quit.
- **Settings GUI**: Run `openwhisper config-gui` or select **Settings...** from the tray menu to inspect live server latency, test microphone audio levels, toggle HUD overlay, and tune parameters visually.

### Configuring Global Hotkeys in KDE Plasma 6
1. Open **System Settings** $\rightarrow$ **Keyboard** $\rightarrow$ **Shortcuts**.
2. Click **Add New** $\rightarrow$ **Command or Script**.
3. Name: `OpenWhisper Dictate (Toggle)`
4. Command: `openwhisper toggle` (or `/usr/local/bin/openwhisper toggle`).
5. Click **Add Custom Shortcut** and assign your preferred trigger (e.g. `Meta+Space` or `Ctrl+Alt+Space` or `Pause/Break`).
6. Click **Apply**.

#### Push-to-Talk (Hold to speak)
For tools or compositors that support separate Key Down and Key Up bindings:
- Key Down: `openwhisper ptt-down`
- Key Up: `openwhisper ptt-up`

---

## CLI Usage Reference

| Command | Subcommand Alias | Description |
|---|---|---|
| `openwhisper daemon` | | Starts the background listening daemon |
| `openwhisper toggle` | | Toggles dictation on/off via IPC |
| `openwhisper ptt-down` | `down`, `press` | Triggers key down (hold to talk) |
| `openwhisper ptt-up` | `up`, `release` | Triggers key up (release to transcribe) |
| `openwhisper cancel` | | Cancels current recording |
| `openwhisper status` | | Queries daemon status |
| `openwhisper reload` | `reload-config` | Reloads daemon configuration live via IPC |
| `openwhisper hud-demo` | `hud`, `test-hud`| Launches interactive preview of the floating HUD overlay |
| `openwhisper config-gui`| `gui`, `settings`| Opens native graphical configuration panel |
| `openwhisper record` | | Standalone one-shot recording (press Enter to finish) |
| `openwhisper test-ovms` | | Tests connectivity and latency to STT endpoint |
| `openwhisper test-hotkey` | `test-key`, `sniff-keys` | Real-time evdev hardware key sniffer (inspect scancodes & hold times) |
| `openwhisper setup` | | Automated setup: installs binary, icons, desktop files, and systemd |
| `openwhisper list-devices`| | Lists available microphone audio devices |
| `openwhisper init-config`| | Initializes default `~/.config/openwhisper/config.toml` |

---

## Documentation & Future Roadmap

- 🤖 **[AGENTS.md](AGENTS.md)**: Developer and AI agent guide detailing architecture invariants, zero-disk memory constraints, and threading safety.
- 🗺️ **[ROADMAP.md](ROADMAP.md)**: Technical specifications for upcoming multi-platform backends (macOS CoreAudio/CGEventTap, Windows WASAPI/SendInput, iOS keyboard extension) and real-time streaming dictation.
