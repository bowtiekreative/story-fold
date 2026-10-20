//! Rust media orchestration. FFmpeg/ffprobe and the isolated local Whisper ML
//! worker are external tools; no TypeScript or Node runtime is used here.
use crate::{
    AppError, Result, Store,
    storage::{read_json, write_json},
    topics, whisper,
};
use rustfft::{FftPlanner, num_complex::Complex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use storyfold_core::{
    Progress, Project, TimelineItem, Topic, VirtualCrop, Word, build_edl, build_fcpxml, build_srt,
    sequence, split_sentences,
};
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProcessingInput {
    pub words: Option<Vec<Word>>,
    #[serde(default)]
    pub manual: bool,
    #[serde(default)]
    pub sample: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportJob {
    pub id: String,
    pub project_id: String,
    pub status: String,
    pub pct: f64,
    pub detail: String,
    pub files: Vec<String>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Probe {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
}

async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
    notify: Option<mpsc::UnboundedSender<String>>,
) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok(result);
        }
        if result.len() + count > limit {
            return Err(AppError::bad("A media tool returned too much output."));
        }
        if let Some(tx) = &notify {
            let _ = tx.send(String::from_utf8_lossy(&buffer[..count]).into_owned());
        }
        result.extend_from_slice(&buffer[..count]);
    }
}

pub(crate) async fn run(
    bin: &Path,
    args: &[String],
    cancel: &CancellationToken,
    timeout: Duration,
    cwd: Option<&Path>,
    offline: bool,
    output: Option<mpsc::UnboundedSender<String>>,
) -> Result<Vec<u8>> {
    let name = bin
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Media tool");
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if offline {
        command.env("HF_HUB_OFFLINE", "1");
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().map_err(|_| {
        AppError::bad(format!(
            "{name} could not start. Check that the media tools are installed."
        ))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::bad("Could not read media tool output."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::bad("Could not read media tool status."))?;
    let operation = async {
        let (stdout, _, status) = tokio::try_join!(
            read_bounded(stdout, 64 * 1024 * 1024, output),
            read_bounded(stderr, 256 * 1024, None),
            async { child.wait().await.map_err(AppError::from) },
        )?;
        if !status.success() {
            // FFmpeg and provider stderr can contain paths, transcripts and server
            // environment information. Do not reflect it into HTTP, project JSON or logs.
            return Err(AppError::bad(format!(
                "{name} failed. Check the media format and available disk space, then retry."
            )));
        }
        Ok(stdout)
    };
    tokio::select! {
        _ = cancel.cancelled() => Err(AppError::bad("Processing cancelled.")),
        result = tokio::time::timeout(timeout, operation) => result.unwrap_or_else(|_| Err(AppError::bad(format!("{name} took too long. Try a shorter clip or resume processing.")))),
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}
fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) async fn ff(values: Vec<String>, cancel: &CancellationToken) -> Result<Vec<u8>> {
    let mut all = args(&["-hide_banner", "-loglevel", "error", "-y"]);
    all.extend(values);
    run(
        Path::new("ffmpeg"),
        &all,
        cancel,
        Duration::from_secs(7200),
        None,
        false,
        None,
    )
    .await
}

async fn probe_cancel(file: &Path, cancel: &CancellationToken) -> Result<Probe> {
    let mut values = args(&[
        "-v",
        "error",
        "-show_format",
        "-show_streams",
        "-of",
        "json",
    ]);
    values.push(path_arg(file));
    let bytes = run(
        Path::new("ffprobe"),
        &values,
        cancel,
        Duration::from_secs(30),
        None,
        false,
        None,
    )
    .await?;
    let info: Value = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::bad("Could not read the video metadata."))?;
    let duration = info
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= 7200.0);
    let streams = info
        .get("streams")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::bad("The video has no readable streams."))?;
    let video = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(Value::as_str) == Some("video"));
    let Some((duration, video)) = duration.zip(video) else {
        return Err(AppError::bad("Upload a video between 0 and 2 hours long."));
    };
    if !streams
        .iter()
        .any(|s| s.get("codec_type").and_then(Value::as_str) == Some("audio"))
    {
        return Err(AppError::bad("The video needs an audio track."));
    }
    Ok(Probe {
        duration,
        width: video.get("width").and_then(Value::as_u64).unwrap_or(1280) as u32,
        height: video.get("height").and_then(Value::as_u64).unwrap_or(720) as u32,
    })
}

pub async fn probe(file: &Path) -> Result<Probe> {
    probe_cancel(file, &CancellationToken::new()).await
}

/// All media references stay under the project directory. The HTTP layer also
/// validates saved projects; checking here protects direct callers and exports.
async fn local_path(dir: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if !whisper::is_plain_relative(relative) {
        return Err(AppError::bad("Invalid project media path."));
    }
    let mut current = dir.to_path_buf();
    for part in relative.components() {
        current.push(part);
        match tokio::fs::symlink_metadata(&current).await {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AppError::bad("Project media cannot be a symbolic link."));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(current)
}

pub fn audio_envelope(samples: &[u8], window: usize) -> Vec<f64> {
    if window == 0 {
        return Vec::new();
    }
    samples
        .chunks_exact(window * 4)
        .map(|chunk| {
            let sum: f64 = chunk
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| {
                    let n = f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64;
                    if n.is_finite() { n * n } else { 0.0 }
                })
                .sum();
            (sum / window as f64).sqrt()
        })
        .collect()
}

