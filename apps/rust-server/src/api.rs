use crate::{
    AppError, Result, Store, control,
    media::{self, ExportJob, ProcessingInput},
    recordings,
    storage::{read_json, write_json},
    youtube,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use storyfold_core::{Camera, ExportSettings, Project, ProjectEdit, Word, sequence};
use tokio::{
    io::AsyncWriteExt,
    sync::{Mutex, Semaphore},
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tower_http::services::ServeFile;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    token: Arc<String>,
    processing: Arc<Mutex<HashMap<String, CancellationToken>>>,
    exports: Arc<Mutex<HashMap<String, CancellationToken>>>,
    queue: Arc<Semaphore>,
    pub(crate) export_queue: Arc<Semaphore>,
    pub control: control::Control,
    pub recordings: recordings::Recorder,
    pub youtube: youtube::YouTube,
}
impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            control: control::Control::new(store.clone()),
            recordings: recordings::Recorder::default(),
            youtube: youtube::YouTube::default(),
            store,
            token: Arc::new(format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )),
            processing: Default::default(),
            exports: Default::default(),
            queue: Arc::new(Semaphore::new(1)),
            export_queue: Arc::new(Semaphore::new(1)),
        }
    }
}

pub fn router(store: Store) -> Router {
    let state = AppState::new(store);
    Router::new()
        .route("/api/session", get(session))
        .route("/api/health", get(health))
        .route("/api/events", get(events))
        .route(
            "/api/projects",
            get(projects)
                .post(upload)
                .layer(DefaultBodyLimit::max(13 * 1024 * 1024 * 1024)),
        )
        .route("/api/projects/sample", post(sample))
        .route("/api/projects/{id}", get(project).patch(edit))
        .route("/api/projects/{id}/timeline", get(timeline))
        .route("/api/projects/{id}/resume", post(resume))
        .route("/api/projects/{id}/cancel", post(cancel))
        .route("/api/projects/{id}/media/{cam}", get(media_file))
        .route("/api/projects/{id}/peaks", get(peaks))
        .route("/api/projects/{id}/exports", post(start_export))
        .route("/api/projects/{pid}/exports/{id}", get(export_job))
        .route(
            "/api/projects/{pid}/exports/{id}/files/{name}",
            get(export_file),
        )
        .route(
            "/api/projects/{pid}/exports/{id}/cancel",
            post(cancel_export),
        )
        .merge(control::routes())
        .merge(recordings::routes())
        .merge(youtube::routes())
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"Endpoint not found."})),
            )
        })
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

async fn guard(State(s): State<AppState>, req: axum::extract::Request, next: Next) -> Response {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let local = host
        .parse::<axum::http::uri::Authority>()
        .is_ok_and(|a| ["localhost", "127.0.0.1", "[::1]"].contains(&a.host()));
    if !local {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"This app is available only on localhost."})),
        )
            .into_response();
    }
    if let Some(origin) = req.headers().get("origin") {
        let text = origin.to_str().unwrap_or("");
        let native_media = req.method() == Method::GET
            && req.uri().path().contains("/media/")
            && [
                "dioxus://index.html",
                "http://dioxus.localhost",
                "https://dioxus.localhost",
                "dioxus://localhost",
                "http://dioxus.index.html",
                "https://dioxus.index.html",
            ]
            .contains(&text);
        let local = text.parse::<axum::http::Uri>().is_ok_and(|uri| {
            uri.host()
                .is_some_and(|h| ["localhost", "127.0.0.1", "[::1]"].contains(&h))
        });
        if !local && !native_media {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Cross-origin requests are not allowed."})),
            )
                .into_response();
        }
    }
    if ![Method::GET, Method::HEAD, Method::OPTIONS].contains(req.method()) {
        let given = req
            .headers()
            .get("x-storyfold-token")
            .map_or(&[][..], |v| v.as_bytes());
        let expected = s.token.as_bytes();
        let mut difference = given.len() ^ expected.len();
        for (a, b) in given.iter().zip(expected) {
            difference |= (a ^ b) as usize;
        }
        if difference != 0 {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Your local session expired. Reload the application."})),
            )
                .into_response();
        }
    }
    let mut response = next.run(req).await;
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "same-origin"),
        ("x-frame-options", "SAMEORIGIN"),
        ("cache-control", "no-store"),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}
