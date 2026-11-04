//! TTS dataset exporter (`openwhisper export-tts-dataset`).
//!
//! Turns the paired `<name>.wav` / `<name>.txt` recordings that OpenWhisper writes when
//! `save_audio_dir` is configured into training manifests for LJSpeech, Piper and XTTS.
//!
//! This is an explicit, user-invoked export. It never touches the dictation path and it is
//! strictly **read-only on the source directory**: source files are opened for reading and
//! copied, never modified, moved, renamed or deleted. The only writes go to the output
//! directory.
//!
//! # Pipeline
//!
//! For every `*.wav` file directly inside the source directory (not recursive), in sorted
//! file-name order:
//!
//! 1. **Integrity** – the WAV must open and decode completely via `hound`. Sample rate,
//!    channel count and duration (`frames / sample_rate`) are read. Failures are recorded
//!    as `corrupt` and skipped; they are never fatal.
//! 2. **Transcript** – the sibling `<name>.txt` must exist, be readable UTF-8 and be
//!    non-empty after whitespace cleanup (`missing_transcript` / `empty_transcript`).
//! 3. **Minimum duration** – clips shorter than `min_duration_secs` are dropped (`too_short`).
//! 4. **Silence / noise gate** – see [`analyze_wav`] (`silent`).
//! 5. **Split** – surviving clips are partitioned into train / validation by [`assign_splits`].
//!
//! # Silence decision
//!
//! The gate reuses the RMS energy scheme of the live VAD ([`VadDetector::calculate_rms`],
//! threshold `vad_energy_threshold`, default `0.015`, and the VAD's default
//! `min_speech_duration` of 300 ms). The clip is cut into consecutive 100 ms windows
//! (all channels interleaved, samples normalised to `[-1.0, 1.0]`). A window is *active* when
//! its RMS is `>=` the threshold. A clip is kept only if its active windows add up to at least
//! `min(min_speech_duration, clip duration)`. Ambient noise (RMS typically `< 0.005`) and
//! digital silence therefore never pass, while a single click does not either. The decision is
//! a pure function of the samples, so it is fully deterministic. Note that RMS cannot tell
//! *loud* noise from speech; that is a limitation shared with the VAD.
//!
//! # Deterministic split
//!
//! Clips are ranked by a seeded 64-bit hash of their id (the file stem), using FNV-1a with a
//! SplitMix64 finaliser (implemented here, so it is stable across Rust versions and
//! platforms). The first `round(n * (1 - train_ratio))` clips in rank order form the
//! validation set, the rest the training set. The result depends only on the set of ids, the
//! seed and the ratio – never on directory iteration order, timestamps or the clock.
//!
//! # Output layout
//!
//! ```text
//! <output>/wavs/<id>.wav            copies of every exported clip (all formats)
//! <output>/metadata.csv             LJSpeech, all clips:   filename|transcript|normalized_transcript
//! <output>/metadata_train.csv       LJSpeech, train split  (same columns)
//! <output>/metadata_val.csv         LJSpeech, validation split (same columns)
//! <output>/piper_train.jsonl        Piper JSON Lines, train split
//! <output>/piper_val.jsonl          Piper JSON Lines, validation split
//! <output>/xtts_metadata_train.csv  XTTS / Coqui CSV, train split
//! <output>/xtts_metadata_eval.csv   XTTS / Coqui CSV, validation split
//! ```
//!
//! * **LJSpeech** files have no header; `filename` is the id **without** extension (the audio
//!   lives at `wavs/<filename>.wav`); `transcript` is the whitespace-cleaned transcript;
//!   `normalized_transcript` additionally has numbers spelled out, a few abbreviations
//!   expanded and typographic punctuation mapped to ASCII (see [`normalize_transcript`]).
//! * **Piper** JSONL – one object per line with the fields `audio_path` (relative to the
//!   output directory), `text`, `normalized_text`, `speaker`, `duration_secs`, `sample_rate`.
//! * **XTTS** pipe-delimited CSV with the header `audio_file|text|speaker_name`
//!   (Coqui's `coqui` formatter); `audio_file` is relative to the output directory.
//!
//! Pipe characters inside transcripts are replaced by a space in the pipe-delimited files so
//! the columns stay unambiguous. Only the manifests selected by [`ExportFormat`] are written;
//! the `wavs/` directory is always populated. Re-using an output directory overwrites the
//! manifests and clips of the same name but never removes other files.