pub fn correlate(a: &[f64], b: &[f64], rate: usize) -> (f64, f64) {
    if a.is_empty() || b.is_empty() || rate == 0 {
        return (0.0, 0.0);
    }
    let size = (a.len() + b.len()).next_power_of_two();
    let mut left = vec![Complex::new(0.0, 0.0); size];
    let mut right = left.clone();
    for (i, value) in a.iter().enumerate() {
        left[a.len() - 1 - i].re = *value;
    }
    for (i, value) in b.iter().enumerate() {
        right[i].re = *value;
    }
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(size);
    fft.process(&mut left);
    fft.process(&mut right);
    for i in 0..size {
        left[i] *= right[i];
    }
    planner.plan_fft_inverse(size).process(&mut left);
    let prefix = |values: &[f64], squared: bool| {
        let mut out = Vec::with_capacity(values.len() + 1);
        out.push(0.0);
        for v in values {
            out.push(out.last().copied().unwrap_or(0.0) + if squared { v * v } else { *v });
        }
        out
    };
    let (pa, pb, pqa, pqb) = (
        prefix(a, false),
        prefix(b, false),
        prefix(a, true),
        prefix(b, true),
    );
    let range = (120 * rate).min(a.len().max(b.len()) - 1) as isize;
    let min_overlap = (5.0 * rate as f64).min(a.len().min(b.len()) as f64 / 2.0);
    let (mut lag, mut peak) = (0, 0.0);
    for l in -range..=range {
        let lo = 0.max(-l) as usize;
        let hi = (a.len() as isize).min(b.len() as isize - l);
        if hi <= lo as isize {
            continue;
        }
        let hi = hi as usize;
        let n = (hi - lo) as f64;
        if n < min_overlap {
            continue;
        }
        let blo = (lo as isize + l) as usize;
        let bhi = (hi as isize + l) as usize;
        let sa = pa[hi] - pa[lo];
        let sb = pb[bhi] - pb[blo];
        let aa = pqa[hi] - pqa[lo] - sa * sa / n;
        let bb = pqb[bhi] - pqb[blo] - sb * sb / n;
        if aa < 1e-10 || bb < 1e-10 {
            continue;
        }
        let score = (left[(a.len() as isize - 1 + l) as usize].re / size as f64 - sa * sb / n)
            / (aa * bb).sqrt();
        if score > peak {
            peak = score;
            lag = l;
        }
    }
    (lag as f64 / rate as f64, peak.clamp(0.0, 1.0))
}

async fn envelope(file: &Path, cancel: &CancellationToken) -> Result<Vec<f64>> {
    let mut values = args(&["-v", "error", "-i"]);
    values.push(path_arg(file));
    values.extend(args(&[
        "-t", "300", "-vn", "-ar", "1000", "-ac", "1", "-f", "f32le", "pipe:1",
    ]));
    Ok(audio_envelope(
        &run(
            Path::new("ffmpeg"),
            &values,
            cancel,
            Duration::from_secs(600),
            None,
            false,
            None,
        )
        .await?,
        10,
    ))
}

async fn progress(
    store: &Store,
    project: &mut Project,
    step: &str,
    detail: impl Into<String>,
    pct: f64,
) -> Result<()> {
    project.status = step.into();
    project.progress.push(Progress {
        step: step.into(),
        detail: detail.into(),
        pct,
        at: chrono::Utc::now().to_rfc3339(),
    });
    store.save(project).await
}

