# AGENTS.md — OpenWhisper Contributor & Agent Guide

Welcome to the **OpenWhisper** codebase! This document provides technical orientation, design decisions, architectural invariants, and workflows for autonomous agents and human developers working on OpenWhisper in Antigravity IDE.

---

## 1. Project Mission & Context

OpenWhisper is an open-source, multi-platform speech-to-text dictation assistant written in Rust (inspired by Superwhisper).

- **Current Primary Platform**: Linux on Wayland (Fedora, KDE Plasma 6)
- **Planned Target Platforms**: macOS, Windows, iOS
- **Local Inference Engine**: [OpenVINO Model Server (OVMS)](https://docs.openvino.ai/2026/model-server/ovms_demos_audio.html) running Whisper locally (typically on `http://localhost:8000/v1/audio/transcriptions`)
- **Compatibility**: Any OpenAI-compatible Speech-to-Text (`/v1/audio/transcriptions`) endpoint (e.g. OpenAI, Groq, vLLM, Whisper.cpp server, Ollama)

---

## 2. Architectural Invariants & Rules

When modifying or extending the codebase, adhere to these core invariants:

1. **Zero-Disk In-Memory Audio Pipeline**:
   - Audio is captured via `cpal`, resampled in-memory to 16kHz mono 16-bit PCM, and encoded to WAV format directly in a `Cursor<Vec<u8>>` via `hound`.
   - **Rule**: Never write temporary audio files to `/tmp` or disk. Keeping audio in RAM maximizes speed (sub-100ms processing) and guarantees user privacy.

2. **Dual-Mode Hotkey State Machine**:
   - The engine in [`src/hotkey/state.rs`](src/hotkey/state.rs) handles both **Push-To-Talk (PTT)** and **Hands-Free Toggle**:
     - Key Down: Starts audio recording and records timestamp.
     - Key Up: If held $\ge$ `ptt_threshold_ms` (default: 350ms), stops and transcribes (PTT). If released in < 350ms, switches to hands-free recording mode until tapped again (Toggle).
   - **Rule**: Do not decouple PTT and Toggle into separate mutually-exclusive modes; preserve this seamless dual behavior.

3. **Wayland Text Insertion**:
   - On Linux/Wayland, background applications cannot arbitrarily inject text or capture global keys without compositor assistance.
   - We solve text insertion by:
     1. Writing text to the clipboard using `wl-copy` (native Wayland protocol) with `arboard` fallback.
     2. Emitting a `Ctrl+V` keypress using a virtual keyboard device created via `/dev/uinput` (using the `evdev` crate).
   - User `mike` (and standard active seat users on Fedora) has ACL permissions (`user:$USER:rw-`) on `/dev/uinput`.
   - **Rule**: If `/dev/uinput` is unavailable, gracefully fall back to clipboard-only insertion without panicking.

4. **Async & Threading Safety**:
   - OpenWhisper runs on Tokio.
   - **Rule**: Never call blocking runtime-instantiating libraries (such as `notify_rust::Notification::show()`) directly within Tokio async tasks, as this triggers `Cannot start a runtime from within a runtime`. Always dispatch blocking UI/sound calls via `std::thread::spawn` or `tokio::task::spawn_blocking`.

---

## 3. Directory Structure & Module Map

```
openwhisper/
├── src/
│   ├── main.rs            # CLI dispatch, daemon event loop, coordination
│   ├── cli.rs             # Clap command-line parser & subcommands
│   ├── config.rs          # Config loader (~/.config/openwhisper/config.toml)
│   ├── notification.rs    # Safe thread-isolated desktop notifications
│   ├── audio/
│   │   ├── mod.rs         # Module exports & device query
│   │   ├── recorder.rs    # cpal stream management & in-memory WAV packaging
│   │   └── resampler.rs   # Downmixing & linear resampling to 16kHz mono i16
│   ├── transcribe/
│   │   ├── mod.rs         # Module exports
│   │   └── client.rs      # Reqwest multipart/form-data client for STT
│   ├── output/
│   │   ├── mod.rs         # OutputManager coordinating clipboard & injection
│   │   ├── clipboard.rs   # wl-copy / arboard clipboard implementation
│   │   └── injector.rs    # /dev/uinput virtual keyboard Ctrl+V emulation
│   └── hotkey/
│       ├── mod.rs         # Module exports
│       ├── state.rs       # PTT vs Toggle state machine
│       ├── ipc.rs         # Unix domain socket server & client commands
│       └── portal.rs      # XDG Desktop Portal GlobalShortcuts (Wayland/KDE)
├── systemd/
│   └── openwhisper.service # systemd user unit for background auto-start
├── Cargo.toml             # Dependencies and build configuration
├── README.md              # User documentation & setup instructions
├── AGENTS.md              # This orientation guide
└── ROADMAP.md             # Future enhancements & platform expansion plans
```

---

## 4. Key Workflows & Commands

### Building & Checking
```bash
# Debug build & test suite
cargo check
cargo test

# Release build
cargo build --release

# Install locally
sudo cp target/release/openwhisper /usr/local/bin/openwhisper
```

### Verification Commands
```bash
# 1. Verify local OpenVINO Model Server STT connectivity
openwhisper test-ovms

# 2. List available microphones
openwhisper list-devices

# 3. Test standalone one-shot microphone recording (2 seconds)
openwhisper record --duration 2

# 4. Check daemon status via IPC
openwhisper status

# 5. Send toggle command to running daemon
openwhisper toggle

# 6. Sniff real-time hardware key scancodes and hold durations
openwhisper test-hotkey
```

---

## 5. System Integration Points (Fedora / KDE Plasma)

- **IPC Socket**: `/run/user/1000/openwhisper.sock` (JSON lines protocol supporting `toggle`, `ptt_down`, `ptt_up`, `cancel`, `status`).
- **KDE Plasma 6 Shortcuts**: Custom Shortcuts $\rightarrow$ Add Command $\rightarrow$ `openwhisper toggle`.
- **Systemd User Service**: `~/.config/systemd/user/openwhisper.service` managed via `systemctl --user`.
- **Configuration File**: `~/.config/openwhisper/config.toml`.
