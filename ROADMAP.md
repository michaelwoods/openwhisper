# OpenWhisper Roadmap & Future Architecture

This document tracks upcoming milestones, proposed features, and architectural plans for **OpenWhisper**.
*(Note: Once features are implemented, tested, and validated, they are removed from this roadmap and promoted to [`README.md`](README.md)).*

---

## Priority & Planning Matrix

| Priority | Feature / Initiative | Target Area | Status | Description |
| :--- | :--- | :--- | :--- | :--- |
| **P1** | **Spoken Punctuation & Formatting Macros** | Transcription Engine | **Next Candidate** | Spoken punctuation ("new line" $\to$ `\n`, "period" $\to$ `.`) |
| **P1** | **Dataset Exporter & Manifest Tool** | Audio / Dataset | **Next Candidate** | `openwhisper export-tts-dataset` (LJSpeech / Piper / XTTS) |
| **P2** | **Custom Text Expansion & Voice Snippets** | Post-Processing | **Planned** | Voice snippet expansion table in GUI & `config.toml` |
| **P2** | **Context-Aware Automatic Formatting** | Desktop Integration | **Planned** | Active window detection via KWin D-Bus for smart formatting |
| **P2** | **LLM Post-Processing & Smart Styles** | Inference Pipeline | **Planned** | Filler word removal & email polishing via local LLM |
| **P3** | **Multi-Platform Backends (macOS, Windows, iOS)** | Core Architecture | **Planned** | Native CoreAudio/CGEvent, WASAPI/SendInput, and iOS extension |
| **P3** | **Streaming & Live Real-Time Dictation** | Audio / STT | **Under Research** | Chunked WebSocket/gRPC streaming with delete-replace buffer |

---

## 1. Spoken Punctuation & Keyword Formatting Macros

### Motivation
Whisper models vary in how reliably they handle explicit punctuation instructions. Sometimes "new line" is transcribed literally as words, or users want to speak formatting commands without an LLM.

### Planned Features
- Configurable toggle: **"Parse Spoken Punctuation"** in settings and `config.toml`.
- Post-processing regex mapping:
  - `"new line"` / `"next line"` $\to$ `\n`
  - `"new paragraph"` $\to$ `\n\n`
  - `"period"` / `"full stop"` $\to$ `.`
  - `"comma"` $\to$ `,`
  - `"question mark"` $\to$ `?`
  - `"exclamation mark"` / `"exclamation point"` $\to$ `!`
  - `"colon"` $\to$ `:`
  - `"semicolon"` $\to$ `;`
  - `"open quote"` / `"close quote"` $\to$ `"`
  - `"open paren"` / `"close paren"` $\to$ `(` / `)`

---

## 2. Audio Dataset Exporter & TTS Training Tool (`openwhisper export-tts-dataset`)

### Motivation
High-quality Text-To-Speech (TTS) models (e.g. Piper, Coqui, F5-TTS, StyleTTS 2) require hundreds of paired `.wav` audio files and matching text transcripts to train or fine-tune personalized synthetic voices. OpenWhisper already records paired `.wav` and `.txt` files in `save_audio_dir`.

### Planned Features
- Built-in CLI tool: `openwhisper export-tts-dataset`:
  - Scans the recordings directory and validates audio integrity and sample rates.
  - Filters out recordings that are too short ($<0.5$s) or contain pure silence/noise.
  - Generates standard training manifests:
    - **LJSpeech Format**: `metadata.csv` (`filename|transcript|normalized_transcript`).
    - **Piper / XTTS Manifests**: JSON/CSV formatted for immediate use by Piper, Coqui, or F5-TTS fine-tuning scripts.
  - Option to split into train/validation sets (e.g. 95% / 5%).

---

## 3. Custom Text Expansion & Snippets (Personal Dictionary)

### Motivation
Dictating complex technical email addresses, long URLs, boilerplate code blocks, or kaomoji/emojis by voice is error-prone.

### Planned Features
- Key-value snippet table configured in GUI and `config.toml`:
  - `"my email"` $\to$ `"user@example.com"`
  - `"shrug"` $\to$ `"¯\_(ツ)_/¯"`
  - `"jira ticket"` $\to$ `"https://jira.internal/browse/"`
  - `"std result"` $\to$ `"Result<T, Box<dyn std::error::Error>>"`
- Exact-phrase and regex substitution applied automatically during text post-processing before output injection.

---

## 4. Context-Aware Automatic Formatting (Smart App Profiles)

### Motivation
Dictating in a Linux terminal or IDE requires different formatting (lowercase, no trailing spaces/periods, snake_case) than writing an email or chat message in Thunderbird or Slack.

### Planned Features
- Query the active window's Wayland `app_id` or X11 `WM_CLASS` via KWin D-Bus (`org.kde.KWin`).
- Configurable app profile rules in `config.toml`:
  - `alacritty`, `konsole`, `kitty`, `foot` $\to$ `Raw` or `snake_case`, no auto-capitalization, no trailing period.
  - `slack`, `discord`, `telegram`, `thunderbird` $\to$ `Standard` sentence case with punctuation.
  - Custom per-application prompt guidance (e.g. specialized terminology for IDE vs chat).

---

## 5. LLM Post-Processing & Smart Dictation Styles

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

## 6. Multi-Platform Backends

While OpenWhisper currently targets Linux on Wayland (Fedora / KDE Plasma 6), the core architecture (in-memory audio processing, Whisper client, dual-mode hotkey engine, VAD, sound generation, and floating HUD overlay) is designed for cross-platform expansion:

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

## 7. Streaming & Real-Time Dictation

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