async fn transcribe(
    store: &Store,
    project: &mut Project,
    cancel: &CancellationToken,
) -> Result<Vec<Word>> {
    let status = whisper::speech_status(store).await;
    if !status.transcription {
        return Err(AppError::bad(
            "Whisper needs one-time setup on this computer. Install the local Whisper runtime, then resume processing.",
        ));
    }
    let dir = store.project_dir(&project.id)?;
    let duration = project.cameras[0].duration;
    let source = local_path(&dir, &project.cameras[0].source_path).await?;
    let mut words: Vec<Word> = Vec::new();
    for start in (0..duration.ceil() as u64).step_by(599) {
        progress(
            store,
            project,
            "transcribing",
            format!(
                "{}: transcribing from minute {} of {}",
                status.speech_provider,
                start / 60,
                (duration / 60.0).ceil() as u64
            ),
            30.0 + 20.0 * start as f64 / duration,
        )
        .await?;
        let wav = local_path(&dir, &format!("audio-{start}.wav")).await?;
        let checkpoint = local_path(&dir, &format!("transcript-{start}.json")).await?;
        let result: Value = if checkpoint.exists() {
            read_json(&checkpoint).await?
        } else {
            let mut values = args(&["-ss", &start.to_string(), "-i"]);
            values.push(path_arg(&source));
            values.extend(args(&["-t", "600", "-vn", "-ar", "16000", "-ac", "1"]));
            values.push(path_arg(&wav));
            ff(values, cancel).await?;
            let value = if status.speech_provider == "Local Whisper" {
                let worker = whisper::paths(store);
                let values = vec![
                    path_arg(&worker.worker),
                    "--model".into(),
                    path_arg(&worker.model),
                    "--audio".into(),
                    path_arg(&wav),
                ];
                let bytes = run(
                    &worker.python,
                    &values,
                    cancel,
                    Duration::from_secs(1800),
                    None,
                    true,
                    None,
                )
                .await?;
                serde_json::from_slice(&bytes)
                    .map_err(|_| AppError::bad("Local Whisper returned invalid JSON."))?
            } else {
                let key = std::env::var("OPENAI_API_KEY")
                    .map_err(|_| AppError::bad("OpenAI Whisper is not configured."))?;
                let audio = tokio::fs::read(&wav).await?;
                let form = reqwest::multipart::Form::new()
                    .part(
                        "file",
                        reqwest::multipart::Part::bytes(audio)
                            .file_name("interview.wav")
                            .mime_str("audio/wav")
                            .map_err(|_| AppError::bad("Could not prepare transcription audio."))?,
                    )
                    .text("model", "whisper-1")
                    .text("response_format", "verbose_json")
                    .text("timestamp_granularities[]", "word");
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(300))
                    .build()
                    .map_err(|_| AppError::bad("Could not initialize transcription."))?;
                let request = async {
                    let mut response = client
                        .post("https://api.openai.com/v1/audio/transcriptions")
                        .bearer_auth(key.trim())
                        .multipart(form)
                        .send()
                        .await
                        .map_err(|_| {
                            AppError::bad("The transcription provider could not be reached.")
                        })?;
                    if !response.status().is_success() {
                        return Err(AppError::bad(format!(
                            "Transcription provider returned {}. Check your server key, credits and audio format.",
                            response.status().as_u16()
                        )));
                    }
                    let mut bytes = Vec::new();
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .map_err(|_| AppError::bad("The transcription response was interrupted."))?
                    {
                        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                            return Err(AppError::bad(
                                "The transcription provider returned too much data.",
                            ));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    serde_json::from_slice(&bytes).map_err(|_| {
                        AppError::bad("The transcription provider returned invalid JSON.")
                    })
                };
                tokio::select! { _ = cancel.cancelled() => return Err(AppError::bad("Processing cancelled.")), value = request => value? }
            };
            whisper::speech_words(&value, start as f64, duration)?;
            write_json(&checkpoint, &value).await?;
            value
        };
        for word in whisper::speech_words(&result, start as f64, duration)? {
            if word.start >= words.last().map_or(0.0, |last| last.end) - 0.03
                && word.end > word.start
            {
                words.push(word);
            }
        }
    }
    if words.is_empty() {
        return Err(AppError::bad(
            "Whisper found no speech. Check that camera A has audible dialogue, then try again.",
        ));
    }
    progress(
        store,
        project,
        "transcribing",
        format!(
            "{}: {} words transcribed",
            status.speech_provider,
            words.len()
        ),
        50.0,
    )
    .await?;
    Ok(words)
}

fn manual_topics(project: &mut Project) {
    project.topics = vec![Topic {
        id: "interview".into(),
        name: "Interview".into(),
        accent: "violet".into(),
        ..Default::default()
    }];
    project.grouping = Some("Manual topics".into());
    for sentence in &mut project.sentences {
        sentence.topic_id = "interview".into();
    }
}

async fn group_topics(
    store: &Store,
    project: &mut Project,
    cancel: &CancellationToken,
) -> Result<()> {
    let settings = topics::topic_settings();
    if !settings.topics {
        manual_topics(project);
        project.warnings.push("Configure a Claude or OpenAI key for automatic topic grouping. Your transcript is ready to organize manually.".into());
        return Ok(());
    }
    let sentences = project.sentences.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let operation = topics::classify_topics(&sentences, &tx, cancel);
    tokio::pin!(operation);
    let result = loop {
        tokio::select! {
            result = &mut operation => break result,
            Some((detail, fraction)) = rx.recv() => progress(store, project, "grouping", detail, 82.0 + 14.0 * fraction).await?,
        }
    };
    match result {
        Ok(out) => {
            let accents = ["violet", "coral", "teal", "amber", "sky", "lime"];
            project.topics = out
                .topics
                .into_iter()
                .enumerate()
                .map(|(i, t)| Topic {
                    id: t.id,
                    name: t.name,
                    summary: Some(t.summary),
                    accent: accents[i % accents.len()].into(),
                    ..Default::default()
                })
                .collect();
            for sentence in &mut project.sentences {
                sentence.topic_id = out
                    .assignments
                    .get(&(sentence.n as u64))
                    .cloned()
                    .ok_or_else(|| AppError::bad("Topic classification omitted a sentence."))?;
            }
            project.grouping = Some(format!(
                "{} topic suggestions — review before editing",
                settings.topic_provider
            ));
        }
        Err(error) if cancel.is_cancelled() => return Err(error),
        Err(error) => {
            manual_topics(project);
            project.warnings.push(format!(
                "{error} Your transcript is intact; organize it manually."
            ));
        }
    }
    Ok(())
}