use crate::audio::{VadConfig, VadDetector};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Default minimum clip duration in seconds.
pub const DEFAULT_MIN_DURATION_SECS: f32 = 0.5;
/// Default fraction of clips assigned to the training split (95 / 5).
pub const DEFAULT_TRAIN_RATIO: f64 = 0.95;
/// Default seed for the deterministic train/validation split.
pub const DEFAULT_SEED: u64 = 42;
/// Speaker label written into Piper and XTTS manifests.
pub const DEFAULT_SPEAKER: &str = "openwhisper";
/// Window length used by the silence gate.
const SPEECH_WINDOW_SECS: f64 = 0.1;

/// Which manifest flavour(s) to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// LJSpeech `metadata*.csv`
    #[value(name = "ljspeech")]
    LjSpeech,
    /// Piper JSON Lines manifests
    #[value(name = "piper")]
    Piper,
    /// XTTS / Coqui CSV manifests
    #[value(name = "xtts")]
    Xtts,
    /// All of the above
    #[value(name = "all")]
    All,
}

impl ExportFormat {
    fn includes(self, other: ExportFormat) -> bool {
        self == ExportFormat::All || self == other
    }
}

/// Options for [`export_dataset`].
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Directory containing the paired `.wav` / `.txt` recordings.
    pub source_dir: PathBuf,
    /// Directory the dataset is written to.
    pub output_dir: PathBuf,
    /// Manifest format(s) to write.
    pub format: ExportFormat,
    /// Clips shorter than this many seconds are dropped.
    pub min_duration_secs: f32,
    /// Fraction of clips assigned to the training split, in `[0.0, 1.0]`.
    pub train_ratio: f64,
    /// Seed for the deterministic split.
    pub seed: u64,
    /// RMS energy threshold of the silence gate (same scheme as the VAD).
    pub silence_threshold: f32,
}

impl ExportOptions {
    /// Options with all documented defaults for the given directories.
    pub fn new(source_dir: impl Into<PathBuf>, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            source_dir: source_dir.into(),
            output_dir: output_dir.into(),
            format: ExportFormat::All,
            min_duration_secs: DEFAULT_MIN_DURATION_SECS,
            train_ratio: DEFAULT_TRAIN_RATIO,
            seed: DEFAULT_SEED,
            silence_threshold: VadConfig::default().energy_threshold,
        }
    }
}

/// Why a recording was not exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The WAV could not be opened or fully decoded.
    Corrupt,
    /// No readable sibling `.txt` transcript.
    MissingTranscript,
    /// The transcript is empty after whitespace cleanup.
    EmptyTranscript,
    /// Shorter than the minimum duration.
    TooShort,
    /// Pure silence or noise below the energy threshold.
    Silent,
}

/// Number of skipped recordings per reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SkipCounts {
    pub corrupt: usize,
    pub missing_transcript: usize,
    pub empty_transcript: usize,
    pub too_short: usize,
    pub silent: usize,
}

impl SkipCounts {
    /// Total number of skipped recordings.
    pub fn total(&self) -> usize {
        self.corrupt
            + self.missing_transcript
            + self.empty_transcript
            + self.too_short
            + self.silent
    }
}

/// A skipped recording and the reason it was skipped.
#[derive(Debug, Clone, Serialize)]
pub struct SkippedFile {
    pub file: String,
    pub reason: SkipReason,
    pub detail: Option<String>,
}

/// Train / validation partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Train,
    Validation,
}

