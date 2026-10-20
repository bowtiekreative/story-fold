//! Opt-in native capture, recoverable source takes, and a local recording sequence.
mod capture;
mod render;

use crate::{
    AppError, Result, Store,
    api::{self, AppState},
    media::{self, ProcessingInput},
    storage::{read_json, reject_link, write_json},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use storyfold_core::{Camera, recording::*};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tower_http::services::ServeFile;

#[derive(Clone, Default)]
pub struct Recorder {
    lock: Arc<Mutex<()>>,
    live: Arc<Mutex<Option<Live>>>,
    renders: Arc<Mutex<HashSet<String>>>,
}
#[derive(Clone)]
struct Live {
    session: String,
    take: String,
    microphone: bool,
    cancel: CancellationToken,
    frame: watch::Receiver<Vec<u8>>,
    done: watch::Receiver<bool>,
    micros: Arc<AtomicU64>,
    marks: Arc<Mutex<Vec<OverlayMark>>>,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/recording-devices", get(devices))
        .route("/api/recordings", get(list).post(create))
        .route("/api/recordings/{id}", get(load).patch(edit))
        .route("/api/recordings/{id}/start", post(start))
        .route("/api/recordings/{id}/stop", post(stop))
        .route("/api/recordings/{id}/overlay", post(overlay))
        .route("/api/recordings/{id}/export", post(export))
        .route("/api/recordings/{id}/editor", post(editor))
        .route("/api/recordings/{id}/media/preview.jpg", get(preview))
        .route("/api/recordings/{id}/media/takes/{take}", get(take_file))
        .route(
            "/api/recordings/{id}/media/exports/{export}",
            get(export_file),
        )
}
fn identifier(id: &str) -> Result<()> {
    if !storyfold_core::identifier(id, 60) {
        return Err(AppError::bad("Invalid recording identifier."));
    }
    Ok(())
}
fn directory(store: &Store, id: &str) -> Result<PathBuf> {
    identifier(id)?;
    let base = store.data.join("recordings");
    store.directory(&base)?;
    let path = base.join(id);
    reject_link(&path)?;
    Ok(path)
}
fn asset(store: &Store, id: &str, file: &str) -> Result<PathBuf> {
    if file.is_empty()
        || file.len() > 120
        || !file
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        || file.starts_with('.')
    {
        return Err(AppError::bad("Invalid recording asset."));
    }
    let path = directory(store, id)?.join(file);
    reject_link(&path)?;
    Ok(path)
}
async fn save(store: &Store, session: &RecordingSession) -> Result<()> {
    store.directory(&directory(store, &session.id)?)?;
    write_json(&asset(store, &session.id, "session.json")?, session).await
}
async fn read(s: &AppState, id: &str) -> Result<RecordingSession> {
    let mut session: RecordingSession = read_json(&asset(&s.store, id, "session.json")?).await?;
    if session.id != id {
        return Err(AppError::bad("Recording identity is invalid."));
    }
    if let Some(live) = s
        .recordings
        .live
        .lock()
        .await
        .as_ref()
        .filter(|l| l.session == id)
    {
        session.active = Some(ActiveRecording {
            take_id: live.take.clone(),
            seconds: live.micros.load(Ordering::Relaxed) as f64 / 1e6,
            microphone: live.microphone,
        });
    } else if let Some(interrupted) = session.active.take() {
        // MKV keeps completed clusters after a crash; recover them before exposing the studio.
        match capture::finalize(&s.store,id,&interrupted.take_id).await {
            Ok(duration)=>{
                let marks=read_json(&asset(&s.store,id,&format!("take-{}-marks.json",interrupted.take_id))?).await.unwrap_or_default();
                append_take(&mut session,interrupted.take_id,duration,interrupted.microphone,marks,true);
            },
            Err(_)=>session.error=Some("The interrupted take could not be finalized. Its source is retained in this recording folder.".into()),
        }
        session.revision += 1;
        save(&s.store, &session).await?;
    }
    if session
        .export
        .as_ref()
        .is_some_and(|e| e.status == "rendering")
        && !s.recordings.renders.lock().await.contains(id)
    {
        if let Some(job) = session.export.as_mut() {
            job.status = "error".into();
            job.error = Some(
                "Export was interrupted. Your source takes are safe; export the sequence again."
                    .into(),
            );
        }
        save(&s.store, &session).await?;
    }
    Ok(session)
}
async fn devices() -> Result<Json<RecordingDevices>> {
    Ok(Json(capture::devices().await?))
}
async fn list(State(s): State<AppState>) -> Result<Json<Vec<RecordingSession>>> {
    let _lock = s.recordings.lock.lock().await;
    let base = s.store.data.join("recordings");
    s.store.directory(&base)?;
    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if let Ok(session) = read(&s, &id).await {
                sessions.push(session);
            }
        }
    }
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(Json(sessions))
}
#[derive(Deserialize)]
struct NewRecording {
    name: String,
    orientation: String,
}
fn validate_header(name: &str, orientation: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
        || !["16:9", "9:16"].contains(&orientation)
    {
        return Err(AppError::bad(
            "Choose a recording name up to 120 characters and horizontal or vertical video.",
        ));
    }
    Ok(())
}
async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewRecording>,
) -> Result<Json<RecordingSession>> {
    validate_header(&input.name, &input.orientation)?;
    let _lock = s.recordings.lock.lock().await;
    let session = RecordingSession {
        id: uuid::Uuid::new_v4().to_string(),
        name: input.name.trim().into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        orientation: input.orientation,
        ..Default::default()
    };
    save(&s.store, &session).await?;
    Ok(Json(session))
}
async fn load(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<RecordingSession>> {
    let _lock = s.recordings.lock.lock().await;
    Ok(Json(read(&s, &id).await?))
}
fn validate_edit(old: &RecordingSession, input: &RecordingEdit) -> Result<()> {
    validate_header(&input.name, &input.orientation)?;
    if old.revision != input.revision {
        return Err(AppError::conflict(
            "This recording changed in another window. Reload before saving.",
        ));
    }
    if old.active.is_some() || old.export.as_ref().is_some_and(|e| e.status == "rendering") {
        return Err(AppError::conflict(
            "Finish the current take or export before editing the sequence.",
        ));
    }
    if input.clips.len() > 2000 || input.overlays.len() > 6 {
        return Err(AppError::bad(
            "Use up to 2,000 sequence clips and six sidebar overlays.",
        ));
    }
    let takes: HashMap<_, _> = old.takes.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut ids = HashSet::new();
    for clip in &input.clips {
        identifier(&clip.id)?;
        let take = takes
            .get(clip.take_id.as_str())
            .ok_or_else(|| AppError::bad("That source take is unavailable."))?;
        if !ids.insert(&clip.id)
            || clip.name.trim().is_empty()
            || clip.name.chars().count() > 120
            || !clip.r#in.is_finite()
            || !clip.out.is_finite()
            || clip.r#in < 0.
            || clip.out - clip.r#in < 0.1
            || clip.out > take.duration + 0.02
        {
            return Err(AppError::bad(
                "Clip ranges must remain inside their original take and at least 0.1 seconds long.",
            ));
        }
    }
    let mut overlay_ids = HashSet::new();
    for overlay in &input.overlays {
        identifier(&overlay.id)?;
        if !overlay_ids.insert(&overlay.id)
            || overlay.label.trim().is_empty()
            || overlay.label.chars().count() > 80
            || overlay.text.chars().count() > 600
            || overlay.secondary.chars().count() > 200
            || !["lowerThird", "quote", "list", "stat"].contains(&overlay.kind.as_str())
            || overlay
                .text
                .chars()
                .chain(overlay.secondary.chars())
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(AppError::bad(
                "Choose a supported overlay and keep its text within the displayed limits.",
            ));
        }
    }
    Ok(())
}
async fn edit(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RecordingEdit>,
) -> Result<Json<RecordingSession>> {
    let _lock = s.recordings.lock.lock().await;
    let mut session = read(&s, &id).await?;
    validate_edit(&session, &input)?;
    let backup = directory(&s.store, &id)?.join("backups");
    s.store.directory(&backup)?;
    write_json(
        &backup.join(format!(
            "revision-{}-{}.json",
            session.revision,
            uuid::Uuid::new_v4()
        )),
        &session,
    )
    .await?;
    session.name = input.name.trim().into();
    session.orientation = input.orientation;
    session.clips = input.clips;
    session.overlays = input.overlays;
    session.revision += 1;
    session.error = None;
    if !session
        .overlays
        .iter()
        .any(|o| Some(&o.id) == session.active_overlay.as_ref())
    {
        session.active_overlay = None;
    }
    save(&s.store, &session).await?;
    Ok(Json(session))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartTake {
    camera_id: String,
    microphone_id: Option<String>,
}
async fn start(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<StartTake>,
) -> Result<Json<RecordingSession>> {
    let guard = s.recordings.lock.lock().await;
    if s.recordings.live.lock().await.is_some() {
        return Err(AppError::conflict(
            "Stop the current recording before starting another take.",
        ));
    }
    let mut session = read(&s, &id).await?;
    if session
        .export
        .as_ref()
        .is_some_and(|e| e.status == "rendering")
    {
        return Err(AppError::conflict(
            "Wait for the export to finish before recording.",
        ));
    }
    let spec = capture::input(&input.camera_id, input.microphone_id.as_deref()).await?;
    let take = uuid::Uuid::new_v4().to_string();
    let (frame_tx, frame) = watch::channel(Vec::new());
    let (done_tx, done) = watch::channel(false);
    let live = Live {
        session: id.clone(),
        take: take.clone(),
        microphone: input.microphone_id.is_some(),
        cancel: CancellationToken::new(),
        frame,
        done,
        micros: Arc::default(),
        marks: Arc::new(Mutex::new(vec![OverlayMark {
            at: 0.,
            overlay_id: session.active_overlay.clone(),
        }])),
    };
    write_json(
        &asset(&s.store, &id, &format!("take-{take}-marks.json"))?,
        &*live.marks.lock().await,
    )
    .await?;
    session.active = Some(ActiveRecording {
        take_id: take.clone(),
        seconds: 0.,
        microphone: live.microphone,
    });
    session.error = None;
    session.revision += 1;
    save(&s.store, &session).await?;
    *s.recordings.live.lock().await = Some(live.clone());
    let task_state = s.clone();
    let task_id = id.clone();
    let task_live = live.clone();
    tokio::spawn(async move {
        let result = capture::run(
            &task_state.store,
            &task_id,
            &take,
            spec,
            &task_live,
            frame_tx,
        )
        .await;
        let _guard = task_state.recordings.lock.lock().await;
        let result = async {
            let mut saved: RecordingSession =
                read_json(&asset(&task_state.store, &task_id, "session.json")?).await?;
            saved.active = None;
            saved.revision += 1;
            match result {
                Ok((duration, interrupted)) => append_take(
                    &mut saved,
                    take,
                    duration,
                    task_live.microphone,
                    task_live.marks.lock().await.clone(),
                    interrupted,
                ),
                Err(error) => saved.error = Some(error.to_string()),
            }
            save(&task_state.store, &saved).await
        }
        .await;
        // A failed manifest save leaves the active marker and recoverable source for the next load.
        let _ = result;
        *task_state.recordings.live.lock().await = None;
        let _ = done_tx.send(true);
    });
    drop(guard);
    let mut ready = live.frame.clone();
    let mut done = live.done.clone();
    let started = tokio::select! {
        value=ready.wait_for(|bytes|!bytes.is_empty())=>value.is_ok(),
        _=done.wait_for(|value|*value)=>false,
        _=tokio::time::sleep(Duration::from_secs(15))=>false,
    };
    if !started {
        live.cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(15), done.wait_for(|value| *value)).await;
        let _lock = s.recordings.lock.lock().await;
        let saved = read(&s, &id).await?;
        return Err(AppError::bad(saved.error.unwrap_or_else(||"The camera did not become ready. Check its connection and permissions, then try again.".into())));
    }
    let _lock = s.recordings.lock.lock().await;
    Ok(Json(read(&s, &id).await?))
}
fn append_take(
    session: &mut RecordingSession,
    id: String,
    duration: f64,
    audio: bool,
    marks: Vec<OverlayMark>,
    interrupted: bool,
) {
    let name = format!("Take {}", session.takes.len() + 1);
    session.clips.push(RecordingClip {
        id: uuid::Uuid::new_v4().to_string(),
        take_id: id.clone(),
        name: name.clone(),
        r#in: 0.,
        out: duration,
    });
    session.takes.push(RecordedTake {
        id,
        name,
        duration,
        has_audio: audio,
        marks,
        interrupted,
    });
    if interrupted {
        session.error =
            Some("Recovered an interrupted take. Check its ending before exporting.".into());
    }
}
async fn stop(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<RecordingSession>> {
    identifier(&id)?;
    let live = s
        .recordings
        .live
        .lock()
        .await
        .clone()
        .filter(|l| l.session == id);
    if let Some(mut live) = live {
        live.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(30), live.done.wait_for(|v| *v))
            .await
            .map_err(|_| {
                AppError::bad(
                    "The take is still finalizing. Keep the studio open and try again shortly.",
                )
            })?
            .map_err(|_| AppError::bad("The recording service stopped."))?;
    }
    let _lock = s.recordings.lock.lock().await;
    Ok(Json(read(&s, &id).await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectOverlay {
    overlay_id: Option<String>,
}
async fn overlay(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SelectOverlay>,
) -> Result<Json<RecordingSession>> {
    let _lock = s.recordings.lock.lock().await;
    let mut session = read(&s, &id).await?;
    if let Some(id) = &input.overlay_id
        && !session
            .overlays
            .iter()
            .any(|o| &o.id == id && !o.text.trim().is_empty())
    {
        return Err(AppError::bad("Write the overlay text before showing it."));
    }
    if session.active_overlay != input.overlay_id {
        if let Some(live) = s
            .recordings
            .live
            .lock()
            .await
            .as_ref()
            .filter(|l| l.session == id)
        {
            let mut marks = live.marks.lock().await;
            if marks.len() >= 2000 {
                return Err(AppError::bad(
                    "This take reached its overlay limit. Stop and start another take.",
                ));
            }
            marks.push(OverlayMark {
                at: live.micros.load(Ordering::Relaxed) as f64 / 1e6,
                overlay_id: input.overlay_id.clone(),
            });
            write_json(
                &asset(&s.store, &id, &format!("take-{}-marks.json", live.take))?,
                &*marks,
            )
            .await?;
        }
        session.active_overlay = input.overlay_id;
        save(&s.store, &session).await?;
    }
    Ok(Json(session))
}
async fn preview(State(s): State<AppState>, Path(id): Path<String>) -> Result<Response> {
    let live = s
        .recordings
        .live
        .lock()
        .await
        .clone()
        .filter(|l| l.session == id)
        .ok_or_else(|| AppError::bad("Camera is off."))?;
    let bytes = live.frame.borrow().clone();
    if bytes.is_empty() {
        return Err(AppError::bad("The camera is starting."));
    }
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response())
}
async fn served(file: PathBuf, request: axum::extract::Request) -> Result<Response> {
    Ok(ServeFile::new(file)
        .oneshot(request)
        .await
        .map_err(|_| AppError::bad("Recording media is unavailable."))?
        .map(Body::new))
}
async fn take_file(
    State(s): State<AppState>,
    Path((id, take)): Path<(String, String)>,
    request: axum::extract::Request,
) -> Result<Response> {
    identifier(&take)?;
    let _lock = s.recordings.lock.lock().await;
    let session = read(&s, &id).await?;
    if !session.takes.iter().any(|t| t.id == take) {
        return Err(AppError::bad("Source take not found."));
    }
    served(asset(&s.store, &id, &format!("take-{take}.mp4"))?, request).await
}
async fn export_file(
    State(s): State<AppState>,
    Path((id, export)): Path<(String, String)>,
    request: axum::extract::Request,
) -> Result<Response> {
    identifier(&export)?;
    let file = asset(&s.store, &id, &format!("export-{export}.mp4"))?;
    if !file.is_file() {
        return Err(AppError::bad("Export is not ready."));
    }
    served(file, request).await
}
async fn export(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RecordingSession>> {
    let _lock = s.recordings.lock.lock().await;
    let mut session = read(&s, &id).await?;
    if session.active.is_some()
        || session
            .export
            .as_ref()
            .is_some_and(|e| e.status == "rendering")
    {
        return Err(AppError::conflict(
            "Stop recording or finish the current export first.",
        ));
    }
    if session.clips.is_empty() {
        return Err(AppError::bad("Add at least one take to the sequence."));
    }
    let total: f64 = session.clips.iter().map(|c| c.out - c.r#in).sum();
    if total > 7200. {
        return Err(AppError::bad(
            "Keep each recording sequence within two hours.",
        ));
    }
    let job = RecordingExport {
        id: uuid::Uuid::new_v4().to_string(),
        status: "rendering".into(),
        pct: 0.,
        revision: session.revision,
        error: None,
    };
    session.export = Some(job.clone());
    s.recordings.renders.lock().await.insert(id.clone());
    save(&s.store, &session).await?;
    let snapshot = session.clone();
    let task_state = s.clone();
    tokio::spawn(async move {
        let s = task_state;
        let result = async {
            let _permit = s
                .export_queue
                .acquire()
                .await
                .map_err(|_| AppError::bad("The export queue stopped."))?;
            render::render(&s.store, &snapshot, &job.id).await
        }
        .await;
        let _lock = s.recordings.lock.lock().await;
        if let Ok(mut saved) =
            read_json::<RecordingSession>(&asset(&s.store, &id, "session.json").unwrap_or_default())
                .await
        {
            if let Some(export) = saved.export.as_mut().filter(|e| e.id == job.id) {
                match result {
                    Ok(()) => {
                        export.status = "done".into();
                        export.pct = 100.;
                    }
                    Err(e) => {
                        export.status = "error".into();
                        export.error = Some(e.to_string());
                    }
                }
            }
            let _ = save(&s.store, &saved).await;
        }
        s.recordings.renders.lock().await.remove(&id);
    });
    Ok(Json(session))
}
#[derive(Deserialize)]
struct EditorInput {
    #[serde(default)]
    manual: bool,
}
async fn editor(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<EditorInput>,
) -> Result<Json<Value>> {
    let _lock = s.recordings.lock.lock().await;
    let session = read(&s, &id).await?;
    let job = session
        .export
        .as_ref()
        .filter(|e| e.status == "done" && e.revision == session.revision)
        .ok_or_else(|| {
            AppError::bad("Export the current sequence before sending it to the editor.")
        })?;
    if !session.takes.iter().any(|t| t.has_audio) {
        return Err(AppError::bad(
            "This recording has no microphone audio. Save its MP4, or record a take with the microphone on to use automatic transcription.",
        ));
    }
    if media::tools_status(&s.store).await["transcription"] != true {
        return Err(AppError::bad(
            "Set up local Whisper before sending a recording to the sentence editor.",
        ));
    }
    let mut project = api::new_project(session.name.clone());
    project.export_settings.ratio = session.orientation.clone();
    let dir = s.store.project_dir(&project.id)?;
    s.store.directory(&dir)?;
    tokio::fs::copy(
        asset(&s.store, &id, &format!("export-{}.mp4", job.id))?,
        dir.join("source-A.mp4"),
    )
    .await?;
    project.cameras.push(Camera {
        id: "A".into(),
        name: "Recording".into(),
        source_path: "source-A.mp4".into(),
        proxy_path: "proxy-A.mp4".into(),
        ..Default::default()
    });
    let processing = ProcessingInput {
        manual: input.manual,
        ..Default::default()
    };
    write_json(&s.store.asset(&project.id, "input.json")?, &processing).await?;
    s.store.save(&project).await?;
    let project_id = project.id.clone();
    api::enqueue(s.clone(), project, processing, false).await?;
    Ok(Json(json!({"projectId":project_id})))
}

/// Internal YouTube handoff: only validated, locally prepared media can become a take.
pub(crate) async fn import_take(
    s: &AppState,
    id: &str,
    take_id: &str,
    source: &std::path::Path,
    name: &str,
) -> Result<RecordingSession> {
    identifier(take_id)?;
    reject_link(source)?;
    let size = std::fs::metadata(source)?.len();
    if size == 0 || size > 4 * 1024 * 1024 * 1024 {
        return Err(AppError::bad(
            "Imported clips must be nonempty and smaller than 4 GiB.",
        ));
    }
    let _lock = s.recordings.lock.lock().await;
    let mut session = read(s, id).await?;
    if session.takes.iter().any(|t| t.id == take_id) {
        return Ok(session);
    }
    if session.active.is_some()
        || session
            .export
            .as_ref()
            .is_some_and(|e| e.status == "rendering")
    {
        return Err(AppError::conflict(
            "Finish the current recording or export before adding the YouTube clip.",
        ));
    }
    let info = media::probe(source).await?;
    let destination = asset(&s.store, id, &format!("take-{take_id}.mp4"))?;
    if destination.exists() {
        if std::fs::metadata(&destination)?.len() != size {
            return Err(AppError::conflict(
                "That imported source already exists with different content.",
            ));
        }
        media::probe(&destination).await?;
    } else {
        let pending = asset(&s.store, id, &format!("import-{take_id}.mp4"))?;
        tokio::fs::copy(source, &pending).await?;
        media::probe(&pending).await?;
        tokio::fs::rename(pending, &destination).await?;
    }
    append_take(
        &mut session,
        take_id.into(),
        info.duration,
        true,
        Vec::new(),
        false,
    );
    let label: String = name.chars().filter(|c| !c.is_control()).take(120).collect();
    let label = if label.trim().is_empty() {
        "YouTube clip"
    } else {
        label.trim()
    };
    session.takes.last_mut().unwrap().name = label.into();
    session.clips.last_mut().unwrap().name = label.into();
    session.revision += 1;
    session.error = None;
    save(&s.store, &session).await?;
    Ok(session)
}

/// Returns an immutable export; creation checks additionally require the current revision.
pub(crate) fn exported_path(store: &Store, id: &str, export: &str) -> Result<PathBuf> {
    identifier(export)?;
    let path = asset(store, id, &format!("export-{export}.mp4"))?;
    if !path.is_file() {
        return Err(AppError::bad("That recording export is unavailable."));
    }
    Ok(path)
}
pub(crate) async fn export_for_upload(
    s: &AppState,
    id: &str,
    export: &str,
) -> Result<(PathBuf, String)> {
    let _lock = s.recordings.lock.lock().await;
    let session = read(s, id).await?;
    if !session.export.as_ref().is_some_and(|job| {
        job.id == export && job.status == "done" && job.revision == session.revision
    }) {
        return Err(AppError::bad(
            "Export the current recording sequence before uploading it to YouTube.",
        ));
    }
    Ok((exported_path(&s.store, id, export)?, session.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    fn fixture() -> RecordingSession {
        RecordingSession {
            id: "studio".into(),
            name: "Interview".into(),
            orientation: "16:9".into(),
            takes: vec![RecordedTake {
                id: "take".into(),
                duration: 10.,
                ..Default::default()
            }],
            clips: vec![RecordingClip {
                id: "clip".into(),
                take_id: "take".into(),
                name: "Take 1".into(),
                r#in: 0.,
                out: 10.,
            }],
            ..Default::default()
        }
    }
    fn editing(s: &RecordingSession) -> RecordingEdit {
        RecordingEdit {
            revision: s.revision,
            name: s.name.clone(),
            orientation: s.orientation.clone(),
            clips: s.clips.clone(),
            overlays: s.overlays.clone(),
        }
    }
    #[test]
    fn edits_preserve_original_sources_and_reject_invalid_ranges_and_stale_revisions() {
        let s = fixture();
        let mut input = editing(&s);
        input.clips[0].r#in = 2.;
        input.clips[0].out = 8.;
        assert!(validate_edit(&s, &input).is_ok());
        input.clips.push(RecordingClip {
            id: "split".into(),
            take_id: "take".into(),
            name: "Second half".into(),
            r#in: 8.,
            out: 10.,
        });
        assert!(validate_edit(&s, &input).is_ok());
        input.clips[0].out = 11.;
        assert!(validate_edit(&s, &input).is_err());
        input.clips[0].out = 8.;
        input.clips[0].take_id = "elsewhere".into();
        assert!(validate_edit(&s, &input).is_err());
        input = editing(&s);
        input.revision = 1;
        assert_eq!(
            validate_edit(&s, &input).unwrap_err().status,
            StatusCode::CONFLICT
        );
        input = editing(&s);
        input.clips.push(input.clips[0].clone());
        assert!(validate_edit(&s, &input).is_err());
        assert_eq!(s.takes[0].duration, 10.);
    }
    #[tokio::test]
    async fn paths_are_private_and_device_open_requires_a_detected_choice() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), temp.path().join("data")).unwrap();
        assert!(directory(&store, "..").is_err());
        assert!(asset(&store, "studio", "../../private").is_err());
        assert!(
            capture::input("https://example.invalid/video", None)
                .await
                .is_err()
        );
        #[cfg(unix)]
        {
            store.directory(&store.data.join("recordings")).unwrap();
            std::os::unix::fs::symlink(temp.path(), store.data.join("recordings/linked")).unwrap();
            assert!(asset(&store, "linked", "session.json").is_err());
        }
    }
    #[tokio::test]
    #[ignore = "Opt-in FFmpeg integration; run with STORYFOLD_CAPTURE_TEST=1, no physical camera is used"]
    async fn generated_capture_preview_overlay_export_and_recovery() {
        use axum::http::StatusCode;
        assert!(
            capture::test_source(),
            "Set STORYFOLD_CAPTURE_TEST=1 for this generated-media check."
        );
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), temp.path().join("data")).unwrap();
        let state = AppState::new(store.clone());
        let Json(mut session) = create(
            State(state.clone()),
            Json(NewRecording {
                name: "Generated recording check".into(),
                orientation: "9:16".into(),
            }),
        )
        .await
        .unwrap();
        let mut input = editing(&session);
        input.overlays.push(RecordingOverlay {
            id: "name".into(),
            label: "Name".into(),
            kind: "lowerThird".into(),
            text: "Recording works".into(),
            secondary: "Local studio".into(),
        });
        session = edit(State(state.clone()), Path(session.id), Json(input))
            .await
            .unwrap()
            .0;
        let id = session.id.clone();
        let first = start(
            State(state.clone()),
            Path(id.clone()),
            Json(StartTake {
                camera_id: "synthetic".into(),
                microphone_id: Some("synthetic-audio".into()),
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(first.active.is_some());
        assert_eq!(
            preview(State(state.clone()), Path(id.clone()))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert!(
            start(
                State(state.clone()),
                Path(id.clone()),
                Json(StartTake {
                    camera_id: "synthetic".into(),
                    microphone_id: None
                })
            )
            .await
            .is_err()
        );
        let _ = overlay(
            State(state.clone()),
            Path(id.clone()),
            Json(SelectOverlay {
                overlay_id: Some("name".into()),
            }),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(1400)).await;
        let finished = stop(State(state.clone()), Path(id.clone()))
            .await
            .unwrap()
            .0;
        assert!(finished.active.is_none(), "{:?}", finished.error);
        assert_eq!(finished.takes.len(), 1, "{:?}", finished.error);
        assert!(finished.takes[0].duration > 1.);
        assert!(
            finished.takes[0]
                .marks
                .iter()
                .any(|m| m.overlay_id.as_deref() == Some("name"))
        );
        let original = std::fs::read(
            asset(&store, &id, &format!("take-{}.mp4", finished.takes[0].id)).unwrap(),
        )
        .unwrap();
        let mut input = editing(&finished);
        input.clips[0].r#in = 0.2;
        input.clips[0].out = finished.takes[0].duration - 0.1;
        let _ = edit(State(state.clone()), Path(id.clone()), Json(input.clone()))
            .await
            .unwrap();
        assert!(
            edit(State(state.clone()), Path(id.clone()), Json(input))
                .await
                .is_err()
        );
        let _ = export(State(state.clone()), Path(id.clone()))
            .await
            .unwrap();
        let rendered = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let s = load(State(state.clone()), Path(id.clone()))
                    .await
                    .unwrap()
                    .0;
                if s.export.as_ref().unwrap().status != "rendering" {
                    break s;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
        let job = rendered.export.as_ref().unwrap();
        assert_eq!(job.status, "done", "{:?}", job.error);
        let info = media::probe(&asset(&store, &id, &format!("export-{}.mp4", job.id)).unwrap())
            .await
            .unwrap();
        assert_eq!((info.width, info.height), (720, 1280));
        assert_eq!(
            std::fs::read(
                asset(&store, &id, &format!("take-{}.mp4", finished.takes[0].id)).unwrap()
            )
            .unwrap(),
            original
        );
        let mut interrupted = rendered.clone();
        interrupted.active = Some(ActiveRecording {
            take_id: finished.takes[0].id.clone(),
            seconds: 0.,
            microphone: true,
        });
        interrupted.takes.clear();
        interrupted.clips.clear();
        save(&store, &interrupted).await.unwrap();
        let recovered = load(State(state.clone()), Path(id.clone()))
            .await
            .unwrap()
            .0;
        assert!(recovered.active.is_none());
        assert_eq!(recovered.takes.len(), 1);
        assert!(recovered.takes[0].interrupted);
        assert!(!recovered.takes[0].marks.is_empty());
        let source = asset(&store, &id, &format!("take-{}.mp4", finished.takes[0].id)).unwrap();
        let imported = import_take(&state, &id, "imported", &source, "Imported source")
            .await
            .unwrap();
        let retried = import_take(&state, &id, "imported", &source, "Imported source")
            .await
            .unwrap();
        assert_eq!(imported.takes.len(), 2);
        assert_eq!(retried.takes.len(), 2);
        assert_eq!(retried.revision, imported.revision);
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}
