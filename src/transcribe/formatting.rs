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

/// A spoken formatting command that can be expanded into literal punctuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpokenMacro {
    /// `\n` or `\n\n`: swallows surrounding spaces.
    Break(&'static str),
    /// `. , ? ! : ;`: attaches to the previous text, a space follows.
    Mark(char),
    /// `"` or `(`: a space precedes, nothing follows.
    Open(char),
    /// `"` or `)`: attaches to the previous text, a space follows.
    Close(char),
}

/// Spoken phrase table (ROADMAP §1). Phrases are lowercase, single-space separated.
const SPOKEN_MACROS: &[(&str, SpokenMacro)] = &[
    ("new line", SpokenMacro::Break("\n")),
    ("next line", SpokenMacro::Break("\n")),
    ("new paragraph", SpokenMacro::Break("\n\n")),
    ("period", SpokenMacro::Mark('.')),
    ("full stop", SpokenMacro::Mark('.')),
    ("comma", SpokenMacro::Mark(',')),
    ("question mark", SpokenMacro::Mark('?')),
    ("exclamation mark", SpokenMacro::Mark('!')),
    ("exclamation point", SpokenMacro::Mark('!')),
    ("colon", SpokenMacro::Mark(':')),
    ("semicolon", SpokenMacro::Mark(';')),
    ("open quote", SpokenMacro::Open('"')),
    ("close quote", SpokenMacro::Close('"')),
    ("open paren", SpokenMacro::Open('(')),
    ("close paren", SpokenMacro::Close(')')),
];

/// Punctuation Whisper commonly glues onto a spoken keyword (e.g. `"period."`, `"comma,"`).
const TRAILING_STT_PUNCTUATION: [char; 6] = [',', '.', ';', ':', '!', '?'];

/// Returns the macro and number of whitespace-separated tokens matched at `tokens[0..]`.
///
/// Matching is case-insensitive and on whole tokens only, so `"commander"`, `"periodic"` and
/// `"newline-ish"` never match. Only the final token of a phrase may carry trailing STT
/// punctuation (which is discarded, since the macro replaces it).
fn match_spoken_macro(tokens: &[&str]) -> Option<(SpokenMacro, usize)> {
    let mut best: Option<(SpokenMacro, usize)> = None;
    for &(phrase, mac) in SPOKEN_MACROS {
        let words: Vec<&str> = phrase.split(' ').collect();
        let n = words.len();
        if n > tokens.len() || best.is_some_and(|(_, len)| len >= n) {
            continue;
        }
        let matches = words.iter().enumerate().all(|(k, word)| {
            let token = tokens[k];
            let core = if k + 1 == n {
                token.trim_end_matches(TRAILING_STT_PUNCTUATION)
            } else {
                token
            };
            core.eq_ignore_ascii_case(word)
        });
        if matches {
            best = Some((mac, n));
        }
    }
    best
}

