//! Integration tests for `openwhisper export-tts-dataset` (library API).
//!
//! Fixtures are tiny WAVs generated into temp directories with `hound`.

use hound::{SampleFormat, WavSpec, WavWriter};
use openwhisper::dataset::{
    ExportFormat, ExportOptions, ExportSummary, SkipReason, Split, export_dataset,
};
use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::SystemTime;

const RATE: u32 = 16_000;

fn write_wav(path: &Path, samples: &[i16]) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).unwrap();
    for s in samples {
        w.write_sample(*s).unwrap();
    }
    w.finalize().unwrap();
}

/// 440 Hz sine, amplitude 0.3 (RMS ~0.21, far above the 0.015 threshold).
fn tone(secs: f32) -> Vec<i16> {
    (0..(secs * RATE as f32) as usize)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            ((t * 440.0 * std::f32::consts::TAU).sin() * 0.3 * i16::MAX as f32) as i16
        })
        .collect()
}

/// Deterministic low-level pseudo-noise, amplitude ~0.003 (RMS well below 0.015).
fn quiet_noise(secs: f32) -> Vec<i16> {
    let mut x: u32 = 12345;
    (0..(secs * RATE as f32) as usize)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = ((x >> 16) as i32 % 200) - 100; // +-100 / 32768 ~ 0.003
            v as i16
        })
        .collect()
}

fn add_clip(dir: &Path, name: &str, samples: &[i16], text: &str) {
    write_wav(&dir.join(format!("{name}.wav")), samples);
    std::fs::write(dir.join(format!("{name}.txt")), text).unwrap();
}

fn run(src: &Path, out: &Path) -> ExportSummary {
    export_dataset(&ExportOptions::new(src, out)).unwrap()
}

fn read(out: &Path, name: &str) -> String {
    std::fs::read_to_string(out.join(name)).unwrap()
}

fn skipped_reason(s: &ExportSummary, file: &str) -> Option<SkipReason> {
    s.skipped_files
        .iter()
        .find(|f| f.file == file)
        .map(|f| f.reason)
}

#[test]
fn happy_path_exports_clips_and_manifests() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "a", &tone(1.0), "Hello  world\n");
    add_clip(src.path(), "b", &tone(1.5), "I have 42 apples");

    let s = run(src.path(), out.path());

    assert_eq!(s.scanned, 2);
    assert_eq!(s.exported, 2);
    assert_eq!(s.skipped.total(), 0);
    assert_eq!(s.train + s.validation, 2);
    assert_eq!(s.sample_rates, vec![RATE]);
    assert!((s.total_duration_secs - 2.5).abs() < 1e-6);

    assert!(out.path().join("wavs/a.wav").is_file());
    assert!(out.path().join("wavs/b.wav").is_file());

    // LJSpeech: filename|transcript|normalized_transcript
    assert_eq!(
        read(out.path(), "metadata.csv"),
        "a|Hello world|Hello world\nb|I have 42 apples|I have forty-two apples\n"
    );
    let train = read(out.path(), "metadata_train.csv");
    let val = read(out.path(), "metadata_val.csv");
    assert_eq!(
        train.lines().count() + val.lines().count(),
        2,
        "train+val partition the full set"
    );

    // Piper JSONL
    let piper = read(out.path(), "piper_train.jsonl") + &read(out.path(), "piper_val.jsonl");
    let rows: Vec<serde_json::Value> = piper
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    for r in &rows {
        for key in [
            "audio_path",
            "text",
            "normalized_text",
            "speaker",
            "duration_secs",
            "sample_rate",
        ] {
            assert!(r.get(key).is_some(), "missing piper field {key}");
        }
        assert_eq!(r["sample_rate"], RATE);
    }

    // XTTS CSV with header
    let xt = read(out.path(), "xtts_metadata_train.csv");
    assert!(xt.starts_with("audio_file|text|speaker_name\n"));
    let xe = read(out.path(), "xtts_metadata_eval.csv");
    assert!(xe.starts_with("audio_file|text|speaker_name\n"));
    assert_eq!(xt.lines().count() + xe.lines().count() - 2, 2);
    assert!((xt + &xe).contains("wavs/a.wav|Hello world|openwhisper"));
}

