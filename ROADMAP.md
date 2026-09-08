# OpenWhisper Future Roadmap & Architecture Specification

This document details upcoming improvements, architectural additions, and multi-platform implementations planned for future releases of **OpenWhisper**.

---

## 1. Multi-Platform Backends

While OpenWhisper currently targets Linux on Wayland (Fedora / KDE Plasma 6), the core architecture (in-memory audio processing, Whisper client, dual-mode hotkey engine, VAD, sound generation, and floating HUD overlay) is fully cross-platform. Platform-specific backends are planned as follows:

### A. macOS Support
- **Audio Capture**: Built-in support via `cpal` CoreAudio host.
- **Global Hotkeys**:
  - Global keydown and keyup capture for PTT via `CGEventTap` or `rdev`.
  - Permission management via macOS Accessibility APIs (`AXIsProcessTrustedWithOptions`).
- **Text Insertion**:
  - Direct clipboard copy via `arboard` (`NSPasteboard`).
  - Keystroke synthesis for `Cmd+V` via `CGEventCreateKeyboardEvent(NULL, (CGKeyCode)9, true)` (`kVK_ANSI_V`) with `kCGEventFlagMaskCommand`.
- **System Integration**:
  - Status bar menu extra via native AppKit `NSStatusItem` or `tray-icon`.
  - User session background daemon managed via `~/Library/LaunchAgents/net.local.openwhisper.plist`.

### B. Windows Support
- **Audio Capture**: Built-in support via `cpal` WASAPI host.
- **Global Hotkeys**:
  - For Toggle mode: Win32 `RegisterHotKey`.
  - For PTT mode: Low-level keyboard hook via `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` to track keydown and keyup events with high precision.
- **Text Insertion**:
  - Direct clipboard copy via `arboard`.
  - Keystroke synthesis for `Ctrl+V` via Win32 `SendInput` (`VK_CONTROL` and `0x56`).
  - Optional direct text insertion using `KEYEVENTF_UNICODE` for instantaneous character streaming.
- **System Integration**:
  - System tray icon via Win32 `Shell_NotifyIcon` or `tray-icon`.
  - Background autostart via Windows Registry Run key or Windows Service.

### C. iOS Architecture
- **Audio Engine**: `AVAudioEngine` / `AudioUnit` input node.
- **Core Library**:
  - Modularize the core pipeline into `openwhisper-core` compiled as a universal static library (`.xcframework`) using UniFFI or C-FFI.
- **User Interface & Extension**:
  - Custom Keyboard Extension providing a dictation button inside any iOS app, inserting text directly via `UITextDocumentProxy.insertText()`.
  - SwiftUI companion application for server configuration, model selection, and prompt management.

---

## 2. Streaming & Real-Time Dictation

### Motivation
For lengthy dictation sessions, seeing words appear in real time reduces perceived latency and provides immediate feedback on speech recognition accuracy.

### Architecture & Protocol
- **Chunked Audio Streaming**:
  - Stream PCM chunks over WebSocket or chunked HTTP transfer encoding to streaming-capable backends (e.g., OpenVINO Model Server gRPC/WebSocket streaming endpoints, Faster-Whisper live streaming server, or WhisperLive).
- **Buffer Synchronization**:
  - Continuous interim transcription tokens are received asynchronously.
  - Implement delete-and-replace buffer updating:
    - Track the character length of the last interim token sequence.
    - Emit backspaces or replacement sequences to rewrite the active draft as Whisper's language model refines word choices based on extended acoustic context.
  - On release/silence, lock the finalized transcript into the active document.

### Research Findings: OpenVINO Model Server (OVMS) Audio Streaming
- **HTTP REST (`/v1/audio/transcriptions`)**: Supports streaming *responses* (SSE text tokens via `stream=true`), but does **not** support live chunked *audio input* streams. The complete audio file must be uploaded in the request body.
- **gRPC (`ModelStreamInfer`)**: OVMS provides bidirectional gRPC streaming, but out-of-the-box Whisper models (`speech2text` task) operate on discrete audio buffers. True live chunked streaming audio requires deploying a custom MediaPipe audio chunking and sliding-window graph on the server.

---

## 3. Persistent Transcription History & Search