/// Expands spoken punctuation commands ("period", "new line", ...) into real punctuation.
///
/// Runs on the raw transcript, before any casing transform. Whitespace is normalised: tokens are
/// re-joined with single spaces, no space precedes `. , ? ! : ;` or a closing quote/paren, and
/// line breaks swallow adjacent spaces.
///
/// ### Dangling and Attached Punctuation
/// Punctuation the STT engine already attached to a spoken keyword (e.g. `"comma,"` or `"period."`)
/// is discarded so the expanded macro replaces it.
///
/// Punctuation left dangling before an inserted mark is deduplicated:
/// - Dangling commas, semicolons, and colons (`[, ; :]`) are dropped before any inserted mark
///   (e.g. `"Yes, comma"` → `"Yes,"`, `"A; semicolon"` → `"A;"`, `"Yes, period"` → `"Yes."`).
/// - A single dangling terminal mark (`[. ! ?]`) is absorbed before an inserted terminal mark
///   (e.g. `"Done. period"` → `"Done."`, `"Really? question mark"` → `"Really?"`,
///   `"Stop! exclamation mark"` → `"Stop!"`).
/// - Multi-dot sequences (an intentional ellipsis like `...`) are preserved and never collapsed
///   into a single dot: a spoken period following an ellipsis is absorbed rather than appending
///   a redundant fourth dot (`"Hmm... period"` → `"Hmm..."`), while other terminal marks attach
///   directly (`"Hmm... question mark"` → `"Hmm...?"`).
pub fn apply_spoken_punctuation(text: &str) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut out = String::with_capacity(text.len());
    // True when the next word must attach directly (start, after a line break or open bracket).
    let mut glue_next = true;
    let mut i = 0;

    while i < tokens.len() {
        let Some((mac, consumed)) = match_spoken_macro(&tokens[i..]) else {
            if !glue_next {
                out.push(' ');
            }
            out.push_str(tokens[i]);
            glue_next = false;
            i += 1;
            continue;
        };
        i += consumed;

        match mac {
            SpokenMacro::Break(s) => {
                out.push_str(s);
                glue_next = true;
            }
            SpokenMacro::Mark(c) => {
                // Drop dangling STT punctuation like the comma in "Yes, comma".
                while out.ends_with([',', ';', ':']) {
                    out.pop();
                }

                if matches!(c, '.' | '!' | '?') {
                    if out.ends_with("..") {
                        // Multi-dot case: an intentional ellipsis (e.g. "Hmm...") is preserved
                        // and not collapsed into a single '.'. If inserting a '.', the dot is
                        // already present in the ellipsis, so absorb the spoken period without
                        // appending a redundant fourth dot.
                        if c == '.' {
                            glue_next = false;
                            continue;
                        }
                    } else {
                        let mut rev = out.chars().rev();
                        let last = rev.next();
                        let prev = rev.next();
                        if last.is_some_and(|ch| matches!(ch, '.' | '!' | '?'))
                            && !prev.is_some_and(|p| matches!(p, '.' | '!' | '?'))
                        {
                            out.pop();
                        }
                    }
                }

                out.push(c);
                glue_next = false;
            }
            SpokenMacro::Open(c) => {
                if !glue_next {
                    out.push(' ');
                }
                out.push(c);
                glue_next = true;
            }
            SpokenMacro::Close(c) => {
                out.push(c);
                glue_next = false;
            }
        }
    }

    out
}

/// Formats the transcribed text according to the desired `FormattingMode` and optional trailing space.
///
/// Spoken punctuation expansion is disabled; use [`format_transcription_with_options`] to opt in.
pub fn format_transcription(text: &str, mode: FormattingMode, trailing_space: bool) -> String {
    format_transcription_with_options(text, mode, trailing_space, false)
}