async fn process_inner(
    store: &Store,
    project: &mut Project,
    input: ProcessingInput,
    cancel: &CancellationToken,
) -> Result<()> {
    if project.cameras.is_empty()
        || project.cameras.len() > 3
        || project.cameras[0].id != "A"
        || project.cameras[0].virtual_crop.is_some()
    {
        return Err(AppError::bad(
            "Choose one to three videos, with camera A as the audio master.",
        ));
    }
    if cancel.is_cancelled() {
        return Err(AppError::bad("Processing cancelled."));
    }
    project.cameras[0].offset = 0.0;
    let dir = store.project_dir(&project.id)?;
    progress(
        store,
        project,
        "uploading",
        "Checking your footage and making playback copies",
        5.0,
    )
    .await?;
    for camera in &mut project.cameras {
        if camera.virtual_crop.is_some() {
            camera.offset = 0.0;
            continue;
        }
        let source = local_path(&dir, &camera.source_path).await?;
        let meta = probe_cancel(&source, cancel).await?;
        camera.duration = meta.duration;
        let proxy = local_path(&dir, &camera.proxy_path).await?;
        if source == proxy {
            return Err(AppError::bad(
                "Playback copies must be separate from original footage.",
            ));
        }
        let valid_proxy = if proxy.exists() {
            probe_cancel(&proxy, cancel)
                .await
                .is_ok_and(|p| (p.duration - meta.duration).abs() < 0.2)
        } else {
            false
        };
        if cancel.is_cancelled() {
            return Err(AppError::bad("Processing cancelled."));
        }
        if !valid_proxy {
            // A cancelled transcode must never look like a completed cache entry.
            // Persist replaces the old proxy only after FFmpeg closes successfully.
            let temporary = tempfile::NamedTempFile::new_in(&dir)?.into_temp_path();
            let mut values = args(&["-i"]);
            values.push(path_arg(&source));
            values.extend(args(&[
                "-vf",
                "scale='min(1280,iw)':-2",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "25",
                "-g",
                "13",
                "-keyint_min",
                "13",
                "-sc_threshold",
                "0",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ]));
            values.push(path_arg(&temporary));
            ff(values, cancel).await?;
            temporary
                .persist(&proxy)
                .map_err(|e| AppError::from(e.error))?;
        }
    }
    if project.cameras.len() == 1 {
        let mut close = project.cameras[0].clone();
        close.id = "B".into();
        close.name = "Close-up".into();
        close.virtual_crop = Some(VirtualCrop {
            scale: 1.65,
            x: 0.5,
            y: 0.45,
        });
        let mut side = project.cameras[0].clone();
        side.id = "C".into();
        side.name = "Side angle".into();
        side.virtual_crop = Some(VirtualCrop {
            scale: 1.3,
            x: 0.3,
            y: 0.5,
        });
        project.cameras.extend([close, side]);
    }
    let peaks_file = local_path(&dir, "peaks.json").await?;
    if !peaks_file.exists() {
        let source = local_path(&dir, &project.cameras[0].source_path).await?;
        let mut values = args(&["-v", "error", "-i"]);
        values.push(path_arg(&source));
        values.extend(args(&[
            "-vn", "-ac", "1", "-ar", "1000", "-f", "f32le", "pipe:1",
        ]));
        let peaks = audio_envelope(
            &run(
                Path::new("ffmpeg"),
                &values,
                cancel,
                Duration::from_secs(1800),
                None,
                false,
                None,
            )
            .await?,
            50,
        );
        let max = peaks
            .iter()
            .take(10_000)
            .chain(peaks.iter().rev().take(10_000))
            .copied()
            .fold(0.0001_f64, f64::max);
        write_json(&peaks_file, &json!({"rate":20,"peaks":peaks.into_iter().map(|p| (p/max).min(1.0)).collect::<Vec<_>>()})).await?;
    }
    progress(
        store,
        project,
        "transcribing",
        if input.sample {
            "Loading the labeled sample transcript"
        } else if input.words.is_some() {
            "Reading your timed transcript"
        } else {
            "Transcribing speech with word timestamps"
        },
        30.0,
    )
    .await?;
    let word_file = local_path(&dir, "words.json").await?;
    let words = if word_file.exists() {
        read_json::<Vec<Word>>(&word_file).await?
    } else if let Some(words) = input.words {
        words
    } else {
        transcribe(store, project, cancel).await?
    };
    if words.is_empty()
        || words.len() > 200_000
        || words.iter().any(|word| word.text.chars().count() > 200)
    {
        return Err(AppError::bad(
            "The transcript must contain 1 to 200,000 timed words.",
        ));
    }
    // Validate imported and cached transcripts before saving or segmenting them.
    project.sentences =
        split_sentences(&words, project.cameras[0].duration).map_err(AppError::bad)?;
    write_json(&word_file, &words).await?;
    progress(
        store,
        project,
        "segmenting",
        "Finding sentence boundaries",
        55.0,
    )
    .await?;
    progress(
        store,
        project,
        "syncing",
        if project.cameras.iter().any(|c| c.virtual_crop.is_some()) {
            "Creating two reframed angles from camera A"
        } else {
            "Matching the camera audio"
        },
        68.0,
    )
    .await?;
    if project
        .cameras
        .iter()
        .filter(|c| c.virtual_crop.is_none())
        .count()
        > 1
    {
        let master = envelope(
            &local_path(&dir, &project.cameras[0].source_path).await?,
            cancel,
        )
        .await?;
        for camera in project
            .cameras
            .iter_mut()
            .skip(1)
            .filter(|c| c.virtual_crop.is_none())
        {
            let (offset, confidence) = correlate(
                &master,
                &envelope(&local_path(&dir, &camera.source_path).await?, cancel).await?,
                100,
            );
            camera.offset = offset;
            if confidence < 0.55 {
                project.warnings.push(format!("Camera {}: audio sync is uncertain. Check and adjust the offset before editing.", camera.id));
            }
        }
    }
    progress(
        store,
        project,
        "grouping",
        if input.manual {
            "Keeping your transcript ready for topic folders"
        } else {
            "Suggesting topic folders"
        },
        82.0,
    )
    .await?;
    if input.sample {
        project.topics = sample_topics();
        for (i, sentence) in project.sentences.iter_mut().enumerate() {
            sentence.topic_id = SAMPLE_LINES.get(i).map_or("interview", |l| l.1).into();
        }
        project.grouping = Some("Sample folders — supplied with the demo".into());
    } else if input.manual {
        manual_topics(project);
    } else {
        group_topics(store, project, cancel).await?;
    }
    let mut seen = HashSet::new();
    let mut k = 0;
    for sentence in &mut project.sentences {
        sentence.cam = if seen.insert(sentence.topic_id.clone()) {
            "A".into()
        } else {
            k += 1;
            project.cameras[k % project.cameras.len()].id.clone()
        };
    }
    project.timeline.clear();
    project.error = None;
    project.revision += 1;
    progress(
        store,
        project,
        "ready",
        format!(
            "{} sentences in {} topic folders",
            project.sentences.len(),
            project.topics.len()
        ),
        100.0,
    )
    .await
}

