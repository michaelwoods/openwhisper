use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FormattingMode {
    #[default]
    Standard,
    SnakeCase,
    CamelCase,
    KebabCase,
    Raw,
}

/// Builds the contextual prompt to pass to Whisper's `/v1/audio/transcriptions` endpoint.
/// Merges custom prompt instructions with vocabulary biasing words.
pub fn build_whisper_prompt(vocabulary: &[String], custom_prompt: Option<&str>) -> Option<String> {
    let clean_vocab: Vec<&str> = vocabulary
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let clean_prompt = custom_prompt.map(|s| s.trim()).filter(|s| !s.is_empty());

    match (clean_prompt, !clean_vocab.is_empty()) {
        (Some(prompt), true) => {
            let p = prompt.trim_end_matches('.');
            Some(format!(
                "{}. Technical vocabulary: {}.",
                p,
                clean_vocab.join(", ")
            ))
        }
        (Some(prompt), false) => Some(prompt.to_string()),
        (None, true) => Some(format!("Technical vocabulary: {}.", clean_vocab.join(", "))),
        (None, false) => None,
    }
}

/// Extracts alphanumeric words from a transcript, stripping punctuation.
pub fn extract_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

/// Formats the transcribed text according to the desired `FormattingMode` and optional trailing space.
pub fn format_transcription(text: &str, mode: FormattingMode, trailing_space: bool) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match mode {
        FormattingMode::Standard => {
            let mut res = trimmed.to_string();
            if trailing_space && !res.ends_with(' ') && !res.ends_with('\n') {
                res.push(' ');
            }
            res
        }

        FormattingMode::Raw => {
            // Trim trailing punctuation (period, exclamation, question mark)
            let without_trailing = trimmed.trim_end_matches(['.', '!', '?', ',']);
            let mut res = without_trailing.trim().to_string();
            if trailing_space && !res.ends_with(' ') && !res.ends_with('\n') {
                res.push(' ');
            }
            res
        }

        FormattingMode::SnakeCase => {
            let words = extract_words(trimmed);
            if words.is_empty() {
                return String::new();
            }
            words
                .into_iter()
                .map(|w| w.to_lowercase())
                .collect::<Vec<_>>()
                .join("_")
        }

        FormattingMode::CamelCase => {
            let words = extract_words(trimmed);
            if words.is_empty() {
                return String::new();
            }

            let mut result = String::new();
            for (i, word) in words.into_iter().enumerate() {
                let lower = word.to_lowercase();
                if i == 0 {
                    result.push_str(&lower);
                } else {
                    let mut chars = lower.chars();
                    if let Some(first) = chars.next() {
                        result.extend(first.to_uppercase());
                        result.push_str(chars.as_str());
                    }
                }
            }
            result
        }

        FormattingMode::KebabCase => {
            let words = extract_words(trimmed);
            if words.is_empty() {
                return String::new();
            }
            words
                .into_iter()
                .map(|w| w.to_lowercase())
                .collect::<Vec<_>>()
                .join("-")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_whisper_prompt_combinations() {
        assert_eq!(build_whisper_prompt(&[], None), None);

        let vocab = vec!["Rust".to_string(), "Wayland".to_string()];
        assert_eq!(
            build_whisper_prompt(&vocab, None),
            Some("Technical vocabulary: Rust, Wayland.".to_string())
        );

        assert_eq!(
            build_whisper_prompt(&[], Some("Code dictation.")),
            Some("Code dictation.".to_string())
        );

        assert_eq!(
            build_whisper_prompt(&vocab, Some("Code dictation.")),
            Some("Code dictation. Technical vocabulary: Rust, Wayland.".to_string())
        );
    }

    #[test]
    fn test_extract_words() {
        assert_eq!(extract_words(""), Vec::<String>::new());
        assert_eq!(
            extract_words("Hello, World! 123-test"),
            vec!["Hello", "World", "123", "test"]
        );
    }

    #[test]
    fn test_format_standard() {
        let input = "  Hello, world!  ";
        // Default with trailing space
        assert_eq!(
            format_transcription(input, FormattingMode::Standard, true),
            "Hello, world! "
        );
        // Without trailing space
        assert_eq!(
            format_transcription(input, FormattingMode::Standard, false),
            "Hello, world!"
        );
        // Does not duplicate existing trailing space
        assert_eq!(
            format_transcription("Hello, world! ", FormattingMode::Standard, true),
            "Hello, world! "
        );
    }

    #[test]
    fn test_format_raw() {
        assert_eq!(
            format_transcription("Hello world.", FormattingMode::Raw, true),
            "Hello world "
        );
        assert_eq!(
            format_transcription("Hello world.", FormattingMode::Raw, false),
            "Hello world"
        );
        assert_eq!(
            format_transcription("Is this real?!", FormattingMode::Raw, true),
            "Is this real "
        );
    }

    #[test]
    fn test_format_snake_case() {
        assert_eq!(
            format_transcription("Calculate Audio RMS Value", FormattingMode::SnakeCase, true),
            "calculate_audio_rms_value"
        );
        assert_eq!(
            format_transcription("hello-world_test 123", FormattingMode::SnakeCase, false),
            "hello_world_test_123"
        );
    }

    #[test]
    fn test_format_camel_case() {
        assert_eq!(
            format_transcription("Calculate Audio RMS Value", FormattingMode::CamelCase, true),
            "calculateAudioRmsValue"
        );
        assert_eq!(
            format_transcription("Single", FormattingMode::CamelCase, false),
            "single"
        );
    }

    #[test]
    fn test_format_kebab_case() {
        assert_eq!(
            format_transcription("Calculate Audio RMS Value", FormattingMode::KebabCase, true),
            "calculate-audio-rms-value"
        );
    }

    #[test]
    fn test_format_empty() {
        assert_eq!(
            format_transcription("   ", FormattingMode::SnakeCase, true),
            ""
        );
        assert_eq!(
            format_transcription("", FormattingMode::CamelCase, false),
            ""
        );
    }
}