/// Formats the transcribed text, optionally expanding spoken punctuation first.
///
/// ### Ordering
/// When `parse_spoken_punctuation` is set, [`apply_spoken_punctuation`] runs on the raw
/// transcript *before* the casing step. Consequently `snake_case`, `camelCase` and `kebab-case`
/// still drop all punctuation (including expanded marks and line breaks) by design, and `raw`
/// still strips trailing punctuation and surrounding whitespace. Spoken punctuation is therefore
/// only visible in `standard` mode. When disabled, output is identical to
/// [`format_transcription`].
pub fn format_transcription_with_options(
    text: &str,
    mode: FormattingMode,
    trailing_space: bool,
    parse_spoken_punctuation: bool,
) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let expanded;
    let trimmed = if parse_spoken_punctuation {
        expanded = apply_spoken_punctuation(trimmed);
        expanded.as_str()
    } else {
        trimmed
    };

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

    fn sp(text: &str) -> String {
        apply_spoken_punctuation(text)
    }

    #[test]
    fn test_spoken_each_mapping_positive() {
        assert_eq!(sp("one new line two"), "one\ntwo");
        assert_eq!(sp("one next line two"), "one\ntwo");
        assert_eq!(sp("one new paragraph two"), "one\n\ntwo");
        assert_eq!(sp("hello period"), "hello.");
        assert_eq!(sp("hello full stop"), "hello.");
        assert_eq!(sp("hello comma world"), "hello, world");
        assert_eq!(sp("really question mark"), "really?");
        assert_eq!(sp("wow exclamation mark"), "wow!");
        assert_eq!(sp("wow exclamation point"), "wow!");
        assert_eq!(sp("note colon item"), "note: item");
        assert_eq!(sp("one semicolon two"), "one; two");
        assert_eq!(sp("he said open quote hi close quote"), "he said \"hi\"");
        assert_eq!(sp("call open paren x close paren now"), "call (x) now");
    }

    #[test]
    fn test_spoken_case_insensitive() {
        assert_eq!(sp("Hello COMMA World Period"), "Hello, World.");
        assert_eq!(sp("One NEW Line two"), "One\ntwo");
        assert_eq!(sp("Really Question Mark"), "Really?");
    }

    #[test]
    fn test_spoken_word_boundary_negative_controls() {
        for text in [
            "the commander arrived",
            "a periodic table",
            "this is newline-ish",
            "newline",
            "colonel mustard",
            "semicolons are fine",
            "periodically commas",
            "a new lines list",
            "the question marks",
            "new paragraphs",
            "an open quotes policy",
            "my full stops",
            "exclamation marker",
            "open parent close parents",
            "new linebacker",
            "brand new, line items",
        ] {
            assert_eq!(sp(text), text, "must not expand: {text}");
        }
    }

    #[test]
    fn test_spoken_whitespace_hygiene() {
        // No space before sentence-ending marks, none doubled, none stranded around breaks.
        assert_eq!(sp("hello   period   next   line   world"), "hello.\nworld");
        assert_eq!(sp("really question mark yes"), "really? yes");
        assert_eq!(sp("hi comma new paragraph bye period"), "hi,\n\nbye.");
        assert_eq!(sp("a open paren b close paren comma c"), "a (b), c");
        assert_eq!(sp("said open quote ok period close quote"), "said \"ok.\"");
        assert_eq!(sp("period"), ".");
        assert_eq!(sp("new line hello"), "\nhello");
        assert!(!sp("a  period  b  comma  c  colon").contains("  "));
        assert!(!sp("a  period  b").contains(" ."));
    }

    #[test]
    fn test_spoken_absorbs_stt_attached_punctuation() {
        // Whisper often decorates keywords: "Hello, comma, world period."
        assert_eq!(sp("Hello, comma, world period."), "Hello, world.");
        assert_eq!(sp("One new line. Two"), "One\nTwo");
        assert_eq!(sp("Yes, period."), "Yes.");
        // Punctuation in the middle of a phrase breaks the match.
        assert_eq!(sp("new, line"), "new, line");

        // Positive controls for attached mark followed by spoken mark (issue #18).
        assert_eq!(sp("Done. period"), "Done.");
        assert_eq!(sp("Really? question mark"), "Really?");
        assert_eq!(sp("Stop! exclamation mark"), "Stop!");
        assert_eq!(sp("Stop! exclamation point"), "Stop!");
        assert_eq!(sp("Done. full stop"), "Done.");

        // Attached marks with decorated keywords.
        assert_eq!(sp("Done. period."), "Done.");
        assert_eq!(sp("Really? question mark?"), "Really?");
        assert_eq!(sp("Stop! exclamation mark!"), "Stop!");

        // Preserves comma/semicolon/colon behaviour.
        assert_eq!(sp("Yes, comma"), "Yes,");
        assert_eq!(sp("A; semicolon"), "A;");
        assert_eq!(sp("Note: colon"), "Note:");

        // Cross-mark terminal replacements.
        assert_eq!(sp("Done. exclamation mark"), "Done!");
        assert_eq!(sp("Really. question mark"), "Really?");
        assert_eq!(sp("Done! period"), "Done.");
        assert_eq!(sp("Really? period"), "Really.");
    }

    #[test]
    fn test_spoken_ellipsis_preserved_not_collapsed() {
        // Multi-dot case: an intentional ellipsis is preserved and never collapsed
        // into a single dot (issue #18).
        assert_eq!(sp("Hmm... period"), "Hmm...");
        assert_eq!(sp("Hmm... full stop"), "Hmm...");
        assert_eq!(sp("Hmm... question mark"), "Hmm...?");
        assert_eq!(sp("Hmm... exclamation mark"), "Hmm...!");
        assert_eq!(sp("Wait.. period"), "Wait..");
        assert_ne!(sp("Hmm... period"), "Hmm.");
    }

    #[test]
    fn test_spoken_dangling_mark_without_keyword_untouched() {
        // Negative controls: trailing marks without a following spoken keyword stay untouched.
        assert_eq!(sp("Done."), "Done.");
        assert_eq!(sp("Really?"), "Really?");
        assert_eq!(sp("Stop!"), "Stop!");
        assert_eq!(sp("Yes,"), "Yes,");
        assert_eq!(sp("A;"), "A;");
        assert_eq!(sp("Note:"), "Note:");
        assert_eq!(sp("Hmm..."), "Hmm...");
        assert_eq!(sp("A. B? C!"), "A. B? C!");
        // Dangling terminal mark before a non-terminal spoken mark is not stripped.
        assert_eq!(sp("Done. comma"), "Done.,");
    }

    #[test]
    fn test_spoken_toggle_off_is_byte_identical() {
        let inputs = [
            "  Hello comma world period  ",
            "Add a new line then a new paragraph.",
            "commander periodic",
            "Done. period",
            "Really? question mark",
            "Stop! exclamation mark",
            "Hmm... period",
            "  ",
            "",
        ];
        let modes = [
            FormattingMode::Standard,
            FormattingMode::Raw,
            FormattingMode::SnakeCase,
            FormattingMode::CamelCase,
            FormattingMode::KebabCase,
        ];
        for input in inputs {
            for mode in modes {
                for trailing in [true, false] {
                    assert_eq!(
                        format_transcription_with_options(input, mode, trailing, false),
                        format_transcription(input, mode, trailing),
                    );
                }
            }
        }
        assert_eq!(
            format_transcription_with_options(
                "Hello comma world period",
                FormattingMode::Standard,
                true,
                false
            ),
            "Hello comma world period "
        );
    }

    #[test]
    fn test_spoken_toggle_on_standard_mode() {
        assert_eq!(
            format_transcription_with_options(
                "Hello comma world period",
                FormattingMode::Standard,
                true,
                true
            ),
            "Hello, world. "
        );
        // A trailing line break is preserved and no trailing space is appended after it.
        assert_eq!(
            format_transcription_with_options(
                "Hello new line",
                FormattingMode::Standard,
                true,
                true
            ),
            "Hello\n"
        );
    }

    #[test]
    fn test_spoken_casing_modes_still_drop_punctuation() {
        let input = "hello comma big world period";
        assert_eq!(
            format_transcription_with_options(input, FormattingMode::SnakeCase, true, true),
            "hello_big_world"
        );
        assert_eq!(
            format_transcription_with_options(input, FormattingMode::CamelCase, true, true),
            "helloBigWorld"
        );
        assert_eq!(
            format_transcription_with_options(input, FormattingMode::KebabCase, true, true),
            "hello-big-world"
        );
    }

    #[test]
    fn test_spoken_does_not_alter_whisper_prompt() {
        let vocab = vec!["period".to_string(), "comma".to_string()];
        assert_eq!(
            build_whisper_prompt(&vocab, None),
            Some("Technical vocabulary: period, comma.".to_string())
        );
    }
}