/// An exported clip and its split assignment.
#[derive(Debug, Clone, Serialize)]
pub struct ClipRecord {
    pub id: String,
    pub split: Split,
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Machine-readable result of an export run (also rendered for humans by
/// [`ExportSummary::format_report`]).
#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    pub source_dir: String,
    pub output_dir: String,
    pub format: ExportFormat,
    pub min_duration_secs: f32,
    pub train_ratio: f64,
    pub seed: u64,
    pub silence_threshold: f32,
    /// Number of `.wav` files found in the source directory.
    pub scanned: usize,
    pub exported: usize,
    pub train: usize,
    pub validation: usize,
    pub skipped: SkipCounts,
    pub total_duration_secs: f64,
    /// Distinct sample rates among exported clips (more than one means mixed rates).
    pub sample_rates: Vec<u32>,
    pub skipped_files: Vec<SkippedFile>,
    /// Every exported clip with its split assignment, sorted by id.
    pub clips: Vec<ClipRecord>,
}

impl ExportSummary {
    fn skip(&mut self, file: &str, reason: SkipReason, detail: Option<String>) {
        let c = &mut self.skipped;
        match reason {
            SkipReason::Corrupt => c.corrupt += 1,
            SkipReason::MissingTranscript => c.missing_transcript += 1,
            SkipReason::EmptyTranscript => c.empty_transcript += 1,
            SkipReason::TooShort => c.too_short += 1,
            SkipReason::Silent => c.silent += 1,
        }
        self.skipped_files.push(SkippedFile {
            file: file.to_string(),
            reason,
            detail,
        });
    }

    /// Human-readable report (intended for `stderr`).
    pub fn format_report(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "OpenWhisper TTS dataset export");
        let _ = writeln!(s, "  source:   {}", self.source_dir);
        let _ = writeln!(s, "  output:   {}", self.output_dir);
        let _ = writeln!(s, "  scanned:  {} .wav file(s)", self.scanned);
        let _ = writeln!(
            s,
            "  exported: {} clip(s), {:.1}s of audio (train {}, validation {}; ratio {}, seed {})",
            self.exported,
            self.total_duration_secs,
            self.train,
            self.validation,
            self.train_ratio,
            self.seed
        );
        let k = &self.skipped;
        let _ = writeln!(
            s,
            "  skipped:  {} (corrupt {}, missing transcript {}, empty transcript {}, too short {}, silent {})",
            k.total(),
            k.corrupt,
            k.missing_transcript,
            k.empty_transcript,
            k.too_short,
            k.silent
        );
        if !self.sample_rates.is_empty() {
            let rates: Vec<String> = self
                .sample_rates
                .iter()
                .map(|r| format!("{r} Hz"))
                .collect();
            let _ = writeln!(s, "  sample rates: {}", rates.join(", "));
            if self.sample_rates.len() > 1 {
                let _ = writeln!(
                    s,
                    "  warning: mixed sample rates; most trainers expect a single rate"
                );
            }
        }
        if !self.skipped_files.is_empty() {
            let _ = writeln!(s, "Skipped files:");
            for f in &self.skipped_files {
                match &f.detail {
                    Some(d) => {
                        let _ = writeln!(s, "  {}  {:?} ({})", f.file, f.reason, d);
                    }
                    None => {
                        let _ = writeln!(s, "  {}  {:?}", f.file, f.reason);
                    }
                }
            }
        }
        if !self.clips.is_empty() {
            let _ = writeln!(s, "Split assignment:");
            for c in &self.clips {
                let label = match c.split {
                    Split::Train => "train",
                    Split::Validation => "validation",
                };
                let _ = writeln!(s, "  {:<10}  {}  {:.2}s", label, c.id, c.duration_secs);
            }
        }
        s
    }
}

/// Facts about a decoded WAV clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipAnalysis {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: f64,
    /// Total length of 100 ms windows whose RMS reached the silence threshold.
    pub active_secs: f64,
}

impl ClipAnalysis {
    /// Whether the clip counts as pure silence/noise (see the module docs).
    pub fn is_silent(&self) -> bool {
        let required = VadConfig::default()
            .min_speech_duration
            .as_secs_f64()
            .min(self.duration_secs);
        self.active_secs <= 0.0 || self.active_secs < required
    }
}

