//! Compatibility adapter for the local faster-whisper worker. Rust owns orchestration,
//! validation and checkpoints; the existing Python process remains the ML runtime.
use crate::{AppError, Result, Store};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use storyfold_core::Word;

pub struct WhisperPaths {
    pub python: PathBuf,
    pub model: PathBuf,
    pub ready: PathBuf,
    pub worker: PathBuf,
}

pub fn paths(store: &Store) -> WhisperPaths {
    let runtime = store.data.join("runtime/whisper");
    WhisperPaths {
        python: runtime.join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        }),
        model: store.data.join("models/whisper-small"),
        ready: runtime.join("ready.json"),
        worker: store.root.join("scripts/whisper.py"),
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechStatus {
    pub transcription: bool,
    pub speech_provider: String,
}

pub async fn speech_status(store: &Store) -> SpeechStatus {
    let p = paths(store);
    let needed = [
        p.python,
        p.ready,
        p.worker,
        p.model.join("model.bin"),
        p.model.join("config.json"),
        p.model.join("tokenizer.json"),
    ];
    let mut local = true;
    for file in needed {
        local &= tokio::fs::metadata(file).await.is_ok_and(|m| m.is_file());
    }
    let remote = std::env::var("OPENAI_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    SpeechStatus {
        transcription: local || remote,
        speech_provider: if local {
            "Local Whisper"
        } else if remote {
            "OpenAI Whisper"
        } else {
            "Setup needed"
        }
        .into(),
    }
}

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Validate a provider chunk before it can enter an on-disk checkpoint. Provider
/// response text is data and is never copied into errors or logs.
pub fn speech_words(input: &Value, offset: f64, duration: f64) -> Result<Vec<Word>> {
    if !offset.is_finite() || offset < 0.0 || !duration.is_finite() || duration <= 0.0 {
        return Err(AppError::bad("Invalid transcription time range."));
    }
    let rows = input
        .get("words")
        .and_then(Value::as_array)
        .filter(|a| a.len() <= 20_000)
        .ok_or_else(|| AppError::bad("Whisper returned an invalid word list."))?;
    let text = match input.get("text") {
        Some(Value::String(text)) => text.as_str(),
        Some(_) => return Err(AppError::bad("Whisper returned invalid transcript text.")),
        None => "",
    };
    let tokens: Vec<_> = text.split_whitespace().collect();
    let mut index = 0;
    let mut result = Vec::with_capacity(rows.len());
    let mut previous_start = 0.0;
    for row in rows {
        let original = row
            .get("word")
            .and_then(Value::as_str)
            .filter(|s| s.chars().count() <= 200)
            .ok_or_else(|| AppError::bad("Whisper returned an invalid word."))?;
        let start = row
            .get("start")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .ok_or_else(|| AppError::bad("Whisper returned invalid word timing."))?;
        let end = row
            .get("end")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= start)
            .ok_or_else(|| AppError::bad("Whisper returned invalid word timing."))?;
        if start < previous_start {
            return Err(AppError::bad("Whisper returned unordered word timing."));
        }
        previous_start = start;
        let mut word = original.trim();
        if let Some(token) = tokens
            .get(index)
            .filter(|token| normalize(token) == normalize(word))
        {
            word = token;
            index += 1;
        }
        let start = start + offset;
        let end = (end + offset).min(duration);
        if !word.is_empty() && end > start {
            if word.chars().count() > 200 || end - start > 20.0 {
                return Err(AppError::bad("Whisper returned an invalid word duration."));
            }
            result.push(Word {
                text: word.into(),
                start,
                end,
            });
        }
    }
    Ok(result)
}

pub fn is_plain_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chunk_restores_unicode_punctuation_offsets_and_clamps() {
        let words = speech_words(
            &json!({"text":"Héllo, world!", "words":[
                {"word":"héllo","start":0.2,"end":0.6},
                {"word":"world","start":0.7,"end":2.0}
            ]}),
            599.0,
            600.0,
        )
        .unwrap();
        assert_eq!(words[0].text, "Héllo,");
        assert_eq!(words[1].text, "world!");
        assert_eq!(words[0].start, 599.2);
        assert_eq!(words[1].end, 600.0);
    }

    #[test]
    fn invalid_chunks_and_paths_are_rejected() {
        for value in [
            json!({}),
            json!({"words":[{"word":"bad","start":2,"end":1}]}),
            json!({"words":[{"word":"bad","start":-1,"end":1}]}),
            json!({"words":[],"text":10}),
        ] {
            assert!(speech_words(&value, 0.0, 10.0).is_err());
        }
        assert!(!is_plain_relative(Path::new("../source.mp4")));
        assert!(!is_plain_relative(Path::new("/source.mp4")));
        assert!(is_plain_relative(Path::new("source-A.mp4")));
    }
}