pub async fn process(
    store: &Store,
    mut project: Project,
    input: ProcessingInput,
    cancel: CancellationToken,
) -> Result<()> {
    match process_inner(store, &mut project, input, &cancel).await {
        Ok(()) => Ok(()),
        Err(error) => {
            project.error = Some(error.to_string());
            progress(store, &mut project, "error", error.to_string(), 0.0).await?;
            Err(error)
        }
    }
}

const SAMPLE_LINES: [(&str, &str); 8] = [
    ("I grew up above a bakery on Delancey Street.", "growing"),
    (
        "Every morning the whole building smelled like rye bread.",
        "bakery",
    ),
    ("My mom was up at four, every single day.", "mom"),
    ("She never once complained about it.", "mom"),
    ("We did not have much, but we never felt poor.", "growing"),
    (
        "The whole block showed up with buckets and brooms.",
        "bakery",
    ),
    ("That is when I understood what community means.", "advice"),
    ("If you love someone, show up early.", "advice"),
];

fn sample_topics() -> Vec<Topic> {
    [
        ("growing", "Growing up", "amber"),
        ("mom", "Mom", "coral"),
        ("bakery", "The bakery", "teal"),
        ("advice", "Advice", "violet"),
    ]
    .into_iter()
    .map(|(id, name, accent)| Topic {
        id: id.into(),
        name: name.into(),
        accent: accent.into(),
        ..Default::default()
    })
    .collect()
}

pub async fn create_sample(store: &Store, project: &Project) -> Result<Vec<Word>> {
    let dir = store.project_dir(&project.id)?;
    tokio::fs::create_dir_all(&dir).await?;
    let output = local_path(&dir, "source-A.mp4").await?;
    let mut values = args(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=960x540:rate=25",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=220:sample_rate=48000",
        "-t",
        "36",
        "-vf",
        "drawbox=x=0:y=ih-90:w=iw:h=90:color=black@0.8:t=fill,drawtext=text='STORYFOLD  -  SYNTHETIC SAMPLE':fontcolor=white:fontsize=28:x=(w-text_w)/2:y=h-65",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-c:a",
        "aac",
        "-af",
        "volume=0.03",
        "-shortest",
    ]);
    values.push(path_arg(&output));
    // Synthetic footage verifies the app flow. It is not speech or sync evidence.
    ff(values, &CancellationToken::new()).await?;
    Ok(SAMPLE_LINES
        .iter()
        .enumerate()
        .flat_map(|(i, (line, _))| {
            let tokens = line.split_whitespace().collect::<Vec<_>>();
            let step = 3.6 / tokens.len() as f64;
            tokens.into_iter().enumerate().map(move |(j, text)| Word {
                text: text.into(),
                start: i as f64 * 4.5 + 0.3 + j as f64 * step,
                end: i as f64 * 4.5 + 0.3 + (j as f64 + 0.9) * step,
            })
        })
        .collect())
}

pub(crate) fn ass_time(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        cs / 6000 % 60,
        cs / 100 % 60,
        cs % 100
    )
}

