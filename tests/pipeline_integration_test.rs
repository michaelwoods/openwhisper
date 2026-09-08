use hound::{SampleFormat, WavSpec, WavWriter};
use openwhisper::audio::save_recording_to_dir;
use openwhisper::config::Config;
use openwhisper::history::{HistoryEntry, HistoryManager};
use openwhisper::transcribe::TranscriptionClient;
use std::io::Cursor;
use tempfile::tempdir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn generate_synthetic_wav(duration_secs: f32) -> Vec<u8> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec).expect("create wav writer");
        let num_samples = (16000.0 * duration_secs) as usize;
        for i in 0..num_samples {
            let sample = (16000.0 * (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / 16000.0).sin()) as i16;
            writer.write_sample(sample).expect("write sample");
        }
        writer.finalize().expect("finalize wav");
    }
    cursor.into_inner()
}

#[tokio::test]
async fn test_transcription_pipeline_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "Hello from OpenWhisper integration test."
        })))
        .mount(&mock_server)
        .await;

    let mut config = Config::default();
    config.server_url = format!("{}/v1/audio/transcriptions", mock_server.uri());

    let client = TranscriptionClient::new(&config);
    let wav_bytes = generate_synthetic_wav(0.5);

    let result = client.transcribe(wav_bytes).await;
    assert!(result.is_ok(), "Transcription should succeed: {:?}", result.err());
    let text = result.unwrap();
    assert_eq!(text.trim(), "Hello from OpenWhisper integration test.");
}

#[tokio::test]
async fn test_transcription_pipeline_with_api_key() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .and(header("Authorization", "Bearer secret-test-token-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "Authenticated transcription successful."
        })))
        .mount(&mock_server)
        .await;

    let mut config = Config::default();
    config.server_url = format!("{}/v1/audio/transcriptions", mock_server.uri());
    config.api_key = Some("secret-test-token-123".to_string());

    let client = TranscriptionClient::new(&config);
    let wav_bytes = generate_synthetic_wav(0.5);

    let result = client.transcribe(wav_bytes).await;
    assert!(result.is_ok(), "Authenticated transcription should succeed");
    assert_eq!(result.unwrap().trim(), "Authenticated transcription successful.");
}

#[tokio::test]
async fn test_transcription_pipeline_server_error_handling() {
    let mock_server = MockServer::start().await;

    // Simulate 500 Internal Server Error
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal inference failure"))
        .mount(&mock_server)
        .await;

    let mut config = Config::default();
    config.server_url = format!("{}/v1/audio/transcriptions", mock_server.uri());

    let client = TranscriptionClient::new(&config);
    let wav_bytes = generate_synthetic_wav(0.5);

    let result = client.transcribe(wav_bytes).await;
    assert!(result.is_err(), "Server 500 should return an Err");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("500") || err_msg.contains("Internal inference failure"));
}

#[tokio::test]
async fn test_transcription_pipeline_empty_audio_rejected() {
    let config = Config::default();
    let client = TranscriptionClient::new(&config);

    let result = client.transcribe(Vec::new()).await;
    assert!(result.is_err(), "Empty audio buffer must fail before sending network request");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("empty"));
}

#[test]
fn test_history_persistence_and_audio_save_integration() {
    let temp = tempdir().expect("create temp dir");
    let temp_path = temp.path();

    let wav_bytes = generate_synthetic_wav(1.0);
    let transcript = "Testing audio disk save and history linkage";

    // 1. Save audio and transcript to directory
    let saved_wav = save_recording_to_dir(temp_path, &wav_bytes, transcript)
        .expect("save recording should succeed");
    assert!(saved_wav.exists());
    let saved_txt = saved_wav.with_extension("txt");
    assert!(saved_txt.exists());
    let text_on_disk = std::fs::read_to_string(&saved_txt).expect("read txt");
    assert_eq!(text_on_disk.trim(), transcript);

    // 2. Open temporary SQLite database
    let db_file = temp_path.join("test_history.sqlite3");
    let history_mgr = HistoryManager::new(Some(db_file)).expect("open history db");

    let entry = HistoryEntry {
        id: None,
        timestamp: "2026-09-08T19:00:00Z".to_string(),
        text: transcript.to_string(),
        raw_text: Some(transcript.to_string()),
        duration_secs: 1.0,
        char_count: transcript.len(),
        model: "whisper-base".to_string(),
        output_mode: "clipboard_and_typing".to_string(),
        audio_path: None, // initially unlinked
    };

    let id = history_mgr.record(&entry).expect("record history");
    assert!(id > 0);

    // Verify search works
    let search_res = history_mgr.search("linkage", 5).expect("search");
    assert_eq!(search_res.len(), 1);
    assert_eq!(search_res[0].id, Some(id));

    // 3. Run backfill to link the saved WAV audio file
    let linked_count = history_mgr.backfill_audio_paths(temp_path).expect("backfill");
    assert_eq!(linked_count, 1);

    let updated_entry = history_mgr.get_by_id(id).expect("get").expect("entry exists");
    assert_eq!(updated_entry.audio_path, Some(saved_wav.to_string_lossy().to_string()));
}
