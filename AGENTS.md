# AGENTS.md — OpenWhisper System Specification & Agent Guidelines

Technical invariants, architectural constraints, and execution workflows for autonomous agents operating in OpenWhisper.

---

## 1. System Mission & Environments

- **Core Goal**: Zero-latency, privacy-focused speech-to-text dictation assistant in Rust (inspired by Superwhisper).
- **Primary Platform**: Linux on Wayland (Fedora, KDE Plasma 6) with multi-platform targets (macOS, Windows, iOS).
- **Inference Targets**: OpenVINO Model Server (OVMS) or any OpenAI-compatible `/v1/audio/transcriptions` endpoint.
- **IPC Protocol**: Newline-delimited JSON (`\n`) over Unix Domain Socket (`/run/user/1000/openwhisper.sock`).

### Required Shell Environment
When executing build or test commands in subshells, ensure toolchain and library paths are populated:
```bash
export CARGO_HOME="${CARGO_HOME:-$HOME/.cache/puccinialin/cargo}"
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"
export PKG_CONFIG_PATH="$HOME/.local/lib/pkgconfig:$HOME/.local/share/pkgconfig:${PKG_CONFIG_PATH:-}:/usr/lib64/pkgconfig:/usr/share/pkgconfig"
```

---

## 2. Non-Negotiable Architectural Invariants

1. **Zero-Disk In-Memory Audio Pipeline**:
   - Audio captured via `cpal`, resampled in RAM, and encoded to WAV in-memory in a `Cursor<Vec<u8>>` via `hound`.
   - **Invariant**: Never write temporary audio files to `/tmp` or disk during dictation. Only write to disk when user explicitly enables permanent paired audio logging (`save_audio_dir`).

2. **Hard Real-Time Audio Capture Safety**:
   - The `cpal` data callback pushes incoming samples into a lock-free Single-Producer Single-Consumer (SPSC) ring buffer (`ringbuf::HeapRb`) consumed by a dedicated reader thread.
   - **Invariant**: Zero memory allocations, zero locks/mutexes, zero async runtime calls inside the audio input stream callback.

3. **Dual-Mode Hotkey State Machine**:
   - `src/hotkey/state.rs`: Key Down starts recording. Key Up evaluates elapsed duration against `ptt_threshold_ms` (default 350ms).
     - Duration $\ge 350\text{ms}$: Finalize & paste immediately (Push-To-Talk).
     - Duration $< 350\text{ms}$: Persist recording in hands-free mode until second tap (Toggle).
     - `KEY_ESC`: Instantly aborts dictation without querying STT or modifying clipboard.
   - **Invariant**: Never decouple PTT and Toggle into mutually exclusive modes.

4. **High-Fidelity Bandlimited Sinc Resampling**:
   - `src/audio/resampler.rs`: Resample arbitrary input rates/channels to 16kHz mono 16-bit PCM using `rubato` bandlimited sinc interpolation with `BlackmanHarris2` anti-aliasing window ($>28\text{ dB}$ stopband attenuation).
   - Fast linear interpolation is strictly a fallback if sinc allocation fails.

5. **Wayland Text Insertion & Fallbacks**:
   - Linux Wayland text insertion uses `/dev/uinput` (via `evdev`) for virtual keyboard `Ctrl+V` (or direct keystrokes), paired with `wl-copy` (native Wayland protocol) and `arboard` fallback.
   - **Invariant**: Never panic if `/dev/uinput` is unavailable; gracefully degrade to clipboard-only insertion.

6. **Async & Thread Isolation (Tokio Safety)**:
   - Daemon runs on Tokio async runtime.
   - **Invariant**: Never call blocking runtime-instantiating APIs (e.g. `notify_rust::Notification::show()`, rodio audio playback) from within Tokio async worker threads. Always isolate via `std::thread::spawn` or `tokio::task::spawn_blocking`.

7. **Stderr Logging Isolation**:
   - `tracing_subscriber` must write to `stderr` (`with_writer(std::io::stderr)`).
   - **Invariant**: `stdout` is reserved strictly for machine-readable command output (e.g. `openwhisper doctor --json`, `openwhisper history --json`) and clean piping.

---

## 3. Directory & Module Map

