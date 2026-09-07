# OpenWhisper Improvement Roadmap & Specification

This document details upcoming improvements, architectural additions, and multi-platform implementations planned for **OpenWhisper**.

---

## 1. Audio Feedback / Earcons [Completed ✅]

Subtle audio earcons provide immediate confirmation of the recording state:
- Pure in-memory sound generation synthesizing sine-wave PCM mathematically with smooth attack/decay envelopes (zero disk files).
- Multi-state sounds: Start recording (rising chime), Stop recording (descending tone), Transcribed (soft confirmation blip), and Error.
- Configurable via `sound_feedback` and `sound_volume` in `config.toml`.

---

## 2. Contextual Prompting & Vocabulary Biasing [Completed ✅]

Whisper prompt biasing and output text transformation:
- Domain vocabulary biasing: Injects custom terms into the Whisper decoding prompt.
- Formatting modes:
  - `Standard`: Natural language with punctuation.
  - `SnakeCase`: Auto-converts to `snake_case`.
  - `CamelCase`: Auto-converts to `camelCase`.
  - `KebabCase`: Auto-converts to `kebab-case`.
  - `Raw`: Verbatim transcription without automatic punctuation.

---

## 3. Voice Activity Detection (VAD) [Completed ✅]

Prevents runaway recordings when toggled hands-free:
- Real-time RMS silence gating in background monitor thread without disturbing audio capture.
- Configurable `vad_enabled`, `vad_silence_timeout_ms`, and `vad_energy_threshold`.
- Automatically triggers `StopAndTranscribe` when trailing silence exceeds timeout.

---

## 4. Desktop Integration: System Tray & Settings GUI [Completed ✅]

Seamless Wayland / KDE Plasma desktop experience:
- **System Tray**: Implemented via `ksni` (D-Bus StatusNotifierItem). Dynamically updates icons:
  - Idle (`openwhisper-tray-idle`)
  - Recording (`openwhisper-tray-recording`)
  - Transcribing (`openwhisper-tray-transcribing`)
  - Error (`openwhisper-tray-error`)
  - Full context menu with left-click toggle, Settings, and Quit.
- **Graphical Configuration Panel**: Native GUI built with `egui` / `eframe` (`openwhisper config-gui`):
  - Test server connectivity & latency in real-time.
  - Live sound feedback volume test.
  - Vocabulary editor, formatting mode picker, VAD controls.
- **Automated Setup Phase**:
  - `openwhisper setup`, `make install`, and `scripts/install.sh` for one-command deployment.

---

## 5. Multi-Platform Implementations


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
