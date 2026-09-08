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
| **Sinc-Based Resampling (`rubato`)** | Section 2.A | **Next Candidate** | Fix linear interpolation anti-aliasing artifacts |
| **Lock-Free Audio Ring Buffer** | Section 2.B | **Next Candidate** | Eliminate `Mutex` in real-time `cpal` callback to prevent dropouts |
| **Robust IPC Message Framing** | Section 2.C | **Next Candidate** | Replace fixed 512-byte buffer with newline-delimited stream |
| **Integration Tests & CI Pipeline** | Section 3 | **Next Candidate** | Mock STT (`wiremock`), `tests/` suite, and GitHub Actions CI |
| **System Diagnostics (`doctor`)** | Section 6 | **Next Candidate** | Automated pre-flight health check CLI tool |
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

### A. Replace Naive Resampler with Sinc-Based Resampling
- **Problem**: `resample_to_mono_16k` in `src/audio/resampler.rs` uses linear interpolation for downsampling (e.g. 48kHz → 16kHz) **without an anti-aliasing low-pass filter**. Frequencies above the Nyquist limit (8kHz) fold back into the audible band, corrupting the spectral content Whisper relies on and degrading transcription accuracy. The same flawed logic is duplicated in `src/audio/denoise.rs`.
- **Solution**: Replace with [`rubato::SincFixedIn`](https://crates.io/crates/rubato) for high-quality sinc resampling. Delete the duplicated `resample_linear` in `denoise.rs` and share a single resampling path.

### B. Lock-Free Audio Callback Buffer
- **Problem**: The `cpal` real-time audio input callback in `src/audio/recorder.rs` acquires a `Mutex<Vec<f32>>` on every buffer delivery (~every 5ms). If the main thread holds this lock (e.g. during `stop_with_options`), the callback blocks, causing buffer overruns and audio dropouts.
- **Solution**: Replace `Arc<Mutex<Vec<f32>>>` with a lock-free ring buffer (e.g. [`ringbuf`](https://crates.io/crates/ringbuf) or [`rtrb`](https://crates.io/crates/rtrb)). A consumer thread drains the ring buffer without contending with the real-time callback.

### C. Robust IPC Socket Message Framing
- **Problem**: The IPC server in `src/hotkey/ipc.rs` reads into a fixed `[0u8; 512]` buffer with a single `read()` call. Commands larger than 512 bytes are silently truncated and fail to parse; fragmented Unix socket reads produce partial JSON that is silently dropped.
- **Solution**: Switch to newline-delimited JSON lines protocol using `tokio::io::AsyncBufReadExt::read_line`, or implement length-prefixed framing.

---

## 3. Testing Infrastructure & CI

### A. Integration Test Suite
- **Problem**: The project has 91 unit tests across 22 source files but zero integration tests. End-to-end flows (record → resample → transcribe → output) are completely untested, and there is no `tests/` directory.
- **Planned**:
  - Add a `tests/` directory with integration tests covering the full dictation pipeline using mock audio data and a local mock STT server ([`wiremock`](https://crates.io/crates/wiremock)).
  - Add `[dev-dependencies]` for `wiremock`, `tempfile`, and `assert_cmd` to support HTTP mocking, temp fixtures, and CLI binary testing.
  - Populate the empty `examples/` directory with runnable usage examples for contributors.

### B. Continuous Integration Pipeline
- **Problem**: There is no CI/CD configuration. Tests only run when manually invoked via `cargo test` or `make test`. Regressions can land undetected.
- **Planned**:
  - Add a GitHub Actions workflow (`.github/workflows/ci.yml`) running `cargo check`, `cargo test`, `cargo clippy`, and `cargo fmt --check` on every push and pull request.
  - Matrix-test across stable and nightly Rust toolchains.
  - Cache `~/.cargo` and `target/` for fast CI builds.

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

