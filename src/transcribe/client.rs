use anyhow::{bail, Context, Result};
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

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

        Self {
            client,
            server_url: config.server_url.clone(),
            model: config.model.clone(),
            language: config.language.clone(),
            prompt: config.prompt.clone(),
            temperature: config.temperature,
            api_key: config.api_key.clone(),
        }
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

        // Parse JSON response {"text": "..."}
        if let Ok(res) = serde_json::from_str::<TranscriptionResponse>(&body_text) {
            if let Some(text) = res.text {
                return Ok(text.trim().to_string());
            }
            if let Some(err) = res.error {
                bail!("STT API returned error object: {}", err);
            }
        }

        // If not standard object, might be raw text or fallback
        bail!(
            "Unexpected response format from STT endpoint: {}",
            body_text
        );
    }
}