### Motivation
Users often dictate thoughts, messages, or code snippets that they need to reference or re-copy later, especially if the target application was not focused or the text was accidentally replaced.

### Architecture & Storage
- **Persistent Storage**:
  - Persist all completed dictations to `~/.local/share/openwhisper/history.sqlite3` (or append-only JSONL).
  - Store metadata: timestamp, raw transcript, formatted transcript, audio duration, endpoint used, and character count.
- **Access & UI**:
  - Add a dedicated **History** tab in the Slint Settings window featuring full-text search, copy-to-clipboard buttons, and date filtering.
  - CLI command: `openwhisper history [--limit N] [--search QUERY]`.

---

## 4. Audio Dataset Collection & TTS Voice Cloning

### Motivation
High-quality Text-To-Speech (TTS) models (e.g. Piper, Coqui, F5-TTS, StyleTTS 2) require hundreds of paired `.wav` audio files and matching text transcripts to train or fine-tune personalized synthetic voices.

### Status & Planned Enhancements
- **[Completed] Paired Audio & Transcript Recording**: Configurable `save_audio_dir` setting (via GUI or `config.toml`) automatically saves paired timestamped `.wav` (16kHz mono) and `.txt` transcripts for every dictation.
- **[Planned] Dataset Exporter & Manifest Tool**: Built-in CLI tool (`openwhisper export-tts-dataset`) to scan the recordings directory, validate audio lengths, filter out silences/noise, and package standard metadata manifests (`metadata.csv` / LJSpeech format) directly usable by Piper, XTTS, and F5-TTS training scripts.

---

## 5. LLM Post-Processing & Smart Dictation Styles (Low Priority)

### Motivation
Spoken language frequently contains conversational artifacts such as filler words ("um", "uh", "you know"), stutters, false starts, and self-corrections ("let's meet Tuesday, wait, I mean Wednesday").

### Architecture
- Optional second-stage LLM pipeline querying a local model (Ollama, vLLM, llama.cpp on `frigg`) or cloud API.
- **Persona & Transformation Styles**:
  - **Cleaned**: Strips fillers, repetitions, and hesitation while preserving exact word choice.
  - **Professional / Email**: Formats stream-of-consciousness thoughts into concise, polished paragraphs.
  - **Code & Terminal**: Automatically detects variable names, shell commands, and syntax.
  - **Bullet Points**: Condenses spoken thoughts into structured action items.
- Configurable per-app rules (e.g. Terminal apps use Code style, email clients use Professional style).

---

## 6. Pre-Flight System Diagnostics (`openwhisper doctor`)

### Motivation
Diagnosing Wayland permissions, D-Bus session issues, remote inference endpoints, and audio capture devices during initial setup or troubleshooting should be instant and automated.

### Planned Features
- **CLI Diagnostic Command**: `openwhisper doctor` to inspect and output a health report:
  - Configuration syntax and schema validation (`~/.config/openwhisper/config.toml`).
  - `/dev/uinput` permissions and ACL verification (`user:$USER:rw-`).
  - Active audio capture device availability, sample rate compatibility, and input level check.
  - Remote STT endpoint reachability and latency probe (`http://frigg:8000/v1/audio/transcriptions`).
  - KDE Plasma KWin rule inspection (`kwinrulesrc` position and reconfigure status).
  - Evdev hardware keyboard device detection (`/dev/input/event*`).

---

## 7. Audio Hardware Resilience & Auto-Reconnection

### Motivation
USB microphones, wireless headsets, and Bluetooth audio devices can be disconnected, put into low-power sleep, or changed while the daemon is running.

### Planned Features
- **Device Watchdog**: Detect capture stream disconnections via `cpal` error callbacks.
- **Graceful Fallback**: Automatically fall back to the system default capture device if the designated custom device disappears.
- **Auto-Reconnect**: Seamlessly re-bind the preferred microphone when it is reconnected without requiring a daemon restart or manual reload.

---

## 8. Direct Virtual Keyboard Keystroke Injection (`/dev/uinput`)

### Motivation
In `output_mode = "type"`, spawning `wtype` or `ydotool` as external subprocesses introduces process fork latency (~15–25ms).

