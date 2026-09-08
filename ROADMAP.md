# OpenWhisper Roadmap & Architecture Specification

This document details completed milestones, upcoming enhancements, and long-term architectural plans for **OpenWhisper**.

## Current Status & Reality Matrix

| Feature / Initiative | Roadmap Section | Reality Status | Notes / Capabilities |
| :--- | :--- | :--- | :--- |
| **Zero-Disk Audio Pipeline** | Core Architecture | **Completed** | 16kHz mono in-memory capture & Hound WAV encoding |
| **Dual-Mode Hotkey Engine** | Core Architecture | **Completed** | PTT hold + hands-free toggle tap state machine |
| **Wayland Text Insertion** | Core Architecture | **Completed** | Native `wl-copy` clipboard + `/dev/uinput` virtual keyboard Ctrl+V |
| **Slint Floating HUD Overlay** | GUI / HUD | **Completed** | Real-time audio waveform meter with KWin positioning rules |
| **Procedural Audio Cues** | Audio Feedback | **Completed** | Synthesized start/stop/error chime envelopes (no audio assets) |
| **Voice Activity Detection** | Audio / VAD | **Completed** | RMS energy-based silence detection with configurable timeout |
| **Paired Audio/Text Dataset** | Section 5 | **Completed** | Timestamped paired `.wav` and `.txt` dictation recording |
| **History & Audio Playback** | Section 5 | **Completed** | SQLite sync, audio backfill, cancellable playback engine, GUI & CLI |
| **Pure Vector System Tray** | Desktop Integration | **Completed** | Breeze-compatible SVG SNI, dynamic palettes, 5 states, 0% blur |
| **Slint Settings GUI** | Desktop Integration | **Completed** | Multi-tab settings (model, hotkeys, audio, VAD, vocabulary) |
| **Sinc-Based Resampling (`rubato`)** | Section 2.A | **Completed** | Bandlimited sinc resampling with BlackmanHarris2 window; eliminates aliasing |
| **Lock-Free Audio Ring Buffer** | Section 2.B | **Completed** | SPSC lock-free `HeapRb` ring buffer; zero mutexes in real-time `cpal` callback |
| **Robust IPC Message Framing** | Section 2.C | **Completed** | Newline-delimited stream framing with BufReader; eliminates buffer truncation |
| **Integration Tests & CI Pipeline** | Section 3 | **Completed** | 104 tests (93 unit, 6 CLI assert_cmd, 5 wiremock pipeline), GitHub/Forgejo CI |
| **System Diagnostics (`doctor`)** | Section 6 | **Completed** | Pre-flight health checks (config, audio, STT probe, uinput, IPC, KDE rules) |
| **Spoken Punctuation Macros** | Section 7 | **Next Candidate** | Voice macros ("new line" $\to$ `\n`, "period" $\to$ `.`) |
| **Custom Text Expansion** | Section 8 | **Planned** | Voice snippet expansion table in Settings |
| **Context-Aware App Profiles** | Section 9 | **Planned** | Active window detection via KWin D-Bus for smart formatting |
| **LLM Post-Processing Styles** | Section 10 | **Planned** | Filler word stripping, email polishing via local LLM |
| **Multi-Platform Backends** | Section 1 | **Planned** | macOS (`CGEvent`), Windows (`SendInput`), iOS (`openwhisper-core`) |
| **Streaming / Live Dictation** | Section 4 | **Under Research** | Chunked WebSocket/gRPC streaming with delete-replace buffer |

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

## 2. Core Audio & Runtime Reliability Fixes

### A. [Completed] Sinc-Based Resampling (`rubato`)
- **Status**: Replaced linear interpolation in `src/audio/resampler.rs` and `src/audio/denoise.rs` with `rubato::Fft` sinc interpolation with `BlackmanHarris2` anti-aliasing window.
- **Benefit**: Signals above the 8kHz Nyquist frequency are strongly attenuated (>28 dB), eliminating aliasing distortion and maximizing Whisper acoustic precision.

### B. [Completed] Lock-Free Audio Ring Buffer (`ringbuf`)
- **Status**: Replaced mutex locking in real-time `cpal` audio callback with a lock-free single-producer single-consumer (`HeapRb`) ring buffer in `src/audio/recorder.rs`.
- **Benefit**: Audio capture thread executes in hard real-time without memory allocations or thread contention; background drain worker forwards samples safely.

### C. [Completed] Robust IPC Socket Message Framing
- **Status**: Refactored `src/hotkey/ipc.rs` to stream newline-delimited JSON commands (`\n`) parsed via `tokio::io::BufReader`.
- **Benefit**: Eliminates the previous 512-byte payload truncation and socket fragmentation bugs.

---

## 3. Testing Infrastructure & CI

### A. [Completed] Integration Test Suite
- **Status**: Created full test target suite under `tests/`:
  - `tests/cli_tests.rs`: Comprehensive CLI flag, help, version, and subcommand validation using `assert_cmd` and `predicates`.
  - `tests/pipeline_integration_test.rs`: End-to-end Whisper transcription flow against a `wiremock` mock server (`POST /v1/audio/transcriptions`), testing multipart WAV packaging, Bearer auth, error responses (500, empty audio), and SQLite history persistence with disk audio linking.
