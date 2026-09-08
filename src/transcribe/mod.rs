pub mod client;
pub mod formatting;

pub use client::TranscriptionClient;
#[allow(unused_imports)]
pub use formatting::{FormattingMode, build_whisper_prompt, format_transcription};