### Planned Features
- Expand the `/dev/uinput` virtual device in [`src/output/injector.rs`](src/output/injector.rs) to map full UTF-8 characters to standard evdev scancodes with Shift/AltGr state tracking.
- Stream scancodes directly through `/dev/uinput` kernel ioctl for zero-fork, sub-millisecond typing.

---

## 9. Granular Tray & HUD Health Diagnostics

### Motivation
When the remote STT server goes down or returns errors (e.g., out-of-memory or model load errors), the user should immediately see clear diagnostic indicators.

### Planned Features
- Dynamic system tray icon status (Green = Ready, Yellow = Network Degraded / Reconnecting, Red = Error).
- Informative hover tooltips on the tray icon displaying latency and last error summary.
- HUD error pills with descriptive troubleshooting messages (e.g., `"OVMS Unreachable (192.168.1.50:8000)"`).

---

## 10. Instant Physical Abort Key (`Escape` to Cancel Recording)

### Status
- **[Completed]**: Physical `KEY_ESC` monitored via `evdev` (`ev.value() == 1`). If recording is active, it drops the recording immediately, clears the HUD, and emits a descending two-tone cancel chime (`EarconType::Cancel`, 440 Hz -> 260 Hz) without querying the STT model or pasting text.

---

## 11. System Tray "Recent Dictations" Quick Re-Copy Menu

### Status
- **[Completed]**: In-memory ring buffer (`VecDeque<String>`) of the last 10 dictations managed by `TrayController`. Dynamically rendered as a `📋 Recent Dictations` submenu in the KDE Plasma `ksni` system tray. Clicking any item copies the full text to the clipboard and shows a desktop notification.

---

## 12. Spoken Punctuation & Keyword Formatting Macros

### Motivation
Whisper models vary in how reliably they handle explicit punctuation instructions. Sometimes "new line" is transcribed literally as words, or users want to speak formatting commands without an LLM.

### Planned Features
- Configurable toggle: **"Parse Spoken Punctuation"**.
- Post-processing regex mapping:
  - `"new line"` / `"next line"` $\to$ `\n`
  - `"new paragraph"` $\to$ `\n\n`
  - `"period"` / `"full stop"` $\to$ `.`
  - `"comma"` $\to$ `,`
  - `"question mark"` $\to$ `?`
  - `"exclamation mark"` / `"exclamation point"` $\to$ `!`
  - `"colon"` $\to$ `:`
  - `"semicolon"` $\to$ `;`

---

## 13. Custom Text Expansion & Snippets (Personal Dictionary)

### Motivation
Dictating complex technical email addresses, long URLs, boilerplate code blocks, or kaomoji/emojis by voice is error-prone.

### Planned Features
- Key-value snippet table in Settings:
  - `"my email"` $\to$ `"user@example.com"`
  - `"shrug"` $\to$ `"¯\_(ツ)_/¯"`
  - `"jira ticket"` $\to$ `"https://jira.internal/browse/"`
- Exact-phrase and regex substitution applied automatically during text post-processing.

---

## 14. Interactive Microphone VU Level Meter in Settings

### Motivation
Users configuring a new microphone or adjusting input gains need immediate visual confirmation that their microphone is picking up sound without having to test a full dictation.

### Planned Features
- Live animated RMS audio level meter bar in the "Audio & Recording" settings tab.
- "Test Microphone" button that streams 3 seconds of capture audio, computing real-time RMS levels and indicating if the signal is clipping or too quiet.

---

## 15. Context-Aware Automatic Formatting (Smart App Profiles)

### Motivation
Dictating in a Linux terminal or IDE requires different formatting (lowercase, no trailing spaces/periods, snake_case) than writing an email or chat message in Thunderbird or Slack.

### Planned Features
- Query the active window's Wayland `app_id` or X11 `WM_CLASS` via KWin D-Bus (`org.kde.KWin`).
- Configurable app profile rules:
  - `alacritty`, `konsole`, `kitty`, `foot` $\to$ `Raw` or `SnakeCase`, no auto-capitalization, no trailing period.
  - `slack`, `discord`, `telegram`, `thunderbird` $\to$ `Standard` sentence case with punctuation.

