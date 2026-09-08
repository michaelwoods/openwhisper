use anyhow::{bail, Context, Result};
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::formatting::{build_whisper_prompt, format_transcription, FormattingMode};
use crate::config::Config;

#[derive(Debug, Clone)]
pub struct TranscriptionClient {
    client: Client,
    server_url: String,
    model: String,
    language: Option<String>,
    prompt: Option<String>,
    temperature: Option<f32>,
    api_key: Option<String>,
    formatting_mode: FormattingMode,
    trailing_space: bool,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: Option<String>,
    error: Option<serde_json::Value>,
}

impl TranscriptionClient {
    pub fn new(config: &Config) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| Client::new());

        let prompt = build_whisper_prompt(&config.vocabulary, config.prompt.as_deref());

        Self {
            client,
            server_url: config.server_url.clone(),
            model: config.model.clone(),
            language: config.language.clone(),
            prompt,
            temperature: config.temperature,
            api_key: config.api_key.clone(),
            formatting_mode: config.formatting_mode,
            trailing_space: config.trailing_space,
        }
    }

    #[allow(dead_code)]
    pub fn prompt(&self) -> Option<&str> {
        self.prompt.as_deref()
    }

    #[allow(dead_code)]
    pub fn formatting_mode(&self) -> FormattingMode {
        self.formatting_mode
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub async fn transcribe(&self, wav_bytes: Vec<u8>) -> Result<String> {
        if wav_bytes.is_empty() {
            bail!("Audio data is empty, cannot transcribe");
        }

        tracing::info!(
            "Sending {} bytes of WAV audio to {} (model: {})",
            wav_bytes.len(),
            self.server_url,
            self.model
        );

        let audio_part = Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .context("Failed to create multipart audio part")?;

        let mut form = Form::new()
            .text("model", self.model.clone())
            .part("file", audio_part);

        if let Some(ref lang) = self.language {
            form = form.text("language", lang.clone());
        }

        if let Some(ref prompt) = self.prompt {
            form = form.text("prompt", prompt.clone());
        }

        if let Some(temp) = self.temperature {
            form = form.text("temperature", temp.to_string());
        }

        let mut request = self.client.post(&self.server_url).multipart(form);

        if let Some(ref key) = self.api_key {
            if !key.trim().is_empty() {
                request = request.bearer_auth(key);
            }
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to connect to STT endpoint at {}", self.server_url))?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .context("Failed to read response body from STT endpoint")?;

        if !status.is_success() {
            bail!(
                "STT endpoint returned error status {}: {}",
                status,
                body_text
            );
        }

        parse_transcription_response(&body_text, self.formatting_mode, self.trailing_space)
    }
}

/// Parses the JSON response body from an OpenAI-compatible STT endpoint and formats the output.
pub fn parse_transcription_response(body_text: &str, mode: FormattingMode, trailing_space: bool) -> Result<String> {
    if let Ok(res) = serde_json::from_str::<TranscriptionResponse>(body_text) {
        if let Some(text) = res.text {
            return Ok(format_transcription(&text, mode, trailing_space));
        }
        if let Some(err) = res.error {
            bail!("STT API returned error object: {}", err);
        }
    }

    bail!(
        "Unexpected response format from STT endpoint: {}",
        body_text
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_response_standard() {
        let json = r#"{"text": "Hello world from Whisper."}"#;
        let res = parse_transcription_response(json, FormattingMode::Standard, true).unwrap();
        assert_eq!(res, "Hello world from Whisper. ");

        let res_no_space = parse_transcription_response(json, FormattingMode::Standard, false).unwrap();
        assert_eq!(res_no_space, "Hello world from Whisper.");
    }

    #[test]
    fn test_parse_valid_response_snake_case() {
        let json = r#"{"text": "OpenWhisper Dictation Engine"}"#;
        let res = parse_transcription_response(json, FormattingMode::SnakeCase, true).unwrap();
        assert_eq!(res, "openwhisper_dictation_engine");
    }

    #[test]
    fn test_parse_error_object() {
        let json = r#"{"error": {"message": "Invalid API key"}}"#;
        let err = parse_transcription_response(json, FormattingMode::Standard, true).unwrap_err();
        assert!(err.to_string().contains("Invalid API key"));
    }

    #[test]
    fn test_parse_invalid_json() {
        let text = "<html>502 Bad Gateway</html>";
        let err = parse_transcription_response(text, FormattingMode::Standard, true).unwrap_err();
        assert!(err.to_string().contains("Unexpected response format"));
    }

    #[test]
    fn test_client_prompt_and_formatting_from_config() {
        let mut config = Config::default();
        config.prompt = Some("Technical notes".to_string());
        config.vocabulary = vec!["Wayland".to_string(), "Rust".to_string()];
        config.formatting_mode = FormattingMode::KebabCase;

        let client = TranscriptionClient::new(&config);
        assert_eq!(client.formatting_mode(), FormattingMode::KebabCase);
        assert!(client.prompt().unwrap().contains("Technical notes"));
        assert!(client.prompt().unwrap().contains("Wayland, Rust"));
    }
}