/// Opens, fully decodes and analyses a WAV file.
///
/// Reads sample rate and channel count from the header, computes the duration from the frame
/// count and measures windowed RMS energy with [`VadDetector::calculate_rms`]. Any error
/// (unparseable header, truncated data, invalid spec) means the file is corrupt.
pub fn analyze_wav(path: &Path, silence_threshold: f32) -> Result<ClipAnalysis> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("cannot open WAV {}", path.display()))?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        bail!(
            "invalid WAV header (channels {}, sample rate {})",
            spec.channels,
            spec.sample_rate
        );
    }
    let channels = usize::from(spec.channels);

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .context("failed to decode float samples")?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .context("failed to decode integer samples")?
        }
    };

    let frames = (samples.len() / channels) as u64;
    let duration_secs = frames as f64 / f64::from(spec.sample_rate);

    let window_frames = ((f64::from(spec.sample_rate) * SPEECH_WINDOW_SECS) as usize).max(1);
    let mut active_secs = 0.0;
    for window in samples.chunks(window_frames * channels) {
        if VadDetector::calculate_rms(window) >= silence_threshold {
            active_secs += (window.len() / channels) as f64 / f64::from(spec.sample_rate);
        }
    }

    Ok(ClipAnalysis {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        duration_secs,
        active_secs,
    })
}

/// Collapses all whitespace runs (including newlines) to single spaces and trims.
pub fn clean_transcript(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// LJSpeech-style normalisation of an already cleaned transcript.
///
/// * typographic quotes, dashes and ellipsis are mapped to ASCII;
/// * `Mr.`, `Mrs.`, `Ms.`, `Dr.` are expanded; `&` becomes `and`, `%` becomes `percent`;
/// * digit runs (with optional thousands commas and a decimal fraction) are spelled out,
///   e.g. `1,200` → `one thousand two hundred`, `3.5` → `three point five`.
pub fn normalize_transcript(clean: &str) -> String {
    let mapped: String = clean
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => "'".to_string(),
            '\u{201C}' | '\u{201D}' => "\"".to_string(),
            '\u{2013}' | '\u{2014}' => "-".to_string(),
            '\u{2026}' => "...".to_string(),
            '&' => " and ".to_string(),
            '%' => " percent".to_string(),
            c => c.to_string(),
        })
        .collect();
    let expanded: Vec<&str> = mapped
        .split_whitespace()
        .map(|t| match t {
            "Mr." => "Mister",
            "Mrs." => "Missus",
            "Ms." => "Ms",
            "Dr." => "Doctor",
            t => t,
        })
        .collect();
    clean_transcript(&expand_numbers(&expanded.join(" ")))
}

fn is_thousands_group(chars: &[char], comma_idx: usize) -> bool {
    let g = comma_idx + 1;
    chars
        .get(g..g + 3)
        .is_some_and(|d| d.iter().all(char::is_ascii_digit))
        && !chars.get(g + 3).is_some_and(char::is_ascii_digit)
}

fn expand_numbers(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        if out.chars().last().is_some_and(char::is_alphanumeric) {
            out.push(' ');
        }
        let mut int_part = String::new();
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                int_part.push(chars[i]);
                i += 1;
            } else if chars[i] == ',' && is_thousands_group(&chars, i) {
                i += 1;
            } else {
                break;
            }
        }
        let mut frac_part = String::new();
        if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                frac_part.push(chars[i]);
                i += 1;
            }
        }
        match int_part.parse::<u64>() {
            Ok(n) if int_part.len() <= 18 => out.push_str(&number_to_words(n)),
            _ => out.push_str(&spell_digits(&int_part)),
        }
        if !frac_part.is_empty() {
            out.push_str(" point ");
            out.push_str(&spell_digits(&frac_part));
        }
        if chars.get(i).is_some_and(|c| c.is_alphabetic()) {
            out.push(' ');
        }
    }
    out
}

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
const SCALES: [&str; 6] = [
    "",
    "thousand",
    "million",
    "billion",
    "trillion",
    "quadrillion",
];

