# OpenWhisper Improvement Roadmap & Specification

This document details upcoming improvements, architectural additions, and multi-platform implementations planned for **OpenWhisper**.

---

## 1. Audio Feedback / Earcons

### Motivation
When using Push-To-Talk or hands-free Toggle dictation, visual notifications can be outside the user's peripheral vision. Subtle audio earcons provide immediate confirmation of the recording state.

### Specification
- **Start Sound**: A gentle, rising two-tone chime (e.g. 440 Hz $\rightarrow$ 880 Hz, 60ms, soft attack/decay envelope) when recording begins.
- **Stop Sound**: A subtle descending tone (e.g. 660 Hz $\rightarrow$ 440 Hz, 50ms) when recording stops.
- **Complete Sound**: A short soft confirmation blip (e.g. 1000 Hz, 30ms) when text is pasted.
- **Error Sound**: A double low buzz (e.g. 220 Hz $\rightarrow$ 180 Hz) if network/transcription fails.

### Implementation Strategy
- Pure in-memory sound generation (synthesizing sine wave PCM buffers mathematically) so OpenWhisper requires zero external `.wav` asset files.
- Audio output playback via `cpal` or a lightweight audio sink (`rodio`).
- Config option in `config.toml`:
  ```toml
  sound_feedback = true
  sound_volume = 0.5
  ```

---

## 2. Contextual Prompting & Vocabulary Biasing

### Motivation
Whisper models support an optional `prompt` parameter on `/v1/audio/transcriptions`. This prompt biases the decoder towards specific spelling, technical terminology, and punctuation style.

### Specification
- Add per-profile or global prompt templates in `config.toml`:
  ```toml
  [prompting]
  # Technical vocabulary biasing
  vocabulary = ["Rust", "Wayland", "KDE", "OpenVINO", "cpal", "tokio", "uinput", "Fedora"]
  custom_prompt = "Technical dictation including code terms and proper punctuation."
  temperature = 0.0
  ```
- Support formatting modes:
  - **Standard**: Natural language with punctuation.
  - **Code/Identifier**: Auto-converts spoken phrases to `snake_case`, `camelCase`, or `kebab-case`.
  - **Raw**: Verbatim transcription without automatic periods.

---

## 3. Voice Activity Detection (VAD)

### Motivation
Prevent runaway recordings when toggled hands-free if the user speaks and then walks away or pauses for extended periods.

### Specification
- Integrate a lightweight VAD (such as Silero VAD via ONNX runtime or energy-based threshold gating):
  ```toml
  [vad]
  enabled = true
  silence_timeout_ms = 1800 # Automatically stop after 1.8s of silence
  energy_threshold = 0.015
  ```
- Real-time sample window checking in `src/audio/recorder.rs` callback.
- Automatically emits `StopAndTranscribe` when trailing silence exceeds threshold.

---

## 4. Multi-Platform Implementations

### A. macOS Support
- **Audio Capture**: `cpal` already supports macOS CoreAudio out of the box.
- **Global Hotkeys**:
  - Use `rdev` or a dedicated `CGEventTap` hook to capture global keydown and keyup events for PTT.
  - Request Accessibility permissions (`AXIsProcessTrustedWithOptions`).
- **Text Insertion**:
  - Set clipboard via `arboard` (`NSPasteboard`).
  - Synthesize `Cmd+V` keystroke using `CGEventCreateKeyboardEvent(NULL, (CGKeyCode)9, true)` (`kVK_ANSI_V`) with `kCGEventFlagMaskCommand`.

### B. Windows Support
- **Audio Capture**: `cpal` supports WASAPI out of the box.
- **Global Hotkeys**:
  - For Toggle mode: Win32 `RegisterHotKey`.
  - For PTT mode: Low-level keyboard hook `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` to track precise keydown and keyup timing.
- **Text Insertion**:
  - Set clipboard via `arboard`.
  - Synthesize `Ctrl+V` using Win32 `SendInput` with `VK_CONTROL` and `'V'`.
  - Alternatively, inject characters directly using `KEYEVENTF_UNICODE` for instantaneous typing.

### C. iOS Architecture
- **Audio Engine**: `AVAudioEngine` / `AudioUnit` for audio capture.
- **Core Library**: Compile `openwhisper-core` as a Rust static library (`.a` / `.xcframework`) exposed via UniFFI or C-FFI.
- **User Interface**: Swift/SwiftUI app or Custom Keyboard Extension that sends audio to the local network or cloud STT endpoint and inserts text via `textDocumentProxy.insertText()`.

---

## 5. Streaming / Real-Time Dictation

### Motivation
For long dictation sessions, seeing words appear in real-time reduces perceived latency.

### Specification
- Support chunked streaming audio over HTTP or WebSocket:
  - If backend supports chunked transcription (or OpenVINO streaming gRPC/WebSocket endpoint), emit text continuously as the user speaks.
  - On Linux/Wayland, stream characters or delete-and-replace preview text in the active buffer.

---

## 6. Minimal Status Overlay / System Tray

### Motivation
A clean, non-intrusive status indicator showing when OpenWhisper is active:
- System tray icon in KDE Plasma panel (via StatusNotifierItem / `ksni` or `tray-icon` crate):
  - ⚪ Grey / Idle: Daemon ready.
  - 🔴 Red / Blinking: Recording audio.
  - 🟡 Amber: Transcribing.
- Optional floating minimalist pill indicator at the bottom of the screen displaying recording time and live audio level meter.