#[test]
fn format_selection_limits_manifests() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "a", &tone(1.0), "hello");

    let mut opts = ExportOptions::new(src.path(), out.path());
    opts.format = ExportFormat::Piper;
    export_dataset(&opts).unwrap();

    assert!(out.path().join("piper_train.jsonl").is_file());
    assert!(!out.path().join("metadata.csv").exists());
    assert!(!out.path().join("xtts_metadata_train.csv").exists());
    assert!(out.path().join("wavs/a.wav").is_file());
}

#[test]
fn short_clips_are_filtered() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "long", &tone(1.0), "long enough");
    add_clip(src.path(), "short", &tone(0.3), "too brief");

    let s = run(src.path(), out.path());
    assert_eq!(s.exported, 1);
    assert_eq!(s.skipped.too_short, 1);
    assert_eq!(skipped_reason(&s, "short.wav"), Some(SkipReason::TooShort));
    assert!(!out.path().join("wavs/short.wav").exists());
    assert!(!read(out.path(), "metadata.csv").contains("too brief"));

    // Baseline: the same clip is kept when the minimum duration is lowered.
    let out2 = tempfile::tempdir().unwrap();
    let mut opts = ExportOptions::new(src.path(), out2.path());
    opts.min_duration_secs = 0.25;
    let s2 = export_dataset(&opts).unwrap();
    assert_eq!(s2.exported, 2);
    assert_eq!(s2.skipped.too_short, 0);
}

#[test]
fn silent_and_noise_clips_are_filtered() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "speech", &tone(1.0), "speech");
    add_clip(src.path(), "zeros", &vec![0i16; RATE as usize], "nothing");
    add_clip(src.path(), "hiss", &quiet_noise(1.0), "hiss");
    // A single 50 ms click inside silence must not count as speech either.
    let mut click = vec![0i16; RATE as usize];
    click[..800].copy_from_slice(&tone(0.05));
    add_clip(src.path(), "click", &click, "click");

    let s = run(src.path(), out.path());
    assert_eq!(s.exported, 1);
    assert_eq!(s.skipped.silent, 3);
    for f in ["zeros.wav", "hiss.wav", "click.wav"] {
        assert_eq!(skipped_reason(&s, f), Some(SkipReason::Silent), "{f}");
    }
    assert_eq!(s.clips[0].id, "speech");

    // Baseline: a threshold below the noise level keeps the hiss (the gate is the RMS scheme,
    // not a hidden constant), while true zeros still never pass.
    let out2 = tempfile::tempdir().unwrap();
    let mut opts = ExportOptions::new(src.path(), out2.path());
    opts.silence_threshold = 0.0005;
    let s2 = export_dataset(&opts).unwrap();
    assert!(s2.clips.iter().any(|c| c.id == "hiss"));
    assert!(!s2.clips.iter().any(|c| c.id == "zeros"));
}

#[test]
fn corrupt_wavs_are_reported_and_skipped() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "good", &tone(1.0), "good clip");

    // Garbage that is not a WAV at all.
    std::fs::write(src.path().join("junk.wav"), b"definitely not a wav").unwrap();
    std::fs::write(src.path().join("junk.txt"), "junk").unwrap();
    // A valid WAV truncated mid-data.
    add_clip(src.path(), "trunc", &tone(1.0), "truncated");
    let full = std::fs::read(src.path().join("trunc.wav")).unwrap();
    std::fs::write(src.path().join("trunc.wav"), &full[..full.len() / 2]).unwrap();
    // Empty file.
    std::fs::write(src.path().join("empty.wav"), b"").unwrap();
    std::fs::write(src.path().join("empty.txt"), "empty").unwrap();

    let s = run(src.path(), out.path());
    assert_eq!(s.scanned, 4);
    assert_eq!(s.exported, 1);
    assert_eq!(s.skipped.corrupt, 3);
    for f in ["junk.wav", "trunc.wav", "empty.wav"] {
        assert_eq!(skipped_reason(&s, f), Some(SkipReason::Corrupt), "{f}");
    }
    assert!(s.format_report().contains("corrupt 3"));
    assert_eq!(s.clips[0].id, "good");
}

#[test]
fn missing_and_empty_transcripts_are_skipped() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "ok", &tone(1.0), "fine");
    write_wav(&src.path().join("notxt.wav"), &tone(1.0));
    add_clip(src.path(), "blank", &tone(1.0), "  \n ");

    let s = run(src.path(), out.path());
    assert_eq!(s.exported, 1);
    assert_eq!(s.skipped.missing_transcript, 1);
    assert_eq!(s.skipped.empty_transcript, 1);
}