async fn session(State(s): State<AppState>) -> Json<Value> {
    Json(json!({"token":*s.token}))
}
async fn health(State(s): State<AppState>) -> Json<Value> {
    let mut v = media::tools_status(&s.store).await;
    v["ok"] = json!(true);
    v["runtime"] = json!("Rust");
    Json(v)
}
async fn events()
-> Sse<impl futures_util::Stream<Item = std::result::Result<Event, std::convert::Infallible>>> {
    Sse::new(futures_util::stream::unfold((), |()| async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Some((Ok(Event::default().event("update").data("{}")), ()))
    }))
    .keep_alive(KeepAlive::default())
}
async fn projects(State(s): State<AppState>) -> Result<Json<Value>> {
    let mut projects = Vec::new();
    let mut dirs = tokio::fs::read_dir(s.store.data.join("projects")).await?;
    while let Some(dir) = dirs.next_entry().await? {
        if !dir.file_type().await?.is_dir() {
            continue;
        }
        if let Ok(p) = s.store.read(&dir.file_name().to_string_lossy()).await {
            projects.push(json!({"id":p.id,"name":p.name,"createdAt":p.created_at,"status":p.status,"sentences":p.sentences.len(),"sample":p.sample}));
        }
    }
    projects.sort_by(|a, b| b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    Ok(Json(json!(projects)))
}
async fn project(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Project>> {
    let mut p = s.store.read(&id).await?;
    if !["ready", "error"].contains(&p.status.as_str())
        && !s.processing.lock().await.contains_key(&id)
    {
        p.status = "error".into();
        p.error = Some("Processing was interrupted. Resume to reuse cached work.".into());
    }
    Ok(Json(p))
}
async fn timeline(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    Ok(Json(serde_json::to_value(sequence(
        &s.store.read(&id).await?,
    ))?))
}
async fn edit(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProjectEdit>,
) -> Result<Json<Project>> {
    Ok(Json(s.store.edit(&id, input).await?))
}
pub(crate) fn new_project(name: String) -> Project {
    Project {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        created_at: chrono::Utc::now().to_rfc3339(),
        status: "uploading".into(),
        ..Default::default()
    }
}
pub(crate) async fn enqueue(
    s: AppState,
    p: Project,
    input: ProcessingInput,
    generate: bool,
) -> Result<()> {
    let mut active = s.processing.lock().await;
    if active.contains_key(&p.id) {
        return Err(AppError::conflict(
            "This project is already being processed.",
        ));
    }
    let token = CancellationToken::new();
    active.insert(p.id.clone(), token.clone());
    drop(active);
    tokio::spawn(async move {
        let id = p.id.clone();
        let work=async {
            let _permit=tokio::select!{r=s.queue.acquire()=>r.map_err(|_|AppError::bad("Processing queue stopped."))?,_=token.cancelled()=>return Err(AppError::bad("Processing cancelled. Resume to reuse cached work."))};
            let mut input=input;
            if generate {input.words=Some(media::create_sample(&s.store,&p).await?);write_json(&s.store.asset(&id,"input.json")?,&input).await?;}
            media::process(&s.store,p,input,token.clone()).await
        }.await;
        if let Err(e) = work
            && let Ok(mut p) = s.store.read(&id).await
        {
            p.status = "error".into();
            p.error = Some(e.to_string());
            let _ = s.store.save(&p).await;
        }
        s.processing.lock().await.remove(&id);
    });
    Ok(())
}
async fn sample(State(s): State<AppState>) -> Result<(StatusCode, Json<Value>)> {
    let mut p = new_project("The bakery · sample story".into());
    p.sample = true;
    p.warnings.push("Synthetic test video and supplied text. The tone is not speech. Use your interview to validate transcription and sync.".into());
    p.cameras.push(Camera {
        id: "A".into(),
        name: "Wide".into(),
        source_path: "source-A.mp4".into(),
        proxy_path: "proxy-A.mp4".into(),
        duration: 36.,
        ..Default::default()
    });
    s.store.save(&p).await?;
    let id = p.id.clone();
    enqueue(
        s,
        p,
        ProcessingInput {
            manual: true,
            sample: true,
            ..Default::default()
        },
        true,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"projectId":id}))))
}
async fn upload(
    State(s): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>)> {
    let uploads = s.store.data.join("uploads");
    s.store.directory(&uploads)?;
    let temp = tempfile::tempdir_in(&uploads)?;
    let mut videos = Vec::new();
    let mut transcript = None;
    let mut name = String::new();
    let mut manual = false;
    let mut fields = 0;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad("Upload data is invalid or too large."))?
    {
        fields += 1;
        if fields > 8 {
            return Err(AppError::bad("Too many upload fields."));
        }
        let key = field.name().unwrap_or("").to_owned();
        match key.as_str() {
            "files" => {
                if videos.len() >= 3 {
                    return Err(AppError::bad("Choose one to three video files."));
                }
                if name.is_empty() {
                    name = field
                        .file_name()
                        .and_then(|n| std::path::Path::new(n).file_stem())
                        .map(|n| n.to_string_lossy().chars().take(120).collect())
                        .unwrap_or_else(|| "Interview".into());
                }
                let path = temp
                    .path()
                    .join(format!("source-{}.mp4", ["A", "B", "C"][videos.len()]));
                let mut file = tokio::fs::File::create(&path).await?;
                let mut size = 0_u64;
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|_| AppError::bad("Video upload was interrupted."))?
                {
                    size += chunk.len() as u64;
                    if size > 4 * 1024 * 1024 * 1024 {
                        return Err(AppError::bad("Each video must be smaller than 4 GiB."));
                    }
                    file.write_all(&chunk).await?;
                }
                if size == 0 {
                    return Err(AppError::bad("The uploaded video is empty."));
                }
                file.sync_all().await?;
                videos.push(path);
            }
            "transcript" => {
                if transcript.is_some() {
                    return Err(AppError::bad("Import only one transcript."));
                }
                let mut data = Vec::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|_| AppError::bad("Transcript upload failed."))?
                {
                    if data.len() + chunk.len() > 20_000_000 {
                        return Err(AppError::bad(
                            "Timed transcript JSON must be smaller than 20 MB.",
                        ));
                    }
                    data.extend_from_slice(&chunk);
                }
                let v: Value = serde_json::from_slice(&data)?;
                let words: Vec<Word> = serde_json::from_value(v["words"].clone())?;
                storyfold_core::split_sentences(&words, 7200.).map_err(AppError::bad)?;
                transcript = Some(words);
            }
            "name" | "grouping" => {
                let mut data = Vec::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|_| AppError::bad("Invalid upload field."))?
                {
                    if data.len() + chunk.len() > 1000 {
                        return Err(AppError::bad("Upload field is too long."));
                    }
                    data.extend_from_slice(&chunk);
                }
                let text =
                    String::from_utf8(data).map_err(|_| AppError::bad("Invalid upload text."))?;
                if key == "name" {
                    if !text.trim().is_empty() {
                        name = text.trim().into();
                    }
                } else {
                    manual = text == "manual";
                }
            }
            _ => return Err(AppError::bad("Unknown upload field.")),
        }
    }
    if videos.is_empty() || name.trim().is_empty() || name.chars().count() > 120 {
        return Err(AppError::bad(
            "Choose one to three videos and a project name up to 120 characters.",
        ));
    }
    if transcript.is_none() && media::tools_status(&s.store).await["transcription"] != true {
        return Err(AppError::bad(
            "Whisper needs one-time setup on this computer before automatic transcription is available.",
        ));
    }
    let mut p = new_project(name);
    let dir = s.store.project_dir(&p.id)?;
    s.store.directory(&dir)?;
    for (i, file) in videos.iter().enumerate() {
        let id = ["A", "B", "C"][i];
        let source = format!("source-{id}.mp4");
        tokio::fs::rename(file, dir.join(&source)).await?;
        p.cameras.push(Camera {
            id: id.into(),
            name: ["Wide", "Close-up", "Side angle"][i].into(),
            source_path: source,
            proxy_path: format!("proxy-{id}.mp4"),
            ..Default::default()
        });
    }
    let input = ProcessingInput {
        words: transcript,
        manual,
        sample: false,
    };
    write_json(&s.store.asset(&p.id, "input.json")?, &input).await?;
    s.store.save(&p).await?;
    let id = p.id.clone();
    enqueue(s, p, input, false).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"projectId":id}))))
}
async fn resume(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Value>)> {
    let mut p = s.store.read(&id).await?;
    if p.status == "ready" {
        return Err(AppError::bad(
            "Only interrupted or failed projects can be resumed.",
        ));
    }
    let input = read_json(&s.store.asset(&id, "input.json")?).await?;
    p.error = None;
    p.status = "uploading".into();
    p.warnings.truncate(if p.sample { 1 } else { 0 });
    // Enqueue claims the project before its worker writes any processing state.
    enqueue(s, p, input, false).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"projectId":id}))))
}
async fn cancel(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    s.store.project_dir(&id)?;
    let jobs = s.processing.lock().await;
    let token = jobs
        .get(&id)
        .ok_or_else(|| AppError::bad("No processing job is running."))?;
    token.cancel();
    Ok(Json(json!({"projectId":id,"cancelled":true})))
}
async fn send_file(
    path: std::path::PathBuf,
    req: axum::extract::Request,
    download: Option<&str>,
) -> Result<Response> {
    crate::storage::reject_link(&path)?;
    let response = ServeFile::new(path).oneshot(req).await;
    let mut r = match response {
        Ok(r) => r.map(Body::new),
        Err(e) => match e {},
    };
    if let Some(name) = download {
        r.headers_mut().insert(
            "content-disposition",
            HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
                .map_err(|_| AppError::bad("Invalid download name."))?,
        );
    }
    Ok(r)
}
async fn media_file(
    State(s): State<AppState>,
    Path((id, cam)): Path<(String, String)>,
    req: axum::extract::Request,
) -> Result<Response> {
    let p = s.store.read(&id).await?;
    let c = p
        .cameras
        .iter()
        .find(|c| c.id == cam)
        .ok_or_else(|| AppError::bad("Camera not found."))?;
    send_file(s.store.asset(&id, &c.proxy_path)?, req, None).await
}
async fn peaks(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    Ok(Json(read_json(&s.store.asset(&id, "peaks.json")?).await?))
}
fn export_path(s: &Store, pid: &str, id: &str) -> Result<std::path::PathBuf> {
    if !storyfold_core::identifier(id, 60) {
        return Err(AppError::bad("Invalid export identifier."));
    }
    let dir = s.project_dir(pid)?.join("exports");
    crate::storage::reject_link(&dir)?;
    let dir = dir.join(id);
    crate::storage::reject_link(&dir)?;
    Ok(dir)
}
async fn start_export(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(settings): Json<ExportSettings>,
) -> Result<(StatusCode, Json<ExportJob>)> {
    settings.validate().map_err(AppError::bad)?;
    let mut p = s.store.read(&id).await?;
    if p.status != "ready" {
        return Err(AppError::bad("This project is not ready to export."));
    }
    if sequence(&p).clips.is_empty() {
        return Err(AppError::bad(
            "Add a folder or sentence to the story before exporting.",
        ));
    }
    let mut active = s.exports.lock().await;
    if active.keys().any(|key| key.starts_with(&format!("{id}:"))) {
        return Err(AppError::conflict(
            "This project already has an export running.",
        ));
    }
    p.export_settings = settings;
    let job = ExportJob {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: id.clone(),
        status: "queued".into(),
        pct: 0.,
        detail: "Waiting to render".into(),
        files: vec![],
        revision: p.revision,
        error: None,
    };
    let dir = export_path(&s.store, &id, &job.id)?;
    s.store.directory(&dir)?;
    write_json(&dir.join("snapshot.json"), &p).await?;
    write_json(&dir.join("job.json"), &job).await?;
    let key = format!("{id}:{}", job.id);
    let token = CancellationToken::new();
    active.insert(key.clone(), token.clone());
    drop(active);
    let copy = job.clone();
    tokio::spawn(async move {
        let permit = s.export_queue.acquire().await;
        if let Ok(_permit) = permit
            && let Err(e) = media::render(&s.store, p, copy.clone(), token).await
        {
            let mut error = copy;
            error.status = "error".into();
            error.error = Some(e.to_string());
            error.detail = e.to_string();
            let _ = write_json(&dir.join("job.json"), &error).await;
        }
        s.exports.lock().await.remove(&key);
    });
    Ok((StatusCode::ACCEPTED, Json(job)))
}
async fn read_export(s: &AppState, pid: &str, id: &str) -> Result<ExportJob> {
    let mut j: ExportJob = read_json(&export_path(&s.store, pid, id)?.join("job.json")).await?;
    if j.project_id != pid || j.id != id {
        return Err(AppError::bad("Export not found."));
    }
    if ["queued", "rendering"].contains(&j.status.as_str())
        && !s.exports.lock().await.contains_key(&format!("{pid}:{id}"))
    {
        j.status = "error".into();
        j.error = Some("The server stopped during rendering. Start a new export.".into());
    }
    Ok(j)
}
async fn export_job(
    State(s): State<AppState>,
    Path((pid, id)): Path<(String, String)>,
) -> Result<Json<ExportJob>> {
    Ok(Json(read_export(&s, &pid, &id).await?))
}
async fn export_file(
    State(s): State<AppState>,
    Path((pid, id, name)): Path<(String, String, String)>,
    req: axum::extract::Request,
) -> Result<Response> {
    let j = read_export(&s, &pid, &id).await?;
    if j.status != "done"
        || !j.files.contains(&name)
        || ![
            "story.mp4",
            "story.edl",
            "story.fcpxml",
            "story.srt",
            "chapters.txt",
        ]
        .contains(&name.as_str())
    {
        return Err(AppError::bad("That export file is not available."));
    }
    send_file(
        export_path(&s.store, &pid, &id)?.join(&name),
        req,
        Some(&name),
    )
    .await
}
async fn cancel_export(
    State(s): State<AppState>,
    Path((pid, id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    export_path(&s.store, &pid, &id)?;
    let jobs = s.exports.lock().await;
    let t = jobs
        .get(&format!("{pid}:{id}"))
        .ok_or_else(|| AppError::bad("No export is running."))?;
    t.cancel();
    Ok(Json(json!({"cancelled":true})))
}