fn spell_digits(digits: &str) -> String {
    digits
        .chars()
        .filter_map(|c| c.to_digit(10))
        .map(|d| ONES[d as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

fn below_thousand(n: u64) -> String {
    let mut parts: Vec<String> = Vec::new();
    let hundreds = n / 100;
    let rest = n % 100;
    if hundreds > 0 {
        parts.push(format!("{} hundred", ONES[hundreds as usize]));
    }
    if rest > 0 {
        if rest < 20 {
            parts.push(ONES[rest as usize].to_string());
        } else if rest.is_multiple_of(10) {
            parts.push(TENS[(rest / 10) as usize].to_string());
        } else {
            parts.push(format!(
                "{}-{}",
                TENS[(rest / 10) as usize],
                ONES[(rest % 10) as usize]
            ));
        }
    }
    parts.join(" ")
}

/// Spells out a non-negative integer (up to 10^18) in English.
fn number_to_words(n: u64) -> String {
    if n == 0 {
        return ONES[0].to_string();
    }
    let mut groups: Vec<String> = Vec::new();
    let mut rest = n;
    let mut scale = 0;
    while rest > 0 {
        let chunk = rest % 1000;
        if chunk > 0 {
            let words = below_thousand(chunk);
            groups.push(if scale == 0 {
                words
            } else {
                format!("{} {}", words, SCALES[scale])
            });
        }
        rest /= 1000;
        scale += 1;
    }
    groups.reverse();
    groups.join(" ")
}

/// Seeded, platform-independent 64-bit hash (FNV-1a + SplitMix64 finaliser).
fn split_key(seed: u64, id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.to_le_bytes().iter().chain(id.as_bytes()) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

/// Deterministically partitions `ids` into train / validation.
///
/// Returns one [`Split`] per input id, in input order. The first
/// `round(n * (1 - train_ratio))` ids ranked by a seeded hash form the validation set. The
/// outcome depends only on the *set* of ids, `train_ratio` and `seed`, not on input order.
pub fn assign_splits(ids: &[String], train_ratio: f64, seed: u64) -> Vec<Split> {
    let n = ids.len();
    let val_count = (((n as f64) * (1.0 - train_ratio)).round() as usize).min(n);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        split_key(seed, &ids[a])
            .cmp(&split_key(seed, &ids[b]))
            .then_with(|| ids[a].cmp(&ids[b]))
    });
    let mut splits = vec![Split::Train; n];
    for &i in order.iter().take(val_count) {
        splits[i] = Split::Validation;
    }
    splits
}

struct Clip {
    id: String,
    wav_path: PathBuf,
    text: String,
    normalized: String,
    analysis: ClipAnalysis,
}

fn pipe_safe(s: &str) -> String {
    s.replace('|', " ")
}

fn validate_options(opts: &ExportOptions) -> Result<()> {
    if !opts.train_ratio.is_finite() || !(0.0..=1.0).contains(&opts.train_ratio) {
        bail!(
            "train ratio must be between 0.0 and 1.0 (got {})",
            opts.train_ratio
        );
    }
    if !opts.min_duration_secs.is_finite() || opts.min_duration_secs < 0.0 {
        bail!(
            "minimum duration must be a non-negative number of seconds (got {})",
            opts.min_duration_secs
        );
    }
    if !opts.silence_threshold.is_finite() || opts.silence_threshold < 0.0 {
        bail!(
            "silence threshold must be non-negative (got {})",
            opts.silence_threshold
        );
    }
    if !opts.source_dir.is_dir() {
        bail!(
            "source directory {} does not exist or is not a directory",
            opts.source_dir.display()
        );
    }
    Ok(())
}

/// Exports the recordings in `opts.source_dir` as a TTS dataset in `opts.output_dir`.
///
/// Never writes to `stdout`/`stderr` and never modifies the source directory; see the module
/// docs for the full contract. Unparseable recordings are reported in the returned summary and
/// skipped; only invalid options and output I/O failures are errors.
pub fn export_dataset(opts: &ExportOptions) -> Result<ExportSummary> {
    validate_options(opts)?;

    let wavs_dir = opts.output_dir.join("wavs");
    let src_canon = opts.source_dir.canonicalize()?;
    let out_abs = match opts.output_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => std::path::absolute(&opts.output_dir)?,
    };
    if src_canon == out_abs || src_canon == out_abs.join("wavs") {
        bail!("output directory must differ from the source directory (source is read-only)");
    }
    std::fs::create_dir_all(&wavs_dir)
        .with_context(|| format!("cannot create output directory {}", wavs_dir.display()))?;

    let mut wav_paths: Vec<PathBuf> = std::fs::read_dir(&opts.source_dir)
        .with_context(|| format!("cannot read {}", opts.source_dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "wav"))
        .collect();
    wav_paths.sort();

    let mut summary = ExportSummary {
        source_dir: opts.source_dir.display().to_string(),
        output_dir: opts.output_dir.display().to_string(),
        format: opts.format,
        min_duration_secs: opts.min_duration_secs,
        train_ratio: opts.train_ratio,
        seed: opts.seed,
        silence_threshold: opts.silence_threshold,
        scanned: wav_paths.len(),
        exported: 0,
        train: 0,
        validation: 0,
        skipped: SkipCounts::default(),
        total_duration_secs: 0.0,
        sample_rates: Vec::new(),
        skipped_files: Vec::new(),
        clips: Vec::new(),
    };

    let mut clips: Vec<Clip> = Vec::new();
    for wav in wav_paths {
        let file_name = wav
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = wav
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let analysis = match analyze_wav(&wav, opts.silence_threshold) {
            Ok(a) => a,
            Err(e) => {
                summary.skip(&file_name, SkipReason::Corrupt, Some(format!("{e:#}")));
                continue;
            }
        };
        let raw = match std::fs::read_to_string(wav.with_extension("txt")) {
            Ok(raw) => raw,
            Err(e) => {
                summary.skip(
                    &file_name,
                    SkipReason::MissingTranscript,
                    Some(e.to_string()),
                );
                continue;
            }
        };
        let text = clean_transcript(&raw);
        if text.is_empty() {
            summary.skip(&file_name, SkipReason::EmptyTranscript, None);
            continue;
        }
        if analysis.duration_secs < f64::from(opts.min_duration_secs) {
            summary.skip(
                &file_name,
                SkipReason::TooShort,
                Some(format!("{:.3}s", analysis.duration_secs)),
            );
            continue;
        }
        if analysis.is_silent() {
            summary.skip(&file_name, SkipReason::Silent, None);
            continue;
        }
        let normalized = normalize_transcript(&text);
        clips.push(Clip {
            id,
            wav_path: wav,
            text,
            normalized,
            analysis,
        });
    }

    let ids: Vec<String> = clips.iter().map(|c| c.id.clone()).collect();
    let splits = assign_splits(&ids, opts.train_ratio, opts.seed);

    let mut rates = BTreeSet::new();
    for (clip, split) in clips.iter().zip(&splits) {
        std::fs::copy(&clip.wav_path, wavs_dir.join(format!("{}.wav", clip.id)))
            .with_context(|| format!("failed to copy {}", clip.wav_path.display()))?;
        rates.insert(clip.analysis.sample_rate);
        summary.total_duration_secs += clip.analysis.duration_secs;
        match split {
            Split::Train => summary.train += 1,
            Split::Validation => summary.validation += 1,
        }
        summary.clips.push(ClipRecord {
            id: clip.id.clone(),
            split: *split,
            duration_secs: clip.analysis.duration_secs,
            sample_rate: clip.analysis.sample_rate,
            channels: clip.analysis.channels,
        });
    }
    summary.exported = clips.len();
    summary.sample_rates = rates.into_iter().collect();

    write_manifests(opts, &clips, &splits)?;
    Ok(summary)
}

fn write_manifests(opts: &ExportOptions, clips: &[Clip], splits: &[Split]) -> Result<()> {
    let out = &opts.output_dir;
    let write = |name: &str, body: String| -> Result<()> {
        std::fs::write(out.join(name), body).with_context(|| format!("failed to write {name}"))
    };
    let select = |want: Option<Split>| -> Vec<&Clip> {
        clips
            .iter()
            .zip(splits)
            .filter(|(_, s)| want.is_none_or(|w| w == **s))
            .map(|(c, _)| c)
            .collect()
    };
    let (train, val, all) = (
        select(Some(Split::Train)),
        select(Some(Split::Validation)),
        select(None),
    );

    if opts.format.includes(ExportFormat::LjSpeech) {
        let lj = |set: &[&Clip]| -> String {
            set.iter()
                .map(|c| {
                    format!(
                        "{}|{}|{}\n",
                        pipe_safe(&c.id),
                        pipe_safe(&c.text),
                        pipe_safe(&c.normalized)
                    )
                })
                .collect()
        };
        write("metadata.csv", lj(&all))?;
        write("metadata_train.csv", lj(&train))?;
        write("metadata_val.csv", lj(&val))?;
    }

    if opts.format.includes(ExportFormat::Piper) {
        let piper = |set: &[&Clip]| -> Result<String> {
            let mut body = String::new();
            for c in set {
                let line = serde_json::json!({
                    "audio_path": format!("wavs/{}.wav", c.id),
                    "text": c.text,
                    "normalized_text": c.normalized,
                    "speaker": DEFAULT_SPEAKER,
                    "duration_secs": c.analysis.duration_secs,
                    "sample_rate": c.analysis.sample_rate,
                });
                body.push_str(&serde_json::to_string(&line)?);
                body.push('\n');
            }
            Ok(body)
        };
        write("piper_train.jsonl", piper(&train)?)?;
        write("piper_val.jsonl", piper(&val)?)?;
    }

    if opts.format.includes(ExportFormat::Xtts) {
        let xtts = |set: &[&Clip]| -> String {
            let mut body = String::from("audio_file|text|speaker_name\n");
            for c in set {
                body.push_str(&format!(
                    "wavs/{}.wav|{}|{}\n",
                    pipe_safe(&c.id),
                    pipe_safe(&c.text),
                    DEFAULT_SPEAKER
                ));
            }
            body
        };
        write("xtts_metadata_train.csv", xtts(&train))?;
        write("xtts_metadata_eval.csv", xtts(&val))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_to_words() {
        assert_eq!(number_to_words(0), "zero");
        assert_eq!(number_to_words(7), "seven");
        assert_eq!(number_to_words(13), "thirteen");
        assert_eq!(number_to_words(40), "forty");
        assert_eq!(number_to_words(42), "forty-two");
        assert_eq!(number_to_words(100), "one hundred");
        assert_eq!(number_to_words(1200), "one thousand two hundred");
        assert_eq!(number_to_words(2_000_015), "two million fifteen");
    }

    #[test]
    fn test_normalize_transcript() {
        assert_eq!(
            normalize_transcript("Dr. Smith paid 1,200 dollars"),
            "Doctor Smith paid one thousand two hundred dollars"
        );
        assert_eq!(
            normalize_transcript("It is 3.5 now"),
            "It is three point five now"
        );
        assert_eq!(
            normalize_transcript("Tom & Jerry \u{2014} 50% \u{201C}ok\u{201D}"),
            "Tom and Jerry - fifty percent \"ok\""
        );
        assert_eq!(normalize_transcript("plain words"), "plain words");
    }

    #[test]
    fn test_clean_transcript() {
        assert_eq!(clean_transcript("  hello \n\tworld  "), "hello world");
        assert_eq!(clean_transcript(" \n "), "");
    }

    #[test]
    fn test_assign_splits_counts_and_determinism() {
        let ids: Vec<String> = (0..40).map(|i| format!("clip_{i:03}")).collect();
        let a = assign_splits(&ids, 0.95, 42);
        let b = assign_splits(&ids, 0.95, 42);
        assert_eq!(a, b);
        assert_eq!(a.iter().filter(|s| **s == Split::Validation).count(), 2);

        let mut reversed = ids.clone();
        reversed.reverse();
        let mut r = assign_splits(&reversed, 0.95, 42);
        r.reverse();
        assert_eq!(a, r, "split must not depend on input order");

        let other = assign_splits(&ids, 0.95, 7);
        assert_ne!(a, other, "different seeds must change the partition");
    }

    #[test]
    fn test_assign_splits_edges() {
        let ids: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        assert!(
            assign_splits(&ids, 1.0, 1)
                .iter()
                .all(|s| *s == Split::Train)
        );
        assert!(
            assign_splits(&ids, 0.0, 1)
                .iter()
                .all(|s| *s == Split::Validation)
        );
        assert!(assign_splits(&[], 0.95, 1).is_empty());
    }

    #[test]
    fn test_validate_options_rejects_bad_values() {
        let tmp = tempfile::tempdir().unwrap();
        let mut o = ExportOptions::new(tmp.path(), tmp.path().join("out"));
        o.train_ratio = 1.5;
        assert!(export_dataset(&o).is_err());
        o.train_ratio = f64::NAN;
        assert!(export_dataset(&o).is_err());
        o.train_ratio = 0.9;
        o.min_duration_secs = -1.0;
        assert!(export_dataset(&o).is_err());
        o.min_duration_secs = 0.5;
        o.source_dir = tmp.path().join("missing");
        assert!(export_dataset(&o).is_err());
    }
}