fn make_dataset(dir: &Path, names: &[String]) {
    for n in names {
        add_clip(dir, n, &tone(0.6), &format!("transcript for {n}"));
    }
}

fn splits(s: &ExportSummary) -> BTreeMap<String, Split> {
    s.clips.iter().map(|c| (c.id.clone(), c.split)).collect()
}

#[test]
fn split_is_deterministic_and_order_independent() {
    let names: Vec<String> = (0..20).map(|i| format!("clip_{i:02}")).collect();
    let mut reversed = names.clone();
    reversed.reverse();

    let src1 = tempfile::tempdir().unwrap();
    let src2 = tempfile::tempdir().unwrap();
    make_dataset(src1.path(), &names);
    make_dataset(src2.path(), &reversed); // different creation order / mtimes

    let (o1, o2, o3) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let opts = |src: &Path, out: &Path, seed: u64| {
        let mut o = ExportOptions::new(src, out);
        o.train_ratio = 0.75;
        o.seed = seed;
        o
    };
    let a = export_dataset(&opts(src1.path(), o1.path(), 42)).unwrap();
    let b = export_dataset(&opts(src1.path(), o2.path(), 42)).unwrap();
    let c = export_dataset(&opts(src2.path(), o3.path(), 42)).unwrap();

    // Non-vacuous: both partitions populated, with the exact requested ratio.
    assert_eq!(a.train, 15);
    assert_eq!(a.validation, 5);
    assert_eq!(splits(&a), splits(&b));
    assert_eq!(splits(&a), splits(&c));
    for f in ["metadata_train.csv", "metadata_val.csv", "piper_val.jsonl"] {
        assert_eq!(read(o1.path(), f), read(o2.path(), f), "{f}");
        assert_eq!(read(o1.path(), f), read(o3.path(), f), "{f}");
    }
    assert!(!read(o1.path(), "metadata_val.csv").is_empty());

    // Baseline: the seed really drives the partition.
    let o4 = tempfile::tempdir().unwrap();
    let d = export_dataset(&opts(src1.path(), o4.path(), 7)).unwrap();
    assert_ne!(splits(&a), splits(&d));
    assert_eq!(d.validation, 5);

    // Split assignment is reported per clip.
    assert_eq!(a.clips.len(), 20);
    assert!(a.format_report().contains("validation"));
}

fn snapshot(dir: &Path) -> BTreeMap<String, (u64, SystemTime)> {
    let mut m = BTreeMap::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let mut h = DefaultHasher::new();
        std::fs::read(&p).unwrap().hash(&mut h);
        let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        m.insert(
            p.file_name().unwrap().to_string_lossy().into_owned(),
            (h.finish(), mtime),
        );
    }
    m
}

#[test]
fn source_directory_is_never_modified() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "a", &tone(1.0), "alpha");
    add_clip(src.path(), "b", &tone(0.2), "short one");
    add_clip(src.path(), "c", &vec![0i16; 16_000], "silent one");
    std::fs::write(src.path().join("bad.wav"), b"garbage").unwrap();
    std::fs::write(src.path().join("bad.txt"), "garbage").unwrap();

    let before = snapshot(src.path());
    assert_eq!(before.len(), 8);
    let s = run(src.path(), out.path());
    let after = snapshot(src.path());

    assert!(s.exported >= 1, "non-vacuous: the run did real work");
    assert!(s.skipped.total() >= 3);
    assert_eq!(before, after, "content hash, mtime and file set unchanged");
}

#[test]
fn output_must_not_be_the_source_directory() {
    let src = tempfile::tempdir().unwrap();
    add_clip(src.path(), "a", &tone(1.0), "alpha");
    let before = snapshot(src.path());

    assert!(export_dataset(&ExportOptions::new(src.path(), src.path())).is_err());
    assert!(export_dataset(&ExportOptions::new(src.path(), src.path().join("."))).is_err());
    assert_eq!(before, snapshot(src.path()));
    assert!(!src.path().join("wavs").exists());
}

#[test]
fn pipes_in_transcripts_do_not_break_columns() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    add_clip(src.path(), "a", &tone(1.0), "either|or");
    run(src.path(), out.path());
    assert_eq!(read(out.path(), "metadata.csv"), "a|either or|either or\n");
}