- **Total Tests**: 104 automated tests (93 unit tests + 6 CLI tests + 5 pipeline integration tests).

### B. [Completed] Continuous Integration Pipeline
- **Status**: Configured `.github/workflows/ci.yml` and dual-compatible `.forgejo/workflows` symlink.
- **Pipeline Stages**:
  1. Installs Linux system audio & GUI dependencies (`libasound2-dev`, `libfontconfig1-dev`, `libx11-dev`, `libwayland-dev`, `libxkbcommon-dev`, `pkg-config`, `build-essential`).
  2. Sets up Rust stable with `clippy` and `rustfmt`.
  3. Enforces `cargo fmt --check`.
  4. Runs `cargo check --verbose`.
  5. Enforces zero-warning lints with `cargo clippy --all-targets -- -D warnings`.
  6. Executes all unit and integration tests with `cargo test --verbose --all-targets`.
  7. Verifies optimized release build with `cargo build --release`.

---

## 4. Streaming & Real-Time Dictation

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

## 5. Audio Dataset Collection & TTS Voice Cloning

### Motivation
High-quality Text-To-Speech (TTS) models (e.g. Piper, Coqui, F5-TTS, StyleTTS 2) require hundreds of paired `.wav` audio files and matching text transcripts to train or fine-tune personalized synthetic voices.

### Status & Planned Enhancements
- **[Completed] Paired Audio & Transcript Recording**: Configurable `save_audio_dir` setting (via GUI or `config.toml`) automatically saves paired timestamped `.wav` (16kHz mono) and `.txt` transcripts for every dictation.
- **[Completed] History & Disk Audio Sync with Native Playback**: SQLite history database stores `audio_path`, automatically backfills existing recordings on disk, and provides responsive Play/Stop audio playback with cancellable audio streams in both the Slint History GUI (`openwhisper history --gui`) and CLI (`openwhisper history --play <ID>`).
- **[Planned] Dataset Exporter & Manifest Tool**: Built-in CLI tool (`openwhisper export-tts-dataset`) to scan the recordings directory, validate audio lengths, filter out silences/noise, and package standard metadata manifests (`metadata.csv` / LJSpeech format) directly usable by Piper, XTTS, and F5-TTS training scripts.

---

## 6. [Completed] Pre-Flight System Diagnostics (`openwhisper doctor`)

### Motivation
Diagnosing Wayland permissions, D-Bus session issues, remote inference endpoints, and audio capture devices during initial setup or troubleshooting should be instant and automated.

### Implementation & Features
- **Command**: `openwhisper doctor` (aliases: `check-system`, `diag`) with optional `--json` machine-readable output.
- **Diagnostics Performed**:
  1. **Configuration**: Path resolution, permissions, syntax/schema validation, active model, server URL, audio device, output mode.
  2. **Audio Hardware**: Enumerates available microphones, selects active capture device, initializes a non-blocking test stream, and measures ambient RMS energy levels.
  3. **STT Inference Backend**: Synthesizes a test tone WAV in-memory, transmits it to `config.server_url`, probes network reachability, verifies HTTP status, reports latency in milliseconds, and provides root-cause remediation tips (e.g. server offline, auth token invalid, model 404).
  4. **Virtual Input & Wayland**: Validates `/dev/uinput` read/write permissions for emulated `Ctrl+V`, detects compositor type (`wayland-0` / X11), and checks availability of clipboard (`wl-copy`, `wl-paste`, `xclip`) and virtual typing tools (`wtype`, `ydotool`).
  5. **Daemon & IPC Service**: Probes the Unix domain socket (`/run/user/<UID>/openwhisper.sock`) with `IpcCommand::Status`, and falls back to inspecting `systemctl --user is-active openwhisper.service`.
  6. **Desktop Integration**: Verifies scalable vector icons (`openwhisper.svg`), desktop entries (`net.local.openwhisper.desktop`), KDE Plasma KWin floating rules, and accessibility of hardware keyboard devices via evdev.

---

## 7. Spoken Punctuation & Keyword Formatting Macros

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

## 8. Custom Text Expansion & Snippets (Personal Dictionary)

### Motivation
Dictating complex technical email addresses, long URLs, boilerplate code blocks, or kaomoji/emojis by voice is error-prone.

### Planned Features
- Key-value snippet table in Settings:
  - `"my email"` $\to$ `"user@example.com"`
  - `"shrug"` $\to$ `"¯\_(ツ)_/¯"`
  - `"jira ticket"` $\to$ `"https://jira.internal/browse/"`
- Exact-phrase and regex substitution applied automatically during text post-processing.

---

## 9. Context-Aware Automatic Formatting (Smart App Profiles)

### Motivation
Dictating in a Linux terminal or IDE requires different formatting (lowercase, no trailing spaces/periods, snake_case) than writing an email or chat message in Thunderbird or Slack.

### Planned Features
- Query the active window's Wayland `app_id` or X11 `WM_CLASS` via KWin D-Bus (`org.kde.KWin`).
- Configurable app profile rules:
  - `alacritty`, `konsole`, `kitty`, `foot` $\to$ `Raw` or `SnakeCase`, no auto-capitalization, no trailing period.
  - `slack`, `discord`, `telegram`, `thunderbird` $\to$ `Standard` sentence case with punctuation.

---

## 10. LLM Post-Processing & Smart Dictation Styles

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

