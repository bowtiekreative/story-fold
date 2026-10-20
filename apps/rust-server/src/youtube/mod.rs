//! Explicit YouTube clip imports and resumable, user-requested uploads.
mod auth;
mod download;
mod upload;

use crate::{
    AppError, Result, Store,
    api::AppState,
    recordings,
    storage::{read_json, reject_link, write_json},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct YouTube {
    control: Arc<Mutex<()>>,
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
    queue: Arc<Semaphore>,
    auth_lock: Arc<Mutex<()>>,
    pending: Arc<Mutex<Option<CancellationToken>>>,
    notice: Arc<Mutex<Option<String>>>,
}
impl Default for YouTube {
    fn default() -> Self {
        Self {
            control: Default::default(),
            active: Default::default(),
            queue: Arc::new(Semaphore::new(1)),
            auth_lock: Default::default(),
            pending: Default::default(),
            notice: Default::default(),
        }
    }
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/youtube/status", get(status))
        .route("/api/youtube/config", post(auth::configure))
        .route("/api/youtube/connect", post(auth::connect))
        .route("/api/youtube/disconnect", post(auth::disconnect))
        .route("/api/youtube/imports", post(import))
        .route("/api/youtube/uploads", post(upload))
        .route("/api/youtube/jobs", get(jobs))
        .route("/api/youtube/jobs/{id}", get(job))
        .route("/api/youtube/jobs/{id}/cancel", post(cancel))
        .route("/api/youtube/jobs/{id}/retry", post(retry))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobView {
    id: String,
    kind: String,
    recording_id: String,
    status: String,
    stage: String,
    progress: f64,
    error: Option<String>,
    created_at: String,
    video_id: Option<String>,
    url: Option<String>,
    bytes_sent: u64,
    bytes_total: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportInput {
    recording_id: String,
    url: String,
    start: f64,
    end: f64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UploadInput {
    recording_id: String,
    channel_id: String,
    export_id: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default = "private")]
    privacy: String,
    // Required: an explicit choice belongs in the upload review form.
    made_for_kids: bool,
}
fn private() -> String {
    "private".into()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Request {
    Import(ImportInput),
    Upload(UploadInput),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedJob {
    view: JobView,
    request: Request,
    #[serde(default)]
    upload: Option<upload::Checkpoint>,
}
fn identifier(id: &str) -> Result<()> {
    if !storyfold_core::identifier(id, 60) {
        return Err(AppError::bad(
            "Invalid YouTube job or recording identifier.",
        ));
    }
    Ok(())
}
fn base(store: &Store) -> Result<PathBuf> {
    let path = store.data.join("youtube");
    store.directory(&path)?;
    Ok(path)
}
fn directory(store: &Store, id: &str) -> Result<PathBuf> {
    identifier(id)?;
    let path = base(store)?.join("jobs");
    store.directory(&path)?;
    let path = path.join(id);
    reject_link(&path)?;
    store.directory(&path)?;
    Ok(path)
}
fn asset(store: &Store, id: &str, file: &str) -> Result<PathBuf> {
    let p = directory(store, id)?.join(file);
    reject_link(&p)?;
    Ok(p)
}
async fn save(s: &AppState, j: &SavedJob) -> Result<()> {
    write_json(&asset(&s.store, &j.view.id, "job.json")?, j).await
}
async fn read(s: &AppState, id: &str) -> Result<SavedJob> {
    let mut j: SavedJob = read_json(&asset(&s.store, id, "job.json")?).await?;
    if j.view.id != id {
        return Err(AppError::bad("YouTube job identity is invalid."));
    }
    if ["queued", "running"].contains(&j.view.status.as_str())
        && !s.youtube.active.lock().await.contains_key(id)
    {
        j.view.status = "error".into();
        j.view.error = Some("This job was interrupted. Retry to resume its saved work.".into());
        j.view.stage = "Interrupted".into();
    }
    Ok(j)
}
async fn status(State(s): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(auth::status(&s).await?))
}
async fn jobs(State(s): State<AppState>) -> Result<Json<Vec<JobView>>> {
    let dir = base(&s.store)?.join("jobs");
    s.store.directory(&dir)?;
    let mut result = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if let Ok(j) = read(&s, &id).await {
                result.push(j.view);
            }
        }
    }
    result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    result.truncate(100);
    Ok(Json(result))
}
async fn job(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<JobView>> {
    Ok(Json(read(&s, &id).await?.view))
}
async fn import(
    State(s): State<AppState>,
    Json(mut input): Json<ImportInput>,
) -> Result<Json<JobView>> {
    identifier(&input.recording_id)?;
    input.url = download::canonical_url(&input.url)?;
    download::validate_range(input.start, input.end)?;
    // Check existence through the recording API helper's store before enqueueing.
    let file = s
        .store
        .data
        .join("recordings")
        .join(&input.recording_id)
        .join("session.json");
    reject_link(file.parent().unwrap())?;
    reject_link(&file)?;
    if !file.is_file() {
        return Err(AppError::bad(
            "Create or open a recording studio before importing a clip.",
        ));
    }
    if !download::available(&s.store) {
        return Err(AppError::bad(
            "Install the YouTube importer once with python3 scripts/setup-youtube.py, then retry.",
        ));
    }
    start(&s, Request::Import(input)).await
}
async fn upload(
    State(s): State<AppState>,
    Json(input): Json<UploadInput>,
) -> Result<Json<JobView>> {
    upload::validate_input(&input)?;
    // The submitted channel ID is the one the user reviewed, not whichever
    // account another window may have connected since that review.
    let account = auth::status(&s).await?;
    if !account["connected"].as_bool().unwrap_or(false) {
        return Err(AppError::bad(
            "Connect your YouTube channel before uploading.",
        ));
    }
    if account["channel"]["id"].as_str() != Some(input.channel_id.as_str()) {
        return Err(AppError::conflict(
            "The connected YouTube channel changed. Review the displayed channel again before uploading.",
        ));
    }
    let _ = recordings::export_for_upload(&s, &input.recording_id, &input.export_id).await?;
    start(&s, Request::Upload(input)).await
}
async fn start(s: &AppState, request: Request) -> Result<Json<JobView>> {
    let _guard = s.youtube.control.lock().await;
    let (kind, id) = match &request {
        Request::Import(i) => ("import", i.recording_id.clone()),
        Request::Upload(i) => ("upload", i.recording_id.clone()),
    };
    for active_id in s.youtube.active.lock().await.keys() {
        // Do not nest read(active_id): it checks this same active mutex.
        let old: SavedJob = read_json(&asset(&s.store, active_id, "job.json")?).await?;
        if old.view.recording_id == id && old.view.kind == kind {
            return Err(AppError::conflict(
                "This recording already has an active YouTube job of that kind.",
            ));
        }
    }
    let j = SavedJob {
        view: JobView {
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            recording_id: id,
            status: "queued".into(),
            stage: "Waiting".into(),
            progress: 0.,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            video_id: None,
            url: None,
            bytes_sent: 0,
            bytes_total: 0,
        },
        request,
        upload: None,
    };
    save(s, &j).await?;
    let view = j.view.clone();
    enqueue(s.clone(), j).await;
    Ok(Json(view))
}
async fn cancel(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<JobView>> {
    let _ = read(&s, &id).await?;
    if let Some(cancel) = s.youtube.active.lock().await.get(&id) {
        cancel.cancel();
    }
    Ok(Json(read(&s, &id).await?.view))
}
async fn retry(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<JobView>> {
    let _guard = s.youtube.control.lock().await;
    if s.youtube.active.lock().await.contains_key(&id) {
        return Err(AppError::conflict("This YouTube job is already running."));
    }
    let mut j = read(&s, &id).await?;
    if !["error", "cancelled"].contains(&j.view.status.as_str()) {
        return Err(AppError::conflict(
            "Only interrupted or cancelled jobs can be retried.",
        ));
    }
    let active_ids: Vec<_> = s.youtube.active.lock().await.keys().cloned().collect();
    for active_id in active_ids {
        let old: SavedJob = read_json(&asset(&s.store, &active_id, "job.json")?).await?;
        if old.view.recording_id == j.view.recording_id && old.view.kind == j.view.kind {
            return Err(AppError::conflict(
                "This recording already has an active YouTube job of that kind.",
            ));
        }
    }
    // Validate the same immutable export; an expired session is deliberately not silently replaced.
    if let Request::Upload(input) = &j.request {
        recordings::exported_path(&s.store, &input.recording_id, &input.export_id)?;
    }
    j.view.status = "queued".into();
    j.view.stage = "Waiting to retry".into();
    j.view.error = None;
    save(&s, &j).await?;
    let view = j.view.clone();
    enqueue(s.clone(), j).await;
    Ok(Json(view))
}
async fn enqueue(s: AppState, mut j: SavedJob) {
    let cancel = CancellationToken::new();
    s.youtube
        .active
        .lock()
        .await
        .insert(j.view.id.clone(), cancel.clone());
    tokio::spawn(async move {
        let result=async {
            let _permit=tokio::select! { _=cancel.cancelled()=>return Err(AppError::bad("Cancelled.")), p=s.youtube.queue.acquire()=>p.map_err(|_|AppError::bad("YouTube queue is unavailable."))? };
            cancelled(&cancel)?; j.view.status="running".into(); save(&s,&j).await?;
            match j.request.clone() { Request::Import(i)=>download::run(&s,&mut j,&i,&cancel).await, Request::Upload(i)=>upload::run(&s,&mut j,&i,&cancel).await }
        }.await;
        if let Err(e) = result {
            j.view.status = if cancel.is_cancelled() {
                "cancelled"
            } else {
                "error"
            }
            .into();
            j.view.stage = if cancel.is_cancelled() {
                "Cancelled; saved work retained"
            } else {
                "Needs attention"
            }
            .into();
            j.view.error = if cancel.is_cancelled() {
                None
            } else {
                Some(e.message)
            };
        } else {
            j.view.status = "done".into();
            j.view.progress = 100.;
            j.view.stage = if j.view.kind == "import" {
                "Added to clip bin"
            } else {
                "Uploaded to YouTube"
            }
            .into();
        }
        let _ = save(&s, &j).await;
        s.youtube.active.lock().await.remove(&j.view.id);
    });
}
fn cancelled(c: &CancellationToken) -> Result<()> {
    if c.is_cancelled() {
        Err(AppError::bad("Cancelled."))
    } else {
        Ok(())
    }
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(Into::into)
}
async fn bounded_json(response: reqwest::Response) -> Result<Value> {
    use futures_util::StreamExt;
    if response.content_length().is_some_and(|n| n > 256 * 1024) {
        return Err(AppError::bad("YouTube returned an oversized response."));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > 256 * 1024 {
            return Err(AppError::bad("YouTube returned an oversized response."));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(Into::into)
}
#[cfg(test)]
mod tests;