```
openwhisper/
├── src/
│   ├── lib.rs                 # Core library exports (audio, transcribe, config, history, etc.)
│   ├── main.rs                # CLI entry point, daemon orchestrator, event loop
│   ├── cli.rs                 # Clap CLI definitions and subcommand routing
│   ├── config.rs              # Configuration loader & validator (~/.config/openwhisper/config.toml)
│   ├── doctor.rs              # Pre-flight diagnostic engine (6 subsystem health checks)
│   ├── notification.rs        # Thread-safe desktop notifications via libnotify
│   ├── setup.rs               # Automated setup, asset deployment, systemd registration
│   ├── autostart.rs           # Autostart inspector and manager (systemd & XDG desktop)
│   ├── ui_service.rs          # Standalone graphical UI runner (Tray + HUD over IPC stream)
│   ├── tray.rs                # StatusNotifierItem system tray implementation via ksni
│   ├── audio/
│   │   ├── mod.rs             # Audio subsystem orchestration and device discovery
│   │   ├── recorder.rs        # cpal input stream with lock-free SPSC ring buffer
│   │   ├── resampler.rs       # rubato bandlimited sinc resampling to 16kHz mono PCM
│   │   ├── vad.rs             # Concurrent RMS energy silence gating
│   │   ├── feedback.rs        # In-memory pure PCM sine-wave earcon synthesizer
│   │   └── denoise.rs         # Spectral noise suppression hooks
│   ├── transcribe/
│   │   ├── mod.rs             # STT client orchestrator
│   │   ├── client.rs          # Reqwest multipart/form-data client for /v1/audio/transcriptions
│   │   └── formatting.rs      # Formatting modes (snake_case, camelCase, etc.) and vocabulary
│   ├── output/
│   │   ├── mod.rs             # OutputManager coordinating clipboard and keyboard injection
│   │   ├── clipboard.rs       # Wayland wl-copy and arboard clipboard manager
│   │   └── injector.rs        # /dev/uinput virtual keyboard Ctrl+V and text typing
│   ├── hotkey/
│   │   ├── mod.rs             # Hotkey module coordinator
│   │   ├── state.rs           # Push-To-Talk vs Hands-Free Toggle state machine
│   │   ├── evdev_listener.rs  # Direct Linux hardware event monitor (/dev/input/event*)
│   │   ├── ipc.rs             # Unix domain socket server/client with newline framing & events
│   │   └── portal.rs          # XDG Desktop Portal GlobalShortcuts fallback
│   ├── history/
│   │   ├── mod.rs             # SQLite WAL-mode durable transcription history store
│   │   └── gui.rs             # Slint-based standalone history viewer & audio player
│   ├── hud/
│   │   ├── mod.rs             # Floating status pill overlay coordinator
│   │   └── app.rs             # Wayland/eframe non-focus-stealing HUD overlay
│   └── gui/
│       └── mod.rs             # Slint settings panel, live VU meter, and latency tester
├── ui/                        # Slint declarative UI definitions
│   ├── history.slint          # Transcription history UI layout
│   └── settings.slint         # Configuration settings UI layout
├── tests/                     # Integration tests
│   ├── cli_tests.rs           # assert_cmd CLI subcommand suite
│   └── pipeline_integration_test.rs # wiremock end-to-end STT transcription pipeline
├── systemd/
│   ├── openwhisper.service    # User systemd service unit (headless core daemon)
│   └── openwhisper-ui.service # User systemd service unit (graphical UI & tray)
├── desktop/
│   ├── net.local.openwhisper.desktop          # Toggle dictation launcher
│   ├── net.local.openwhisper.settings.desktop # Settings panel launcher
│   └── net.local.openwhisper-ui.desktop       # Graphical UI service launcher & autostart
└── assets/icons/              # Scalable SVG vector icons (app & tray states)
```

---

## 4. Standard Agent Verification Commands

Agents must verify code changes using these exact commands prior to finishing tasks:

```bash
# 1. Format check
cargo fmt --check

# 2. Strict linter check (zero warnings permitted, enforced via Cargo.toml [lints])
cargo clippy --all-targets -- -D warnings

# 3. Documentation build verification (zero broken intra-doc links)
cargo doc --no-deps

# 4. Comprehensive test suite (3-tier testing: unit, assert_cmd CLI, wiremock pipeline)
cargo test --all-targets

# 5. End-to-end system diagnostic verification
cargo run -- doctor --json
```

---

## 5. 3-Tier Testing Architecture

1. **Tier 1 (Unit Tests)**: Pure functions, DSP algorithms (`rubato` sinc downmixing, RMS float stability in `vad.rs`), hotkey state machine transitions, and text casing formatters.
2. **Tier 2 (Subsystem Integration Tests)**:
   - `tests/cli_tests.rs`: `assert_cmd` CLI flag, exit code, and subcommand execution testing.
   - `tests/pipeline_integration_test.rs`: `wiremock` mock HTTP server end-to-end Whisper audio multipart upload and error simulation.
3. **Tier 3 (System Diagnostics)**:
   - `openwhisper doctor`: 6-subsystem runtime probe inspecting active hardware, `/dev/uinput`, live daemon IPC socket, and STT endpoint reachability.