fn caption_text(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| !matches!(c, '{' | '}' | '\\' | '\r' | '\n'))
        .collect();
    let tokens: Vec<_> = clean.split_whitespace().collect();
    if tokens.len() > 7 {
        let half = tokens.len().div_ceil(2);
        format!(
            "{}\\N{}",
            tokens[..half].join(" "),
            tokens[half..].join(" ")
        )
    } else {
        tokens.join(" ")
    }
}

struct RenderPlan {
    filter: String,
    subtitles: String,
    metadata: String,
    chapters: String,
    video_label: &'static str,
}

fn render_plan(project: &Project) -> Result<RenderPlan> {
    project.export_settings.validate().map_err(AppError::bad)?;
    let sq = sequence(project);
    if sq.clips.is_empty() {
        return Err(AppError::bad(
            "Add a topic or sentence to the story before exporting.",
        ));
    }
    let (w, h) = match project.export_settings.ratio.as_str() {
        "9:16" => (720, 1280),
        "1:1" => (720, 720),
        _ => (1280, 720),
    };
    let mut filters = Vec::new();
    let mut videos = String::new();
    let mut audios = String::new();
    for (i, c) in sq.clips.iter().enumerate() {
        let (ci, cam) = project
            .cameras
            .iter()
            .enumerate()
            .find(|(_, cam)| cam.id == c.sentence.cam)
            .ok_or_else(|| AppError::bad("A cut uses an unknown camera."))?;
        let (scale, x, y) = cam
            .virtual_crop
            .as_ref()
            .map_or((1.0, 0.5, 0.5), |crop| (crop.scale, crop.x, crop.y));
        let from = c.sentence.r#in + cam.offset;
        let frames = ((c.v_end * 25.0).round() - (c.v_start * 25.0).round()).max(1.0) as u64;
        let length = frames as f64 / 25.0;
        if ![from, c.dur, c.sentence.r#in, scale, x, y]
            .iter()
            .all(|v| v.is_finite())
            || from < 0.0
            || from >= cam.duration
            || !(1.0..=5.0).contains(&scale)
            || !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
        {
            return Err(AppError::bad(format!(
                "Camera {} has no valid footage at this cut. Adjust its sync offset or choose another camera.",
                cam.id
            )));
        }
        filters.push(format!("[{ci}:v]trim=start={from}:duration={length},setpts=PTS-STARTPTS,crop=iw/{scale}:ih/{scale}:(iw-ow)*{x}:(ih-oh)*{y},scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1,fps=25,tpad=stop_mode=clone:stop_duration={length},trim=end_frame={frames},setpts=PTS-STARTPTS[v{i}]"));
        filters.push(format!("[0:a]atrim=start={}:duration={},asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,apad,atrim=duration={},afade=t=in:d=0.005,afade=t=out:st={}:d=0.005[a{i}]",c.sentence.r#in,c.dur,c.dur,(c.dur-0.005).max(0.0)));
        videos.push_str(&format!("[v{i}]"));
        audios.push_str(&format!("[a{i}]"));
    }
    filters.push(format!(
        "{videos}concat=n={}:v=1:a=0[vjoined]",
        sq.clips.len()
    ));
    filters.push(format!("{audios}concat=n={}:v=0:a=1[aout]", sq.clips.len()));
    let mut subtitles=vec!["[Script Info]".into(),"ScriptType: v4.00+".into(),format!("PlayResX: {w}"),format!("PlayResY: {h}"),"[V4+ Styles]".into(),"Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding".into(),format!("Style: Default,DejaVu Sans,{},&H00FFFFFF,&H00FFFFFF,&H00222222,&H99000000,0,0,0,0,100,100,0,0,3,2,0,2,40,40,{},1",(w as f64*0.031).round(),(h as f64*0.07).round()),"[Events]".into(),"Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text".into()];
    for c in &sq.clips {
        subtitles.push(format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{}",
            ass_time(c.a_start),
            ass_time(c.a_end),
            caption_text(&c.sentence.text)
        ));
    }
    let video_label = if project.export_settings.captions {
        filters.push("[vjoined]ass=filename='captions.ass'[vout]".into());
        "vout"
    } else {
        "vjoined"
    };
    let mut metadata = vec![";FFMETADATA1".into()];
    let mut chapters = Vec::new();
    if project.export_settings.chapters {
        for (i, span) in sq.spans.iter().enumerate() {
            if span.count == 0 {
                continue;
            }
            let title = match &project.timeline[i] {
                TimelineItem::Group { topic_id } => project
                    .topics
                    .iter()
                    .find(|t| t.id == *topic_id)
                    .map(|t| t.name.as_str()),
                TimelineItem::Clip { sentence_id } => project
                    .sentences
                    .iter()
                    .find(|s| s.id == *sentence_id)
                    .map(|s| s.text.as_str()),
            }
            .ok_or_else(|| AppError::bad("The story contains an unknown chapter."))?;
            let safe_title: String = title
                .chars()
                .map(|c| {
                    if matches!(c, '=' | ';' | '#' | '\\' | '\n' | '\r') {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
            metadata.extend([
                "[CHAPTER]".into(),
                "TIMEBASE=1/1000".into(),
                format!("START={}", (span.start * 1000.0).round()),
                format!("END={}", (span.end * 1000.0).round()),
                format!("title={safe_title}"),
            ]);
            chapters.push(format!(
                "{}:{:02} {}",
                (span.start / 60.0).floor() as u64,
                span.start.floor() as u64 % 60,
                title.replace(['\r', '\n'], " ")
            ));
        }
    }
    Ok(RenderPlan {
        filter: filters.join(";\n"),
        subtitles: subtitles.join("\n"),
        metadata: metadata.join("\n"),
        chapters: chapters.join("\n"),
        video_label,
    })
}

async fn render_inner(
    store: &Store,
    project: &Project,
    job: &mut ExportJob,
    out: &Path,
    cancel: &CancellationToken,
) -> Result<()> {
    if project.cameras.is_empty()
        || project.cameras[0].id != "A"
        || project.cameras[0].offset != 0.0
    {
        return Err(AppError::bad("Camera A must provide the story audio."));
    }
    let plan = render_plan(project)?;
    let sq = sequence(project);
    let dir = store.project_dir(&project.id)?;
    job.status = "rendering".into();
    job.detail = "Preparing your cuts".into();
    write_json(&out.join("job.json"), job).await?;
    write_json(&out.join("project.json"), project).await?;
    tokio::fs::write(out.join("story.srt"), build_srt(project)).await?;
    tokio::fs::write(out.join("story.edl"), build_edl(project)).await?;
    tokio::fs::write(out.join("story.fcpxml"), build_fcpxml(project)).await?;
    tokio::fs::write(out.join("captions.ass"), plan.subtitles).await?;
    tokio::fs::write(out.join("chapters.ffmeta"), plan.metadata).await?;
    tokio::fs::write(out.join("chapters.txt"), plan.chapters).await?;
    tokio::fs::write(out.join("filters.txt"), plan.filter).await?;
    let mut values = args(&["-hide_banner", "-loglevel", "error", "-y"]);
    for camera in &project.cameras {
        values.push("-i".into());
        values.push(path_arg(&local_path(&dir, &camera.source_path).await?));
    }
    values.extend(args(&[
        "-f",
        "ffmetadata",
        "-i",
        "chapters.ffmeta",
        "-filter_complex_threads",
        "1",
        "-filter_complex_script",
        "filters.txt",
        "-map",
        &format!("[{}]", plan.video_label),
        "-map",
        "[aout]",
        "-map_metadata",
        &project.cameras.len().to_string(),
        "-map_chapters",
        &project.cameras.len().to_string(),
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "22",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-t",
        &sq.total.to_string(),
        "-movflags",
        "+faststart",
        "-progress",
        "pipe:1",
        "story.mp4",
    ]));
    job.detail = "Rendering video, angles and L-cuts".into();
    write_json(&out.join("job.json"), job).await?;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let operation = run(
        Path::new("ffmpeg"),
        &values,
        cancel,
        Duration::from_secs(7200),
        Some(out),
        false,
        Some(tx),
    );
    tokio::pin!(operation);
    let mut pending = String::new();
    let mut last = tokio::time::Instant::now() - Duration::from_secs(1);
    loop {
        tokio::select! {
            result=&mut operation=>{result?;break;},
            Some(output)=rx.recv()=>{
                pending.push_str(&output);
                while let Some(end)=pending.find('\n') {
                    let line=pending[..end].trim().to_owned();pending.drain(..=end);
                    if let Some(us)=line.strip_prefix("out_time_us=").and_then(|s|s.parse::<f64>().ok()) {job.pct=(us/1_000_000.0/sq.total*100.0).round().clamp(0.0,99.0);}
                }
                if last.elapsed()>Duration::from_millis(700){last=tokio::time::Instant::now();write_json(&out.join("job.json"),job).await?;}
            }
        }
    }
    let mut verify = args(&[
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
    ]);
    verify.push(path_arg(&out.join("story.mp4")));
    let duration = run(
        Path::new("ffprobe"),
        &verify,
        cancel,
        Duration::from_secs(30),
        None,
        false,
        None,
    )
    .await?;
    let duration = String::from_utf8_lossy(&duration)
        .trim()
        .parse::<f64>()
        .map_err(|_| AppError::bad("Could not verify the rendered video duration."))?;
    if !duration.is_finite() || (duration - sq.total).abs() > 0.12 {
        return Err(AppError::bad(
            "The rendered duration did not match the story. Retry the export.",
        ));
    }
    job.status = "done".into();
    job.pct = 100.0;
    job.detail = "Your story is ready".into();
    job.error = None;
    job.files = [
        "story.mp4",
        "story.edl",
        "story.fcpxml",
        "story.srt",
        "chapters.txt",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    write_json(&out.join("job.json"), job).await
}

pub async fn render(
    store: &Store,
    project: Project,
    mut job: ExportJob,
    cancel: CancellationToken,
) -> Result<()> {
    if job.project_id != project.id
        || job.revision != project.revision
        || !storyfold_core::identifier(&job.id, 80)
    {
        return Err(AppError::bad("Invalid export snapshot."));
    }
    let dir = store.project_dir(&project.id)?;
    let out = local_path(&dir, &format!("exports/{}", job.id)).await?;
    tokio::fs::create_dir_all(&out).await?;
    match render_inner(store, &project, &mut job, &out, &cancel).await {
        Ok(()) => Ok(()),
        Err(error) => {
            job.status = "error".into();
            job.error = Some(error.to_string());
            job.detail = error.to_string();
            write_json(&out.join("job.json"), &job).await?;
            Err(error)
        }
    }
}

pub async fn tools_status(store: &Store) -> Value {
    let cancel = CancellationToken::new();
    let values = args(&["-version"]);
    let (ffmpeg, ffprobe, speech) = tokio::join!(
        run(
            Path::new("ffmpeg"),
            &values,
            &cancel,
            Duration::from_secs(10),
            None,
            false,
            None
        ),
        run(
            Path::new("ffprobe"),
            &values,
            &cancel,
            Duration::from_secs(10),
            None,
            false,
            None
        ),
        whisper::speech_status(store)
    );
    let topics = topics::topic_settings();
    json!({"ffmpeg":ffmpeg.is_ok(),"ffprobe":ffprobe.is_ok(),"transcription":speech.transcription,"speechProvider":speech.speech_provider,"topicProvider":topics.topic_provider,"topicModel":topics.topic_model,"topics":topics.topics})
}

#[cfg(test)]
mod tests {
    use super::*;
    use storyfold_core::{Camera, ExportSettings, Sentence};

    #[test]
    fn fft_sync_finds_delayed_camera_and_rejects_flat_signal() {
        let a: Vec<_> = (0..1600)
            .map(|i| ((i as f64 * 0.071).sin() * (i as f64 * 0.013).cos()).abs())
            .collect();
        let mut b = vec![0.0; 83];
        b.extend_from_slice(&a);
        let (offset, confidence) = correlate(&a, &b, 100);
        assert!((offset - 0.83).abs() < 0.001);
        assert!(confidence > 0.99);
        assert_eq!(correlate(&[1.0; 100], &[1.0; 100], 100), (0.0, 0.0));
        let samples: Vec<_> = [-0.5_f32, 0.5, -0.5, 0.5]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        assert_eq!(audio_envelope(&samples, 2), vec![0.5, 0.5]);
    }

    #[test]
    fn render_plan_keeps_lcut_video_separate_from_master_audio() {
        let sentences = vec![
            Sentence {
                id: "s1".into(),
                n: 1,
                text: "A caption {injection} \\N".into(),
                r#in: 0.0,
                out: 3.0,
                lcut: 1.0,
                cam: "A".into(),
                topic_id: "interview".into(),
                ..Default::default()
            },
            Sentence {
                id: "s2".into(),
                n: 2,
                text: "Second.".into(),
                r#in: 3.0,
                out: 6.0,
                cam: "B".into(),
                topic_id: "interview".into(),
                ..Default::default()
            },
        ];
        let project = Project {
            cameras: vec![
                Camera {
                    id: "A".into(),
                    duration: 10.0,
                    ..Default::default()
                },
                Camera {
                    id: "B".into(),
                    duration: 10.0,
                    offset: 0.2,
                    ..Default::default()
                },
            ],
            sentences,
            topics: vec![Topic {
                id: "interview".into(),
                name: "Interview;#=\n".into(),
                ..Default::default()
            }],
            timeline: vec![TimelineItem::Group {
                topic_id: "interview".into(),
            }],
            export_settings: ExportSettings {
                ratio: "9:16".into(),
                captions: true,
                chapters: true,
            },
            ..Default::default()
        };
        let plan = render_plan(&project).unwrap();
        assert!(plan.filter.contains("[0:v]trim=start=0:duration=2"));
        assert!(plan.filter.contains("[1:v]trim=start=3.2:duration=4"));
        assert!(plan.filter.contains("[0:a]atrim=start=0:duration=3"));
        assert!(plan.filter.contains("[0:a]atrim=start=3:duration=3"));
        assert!(plan.filter.contains("scale=720:1280"));
        assert!(!plan.subtitles.contains("{injection}"));
        assert!(plan.metadata.contains("title=Interview    "));
    }

    #[tokio::test]
    async fn media_paths_reject_parent_traversal_and_linked_directories() {
        let root = tempfile::tempdir().unwrap();
        assert!(local_path(root.path(), "../escaped.mp4").await.is_err());
        assert!(local_path(root.path(), "normal.mp4").await.is_ok());
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
            assert!(local_path(root.path(), "linked/source.mp4").await.is_err());
        }
    }

    #[tokio::test]
    async fn commands_cancel_without_reflecting_stderr() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = run(
            Path::new("ffmpeg"),
            &args(&["-version"]),
            &cancel,
            Duration::from_secs(2),
            None,
            false,
            None,
        )
        .await;
        assert!(result.is_err());
        let result = run(
            Path::new("ffmpeg"),
            &args(&["-i", "private-secret-path-do-not-display"]),
            &CancellationToken::new(),
            Duration::from_secs(2),
            None,
            false,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(!result.contains("private-secret-path"));
    }
}
