//! Native, local camera studio. Device capture is always an explicit user action.
use crate::{Destination, Page, State, api::Api, dom, navigate};
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use reqwest::Method;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use storyfold_core::recording::*;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum CapturePhase {
    #[default]
    Idle,
    Starting,
    Recording,
    Stopping,
}
#[derive(Clone, Debug, Default)]
struct CaptureGesture {
    phase: CapturePhase,
    pressed: Option<Instant>,
    long_take: bool,
    stop_requested: bool,
}
impl CaptureGesture {
    fn start(&mut self, long_take: bool, now: Instant) -> bool {
        if self.phase != CapturePhase::Idle {
            return false;
        }
        self.phase = CapturePhase::Starting;
        self.pressed = (!long_take).then_some(now);
        self.long_take = long_take;
        self.stop_requested = false;
        true
    }
    fn release(&mut self, elapsed: Duration, cancelled: bool) -> bool {
        if self.pressed.take().is_none() && !cancelled {
            return false;
        }
        if cancelled || elapsed >= Duration::from_millis(350) {
            self.request_stop()
        } else {
            self.long_take = true;
            false
        }
    }
    fn request_stop(&mut self) -> bool {
        self.pressed = None;
        self.stop_requested = true;
        if self.phase == CapturePhase::Recording {
            self.phase = CapturePhase::Stopping;
            true
        } else {
            false
        }
    }
    fn started(&mut self) -> bool {
        self.phase = CapturePhase::Recording;
        if self.stop_requested {
            self.phase = CapturePhase::Stopping;
            true
        } else {
            false
        }
    }
    fn busy(&self) -> bool {
        self.phase != CapturePhase::Idle
    }
}

#[derive(Clone, Default)]
struct Studio {
    sessions: Vec<RecordingSession>,
    devices: RecordingDevices,
    loaded: bool,
    session: Option<RecordingSession>,
    saved: Option<RecordingEdit>,
    selected: String,
    camera: String,
    microphone: String,
    gesture: CaptureGesture,
    request: bool,
    error: String,
    preview_export: bool,
    sequence_status: String,
    sequence_error: String,
    split_at: f64,
    manual: bool,
    epoch: u64,
    external_job: bool,
    discovering: bool,
}
fn edit(session: &RecordingSession) -> RecordingEdit {
    RecordingEdit {
        revision: session.revision,
        name: session.name.clone(),
        orientation: session.orientation.clone(),
        clips: session.clips.clone(),
        overlays: session.overlays.clone(),
    }
}
impl Studio {
    fn update_devices(&mut self, devices: RecordingDevices, initial: bool) {
        if self.capture() {
            return;
        }
        if !devices.cameras.iter().any(|d| d.id == self.camera) {
            self.camera = devices
                .cameras
                .first()
                .map(|d| d.id.clone())
                .unwrap_or_default();
        }
        if initial {
            self.microphone = devices
                .microphones
                .first()
                .map(|d| d.id.clone())
                .unwrap_or_default();
        } else if !self.microphone.is_empty()
            && !devices.microphones.iter().any(|d| d.id == self.microphone)
        {
            self.microphone.clear();
        }
        self.devices = devices;
        if self.error.starts_with("Camera discovery:") {
            self.error.clear();
        }
    }
    fn dirty(&self) -> bool {
        self.session.as_ref().map(edit) != self.saved
    }
    fn capture(&self) -> bool {
        self.gesture.busy() || self.session.as_ref().is_some_and(|s| s.active.is_some())
    }
    fn exporting(&self) -> bool {
        self.session.as_ref().is_some_and(|s| {
            s.export
                .as_ref()
                .is_some_and(|job| job.status == "rendering")
        })
    }
    fn locked(&self) -> bool {
        self.request || self.capture() || self.exporting() || self.external_job
    }
    fn accept(&mut self, session: RecordingSession) {
        self.epoch = self.epoch.wrapping_add(1);
        if let Some(summary) = self.sessions.iter_mut().find(|s| s.id == session.id) {
            *summary = session.clone();
        } else {
            self.sessions.insert(0, session.clone());
        }
        self.saved = Some(edit(&session));
        self.session = Some(session);
        self.error.clear();
        if !self
            .session
            .as_ref()
            .is_some_and(|s| s.clips.iter().any(|c| c.id == self.selected))
        {
            self.select_last();
        }
    }
    fn select_last(&mut self) {
        let clip = self.session.as_ref().and_then(|s| s.clips.last()).cloned();
        if let Some(clip) = clip {
            self.select(&clip);
        } else {
            self.selected.clear();
        }
    }
    fn select(&mut self, clip: &RecordingClip) {
        self.selected = clip.id.clone();
        self.split_at = (clip.r#in + clip.out) / 2.;
        self.preview_export = false;
    }
    fn selected_clip(&self) -> Option<RecordingClip> {
        self.session
            .as_ref()?
            .clips
            .iter()
            .find(|c| c.id == self.selected)
            .cloned()
    }
    fn can_reconcile(&self, epoch: u64, remote: &RecordingSession) -> bool {
        self.epoch == epoch
            && !self.request
            && !matches!(
                self.gesture.phase,
                CapturePhase::Starting | CapturePhase::Stopping
            )
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.id == remote.id && remote.revision >= s.revision)
    }
}

fn change(mut studio: Signal<Studio>, f: impl FnOnce(&mut RecordingSession)) {
    if !studio.peek().locked() {
        let mut s = studio.write();
        if let Some(session) = s.session.as_mut() {
            f(session);
            s.epoch = s.epoch.wrapping_add(1);
            s.preview_export = false;
            s.sequence_status.clear();
            s.sequence_error.clear();
        }
    }
}
fn fmt(seconds: f64) -> String {
    let tenths = (seconds.max(0.) * 10.).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}
fn media_url(api: &Api, id: &str, suffix: &str) -> String {
    api.url(&format!("/recordings/{id}/media/{suffix}"))
}
fn preview_url(api: &Api, id: &str) -> String {
    media_url(api, id, "preview.mjpeg")
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SequenceAction {
    Preview,
    Export,
}

fn current_export(studio: &Studio) -> Option<&RecordingExport> {
    if studio.dirty() {
        return None;
    }
    let session = studio.session.as_ref()?;
    session
        .export
        .as_ref()
        .filter(|job| job.status == "done" && job.revision == session.revision)
}

fn sequence_unavailable(studio: &Studio) -> Option<&'static str> {
    if studio.capture() {
        Some("Stop and save your recording before previewing or exporting.")
    } else if studio.session.as_ref().is_none_or(|s| s.clips.is_empty()) {
        Some("Record or import a take to preview and export your sequence.")
    } else if studio.external_job {
        Some("Wait for the YouTube transfer to finish before previewing or exporting.")
    } else {
        None
    }
}

async fn persist(mut studio: Signal<Studio>, api: &Api) -> Result<(), String> {
    if !studio.peek().dirty() {
        return Ok(());
    }
    let session = studio
        .peek()
        .session
        .clone()
        .ok_or("Create a recording first.")?;
    let next: RecordingSession = api
        .send(
            Method::PATCH,
            &format!("/recordings/{}", session.id),
            Some(
                serde_json::to_value(edit(&session))
                    .map_err(|_| "Could not save the recording draft.")?,
            ),
        )
        .await?;
    studio.write().accept(next);
    Ok(())
}
fn save(mut studio: Signal<Studio>, api: Api) {
    if studio.peek().locked() {
        return;
    }
    studio.write().request = true;
    spawn(async move {
        let result = persist(studio, &api).await;
        studio.write().request = false;
        if let Err(error) = result {
            studio.write().error = error;
        }
    });
}
fn create(mut studio: Signal<Studio>, api: Api) {
    if studio.peek().locked() {
        return;
    }
    if studio.peek().dirty() {
        studio.write().error =
            "Save or reload your studio changes before starting another recording.".into();
        return;
    }
    studio.write().request = true;
    spawn(async move {
        let result = api
            .send::<RecordingSession>(
                Method::POST,
                "/recordings",
                Some(json!({"name":"Untitled recording","orientation":"16:9"})),
            )
            .await;
        studio.write().request = false;
        match result {
            Ok(session) => {
                studio.write().accept(session);
            }
            Err(error) => studio.write().error = error,
        }
    });
}
fn open_session(mut studio: Signal<Studio>, api: Api, id: String) {
    if studio.peek().locked() || id.is_empty() {
        return;
    }
    if studio.peek().dirty() {
        studio.write().error =
            "Save or reload your studio changes before opening another recording.".into();
        return;
    }
    studio.write().request = true;
    spawn(async move {
        let result = api
            .get::<RecordingSession>(&format!("/recordings/{id}"))
            .await;
        studio.write().request = false;
        match result {
            Ok(session) => studio.write().accept(session),
            Err(error) => studio.write().error = error,
        }
    });
}
fn refresh_devices(mut studio: Signal<Studio>, api: Api) {
    if studio.peek().capture() || studio.peek().discovering {
        return;
    }
    studio.write().discovering = true;
    spawn(async move {
        let result = api.get::<RecordingDevices>("/recording-devices").await;
        let mut s = studio.write();
        s.discovering = false;
        if s.capture() {
            return;
        }
        match result {
            Ok(devices) => {
                s.update_devices(devices, false);
            }
            Err(error) => s.error = format!("Camera discovery: {error}"),
        }
    });
}

fn reload_import(mut studio: Signal<Studio>, api: Api) {
    if studio.peek().capture() || studio.peek().dirty() {
        return;
    }
    let Some(id) = studio.peek().session.as_ref().map(|s| s.id.clone()) else {
        return;
    };
    studio.write().request = true;
    spawn(async move {
        let result = api
            .get::<RecordingSession>(&format!("/recordings/{id}"))
            .await;
        let mut s = studio.write();
        s.request = false;
        match result {
            Ok(session) => {
                s.accept(session);
                s.select_last();
            }
            Err(error) => s.error = error,
        }
    });
}

fn stop(mut studio: Signal<Studio>, mut state: Signal<State>, api: Api) {
    let Some(id) = studio.peek().session.as_ref().map(|s| s.id.clone()) else {
        return;
    };
    state.write().recording_capture = true;
    studio.write().gesture.phase = CapturePhase::Stopping;
    spawn(async move {
        let result = api
            .send::<RecordingSession>(
                Method::POST,
                &format!("/recordings/{id}/stop"),
                Some(json!({})),
            )
            .await;
        match result {
            Ok(session) => {
                let mut s = studio.write();
                s.gesture = CaptureGesture::default();
                s.accept(session);
                s.select_last();
                state.write().recording_capture = s.capture();
            }
            Err(error) => {
                // A transport timeout cannot prove the hardware stopped. The poll loop
                // reconciles state; keep the navigation guard until the service confirms it.
                studio.write().error = error;
                studio.write().gesture.phase = CapturePhase::Recording;
            }
        }
    });
}
fn start(mut studio: Signal<Studio>, mut state: Signal<State>, api: Api, long_take: bool) {
    if studio.peek().locked() || studio.peek().camera.is_empty() || studio.peek().session.is_none()
    {
        return;
    }
    if !studio.write().gesture.start(long_take, Instant::now()) {
        return;
    }
    state.write().recording_capture = true;
    spawn(async move {
        let saved = persist(studio, &api).await;
        if let Err(error) = saved {
            studio.write().gesture = CaptureGesture::default();
            studio.write().error = error;
            state.write().recording_capture = false;
            return;
        }
        let (id, camera, microphone) = {
            let s = studio.peek();
            (
                s.session.as_ref().unwrap().id.clone(),
                s.camera.clone(),
                (!s.microphone.is_empty()).then(|| s.microphone.clone()),
            )
        };
        let result = api
            .send::<RecordingSession>(
                Method::POST,
                &format!("/recordings/{id}/start"),
                Some(json!({"cameraId":camera,"microphoneId":microphone})),
            )
            .await;
        match result {
            Ok(session) => {
                studio.write().accept(session);
                if studio.write().gesture.started() {
                    stop(studio, state, api);
                }
            }
            Err(error) => {
                // Reconcile even a lost HTTP response before releasing the guard.
                match api
                    .get::<RecordingSession>(&format!("/recordings/{id}"))
                    .await
                {
                    Ok(session) if session.active.is_some() => {
                        studio.write().accept(session);
                        studio.write().gesture.phase = CapturePhase::Recording;
                        studio.write().gesture.stop_requested = true;
                        stop(studio, state, api);
                    }
                    Ok(session) => {
                        studio.write().accept(session);
                        studio.write().gesture = CaptureGesture::default();
                        state.write().recording_capture = false;
                    }
                    Err(_) => {
                        studio.write().gesture.phase = CapturePhase::Recording;
                    }
                }
                studio.write().error = error;
            }
        }
    });
}
fn release(mut studio: Signal<Studio>, state: Signal<State>, api: Api, cancelled: bool) {
    let elapsed = studio
        .peek()
        .gesture
        .pressed
        .map(|t| t.elapsed())
        .unwrap_or_default();
    let stop_now = studio.write().gesture.release(elapsed, cancelled);
    if stop_now {
        stop(studio, state, api);
    }
}
fn explicit_stop(mut studio: Signal<Studio>, state: Signal<State>, api: Api) {
    if studio.write().gesture.request_stop() {
        stop(studio, state, api);
    }
}
fn overlay(mut studio: Signal<Studio>, api: Api, id: Option<String>) {
    if studio.peek().request
        || studio.peek().external_job
        || studio.peek().exporting()
        || matches!(
            studio.peek().gesture.phase,
            CapturePhase::Starting | CapturePhase::Stopping
        )
    {
        return;
    }
    let Some(session) = studio.peek().session.clone() else {
        return;
    };
    // Save prepared card text before adding its first timing mark.
    studio.write().request = true;
    spawn(async move {
        let result = async {
            persist(studio, &api).await?;
            api.send::<RecordingSession>(
                Method::POST,
                &format!("/recordings/{}/overlay", session.id),
                Some(json!({"overlayId":id})),
            )
            .await
        }
        .await;
        studio.write().request = false;
        match result {
            Ok(session) => studio.write().accept(session),
            Err(error) => studio.write().error = error,
        }
    });
}
async fn export_destination() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if std::env::var_os("STORYFOLD_RECORD_SMOKE_FILE").is_some()
        && let Some(path) = std::env::var_os("STORYFOLD_RECORD_EXPORT_PATH")
    {
        return Some(PathBuf::from(path));
    }
    rfd::AsyncFileDialog::new()
        .set_title("Export sequence as MP4")
        .add_filter("MP4 video", &["mp4"])
        .set_file_name("storyfold-recording.mp4")
        .save_file()
        .await
        .map(|file| file.path().to_path_buf())
}

async fn prepare_current_sequence(
    mut studio: Signal<Studio>,
    api: &Api,
) -> Result<RecordingExport, String> {
    persist(studio, api).await?;
    if let Some(job) = current_export(&studio.peek()).cloned() {
        return Ok(job);
    }
    let session = studio
        .peek()
        .session
        .clone()
        .ok_or("Create a recording first.")?;
    let id = session.id;
    let revision = session.revision;
    studio.write().sequence_status = "Preparing your sequence…".into();
    let mut next = api
        .send::<RecordingSession>(
            Method::POST,
            &format!("/recordings/{id}/export"),
            Some(json!({})),
        )
        .await?;
    loop {
        let job = next
            .export
            .clone()
            .ok_or("The service did not start preparing your sequence.")?;
        if next.revision != revision || job.revision != revision {
            return Err(
                "Your sequence changed in another window. Reload it before trying again.".into(),
            );
        }
        studio.write().accept(next);
        match job.status.as_str() {
            "done" => return Ok(job),
            "error" => {
                return Err(job
                    .error
                    .unwrap_or_else(|| "Could not prepare this sequence. Try again.".into()));
            }
            "rendering" => {}
            _ => return Err("Sequence preparation stopped. Try again.".into()),
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        next = api.get(&format!("/recordings/{id}")).await?;
    }
}

async fn play_sequence(id: String) {
    // Transport only: all revision, preparation and selection decisions are Rust.
    for _ in 0..40 {
        let result=dom::eval("const id=await dioxus.recv();const studio=document.querySelector('.record-studio');if(!studio||studio.closest('[hidden]'))return true;const v=document.getElementById('record-sequence-preview');if(!v||!v.src.includes('/exports/'+id))return false;try{v.currentTime=0;v.play().catch(()=>{});}catch(_){}return true;",Some(json!(id))).await;
        if result.ok() == Some(json!(true)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn sequence_action(mut studio: Signal<Studio>, api: Api, action: SequenceAction) {
    let unavailable = sequence_unavailable(&studio.peek());
    if let Some(reason) = unavailable {
        studio.write().sequence_error = reason.into();
        return;
    }
    if studio.peek().locked() {
        return;
    }
    studio.write().request = true;
    studio.write().preview_export = false;
    studio.write().sequence_error.clear();
    studio.write().sequence_status = if action == SequenceAction::Export {
        "Choose where to save your MP4…"
    } else {
        "Preparing your sequence…"
    }
    .into();
    spawn(async move {
        let destination = if action == SequenceAction::Export {
            let Some(path) = export_destination().await else {
                let mut s = studio.write();
                s.request = false;
                s.sequence_status.clear();
                return;
            };
            Some(path)
        } else {
            None
        };
        let result = async {
            let job = prepare_current_sequence(studio, &api).await?;
            studio.write().preview_export = true;
            if let Some(destination) = destination {
                studio.write().sequence_status = "Saving your MP4…".into();
                let id = studio
                    .peek()
                    .session
                    .as_ref()
                    .ok_or("This recording is no longer open.")?
                    .id
                    .clone();
                api.download(
                    &format!("/recordings/{id}/media/exports/{}", job.id),
                    &destination,
                )
                .await?;
                studio.write().sequence_status =
                    "MP4 exported. Your full sequence is ready to preview.".into();
            } else {
                studio.write().sequence_status =
                    "Your full sequence is ready. Use the player controls to watch it.".into();
                play_sequence(job.id).await;
            }
            Ok::<_, String>(())
        }
        .await;
        studio.write().request = false;
        if let Err(error) = result {
            let mut s = studio.write();
            s.sequence_status.clear();
            s.sequence_error = error;
        }
    });
}

fn sequence_actions(studio: Signal<Studio>, api: Api, viewer: bool) -> Element {
    let s = studio.read().clone();
    let unavailable = sequence_unavailable(&s);
    let blocked = s.locked() || unavailable.is_some();
    let prefix = if viewer { "record" } else { "record-bottom" };
    let preview_api = api.clone();
    rsx! {
        div {class:"record-sequence-actions",
            div {class:"actions",
                button {id:"{prefix}-preview-sequence",class:"primary",disabled:blocked,onclick:move |_|sequence_action(studio,preview_api.clone(),SequenceAction::Preview),"▶ Preview sequence"}
                button {id:"{prefix}-export-mp4",disabled:blocked,onclick:move |_|sequence_action(studio,api.clone(),SequenceAction::Export),"Export MP4…"}
            }
            if let Some(reason)=unavailable {p {class:"hint",role:"status","{reason}"}}
            if let Some(job)=s.session.as_ref().and_then(|s|s.export.as_ref()).filter(|j|j.status=="rendering") {
                progress {max:100,value:job.pct,"aria-label":"Preparing sequence progress"}
                p {class:"hint",role:"status","Preparing your sequence · {job.pct:.0}%"}
            } else if !s.sequence_status.is_empty() {p {class:"hint",role:"status","{s.sequence_status}"}}
            if !s.sequence_error.is_empty(){p {class:"error",role:"alert","{s.sequence_error}"}}
        }
    }
}

fn send_to_editor(mut studio: Signal<Studio>, mut state: Signal<State>, api: Api) {
    if studio.peek().locked() || studio.peek().dirty() {
        return;
    }
    let Some(id) = studio.peek().session.as_ref().map(|s| s.id.clone()) else {
        return;
    };
    let manual = studio.peek().manual;
    studio.write().request = true;
    spawn(async move {
        let result = api
            .send::<Value>(
                Method::POST,
                &format!("/recordings/{id}/editor"),
                Some(json!({"manual":manual})),
            )
            .await;
        studio.write().request = false;
        match result {
            Ok(value) => {
                if let Some(id) = value["projectId"].as_str() {
                    navigate(state, Destination::Project(id.to_owned()));
                } else {
                    state.write().error =
                        "The local service did not return the interview it created.".into();
                }
            }
            Err(error) => studio.write().error = error,
        }
    });
}

fn move_clip(session: &mut RecordingSession, selected: &str, delta: isize) {
    if let Some(index) = session.clips.iter().position(|c| c.id == selected) {
        let next = index as isize + delta;
        if next >= 0 && next < session.clips.len() as isize {
            session.clips.swap(index, next as usize);
        }
    }
}
fn split_clip(session: &mut RecordingSession, selected: &str, at: f64) -> bool {
    let Some(index) = session.clips.iter().position(|c| c.id == selected) else {
        return false;
    };
    let clip = &mut session.clips[index];
    if !at.is_finite() || at < clip.r#in + 0.12 || at > clip.out - 0.12 {
        return false;
    }
    let mut second = clip.clone();
    second.id = Uuid::new_v4().to_string();
    second.name = format!("{} (b)", second.name);
    second.r#in = at;
    clip.out = at;
    session.clips.insert(index + 1, second);
    true
}
fn trim_clip(session: &mut RecordingSession, selected: &str, value: f64, start: bool) {
    if !value.is_finite() {
        return;
    }
    let Some(clip) = session.clips.iter_mut().find(|c| c.id == selected) else {
        return;
    };
    let duration = session
        .takes
        .iter()
        .find(|t| t.id == clip.take_id)
        .map(|t| t.duration)
        .unwrap_or(clip.out);
    let minimum = 0.12_f64.min(duration);
    if start {
        clip.r#in = value.clamp(0., (clip.out - minimum).max(0.));
    } else {
        clip.out = value.clamp((clip.r#in + minimum).min(duration), duration);
    }
}
fn add_overlay(session: &mut RecordingSession) {
    if session.overlays.len() < OVERLAY_SLOTS {
        session.overlays.push(RecordingOverlay {
            id: Uuid::new_v4().to_string(),
            label: format!("Card {}", session.overlays.len() + 1),
            kind: "lowerThird".into(),
            text: "Your words here".into(),
            secondary: String::new(),
        });
    }
}

#[component]
pub fn RecordingStudio(mut state: Signal<State>, active: bool) -> Element {
    let api = use_context::<Api>();
    let mut studio = use_signal(Studio::default);
    let poll_api = api.clone();
    use_future(move || {
        let api = poll_api.clone();
        async move {
            let mut was_visible = false;
            let mut next_discovery = Instant::now();
            loop {
                let visible = state.peek().page == Page::Record;
                if visible
                    && studio.peek().loaded
                    && !studio.peek().capture()
                    && (!was_visible || Instant::now() >= next_discovery)
                {
                    refresh_devices(studio, api.clone());
                    next_discovery = Instant::now() + Duration::from_secs(3);
                }
                if visible || studio.peek().capture() {
                    if !studio.peek().loaded {
                        let devices = api.get::<RecordingDevices>("/recording-devices").await;
                        let sessions = api.get::<Vec<RecordingSession>>("/recordings").await;
                        match (devices, sessions) {
                            (Ok(devices), Ok(sessions)) => {
                                let mut s = studio.write();
                                // Discovery reads capabilities; Record is the explicit capture action.
                                s.update_devices(devices, true);
                                next_discovery = Instant::now() + Duration::from_secs(3);
                                s.sessions = sessions;
                                if !s.request
                                    && s.session.is_none()
                                    && let Some(session) = s.sessions.first().cloned()
                                {
                                    if session.active.is_some() {
                                        s.gesture.phase = CapturePhase::Recording;
                                    }
                                    s.accept(session);
                                }
                                s.loaded = true;
                            }
                            (Err(error), _) | (_, Err(error)) => studio.write().error = error,
                        }
                    }
                    let current = studio.peek().session.as_ref().map(|s| s.id.clone());
                    if let Some(id) = current {
                        let epoch = studio.peek().epoch;
                        let result = api
                            .get::<RecordingSession>(&format!("/recordings/{id}"))
                            .await;
                        if let Ok(session) = result {
                            let mut s = studio.write();
                            if s.can_reconcile(epoch, &session) {
                                if !s.dirty() {
                                    let active = session.active.is_some();
                                    let error = s.error.clone();
                                    s.accept(session);
                                    if !error
                                        .starts_with("Cannot reach the local Storyfold service.")
                                    {
                                        s.error = error;
                                    }
                                    if !active {
                                        s.gesture = CaptureGesture::default();
                                    } else if s.gesture.phase == CapturePhase::Idle {
                                        s.gesture.phase = CapturePhase::Recording;
                                    }
                                } else if let Some(local) = s.session.as_mut() {
                                    // An external change never replaces this window's draft.
                                    local.active = session.active;
                                    local.export = session.export;
                                    local.error = session.error;
                                }
                            }
                        } else if let Err(error) = result {
                            studio.write().error = error;
                        }
                    }
                }
                was_visible = visible;
                tokio::time::sleep(Duration::from_millis(if studio.peek().capture() {
                    500
                } else {
                    1000
                }))
                .await;
            }
        }
    });
    use_effect(move || {
        let s = studio.read();
        let dirty = s.dirty();
        let capture = s.capture();
        if state.peek().recording_dirty != dirty || state.peek().recording_capture != capture {
            let mut app = state.write();
            app.recording_dirty = dirty;
            app.recording_capture = capture;
        }
    });
    use_effect(move || {
        // Plain props do not subscribe a Dioxus effect. Track the page signal even
        // on the initial hidden mount, so later navigation reliably pauses media.
        let current_page = state.read().page;
        let visible = active && current_page == Page::Record;
        // This bridge only manages DOM pointer capture and media pause. Capture
        // decisions, devices, timing, and clip state remain entirely in Rust.
        spawn(async move {
            let _ = dom::eval(r#"
                    const visible=await dioxus.recv();
                    const pause=()=>document.querySelectorAll('.record-studio video').forEach(v=>v.pause());
                    if (!window.storyfoldRecordPointer) {
                        window.storyfoldRecordPointer=true;
                        document.addEventListener('pointerdown', e=>{
                            if(e.target.closest?.('.record-studio') && !e.target.closest?.('video')) pause();
                            const b=e.target.closest?.('#record-gesture');
                            if(b && !b.disabled && e.isPrimary && e.button===0) { try { b.setPointerCapture(e.pointerId); } catch (_) {} }
                        },true);
                        document.addEventListener('click',e=>{
                            if(e.target.closest?.('.record-studio') && !e.target.closest?.('video')) pause();
                        },true);
                        document.addEventListener('input',e=>{
                            if(e.target.closest?.('.record-studio') && !e.target.closest?.('video')) pause();
                        },true);
                        document.addEventListener('keydown',e=>{
                            if((e.key==='Enter'||e.key===' ') && e.target.closest?.('#record-gesture')) pause();
                        },true);
                    }
                    if(!visible) pause();
                    return true;
                "#, Some(json!(visible))).await;
        });
    });
    studio_view(state, studio, api)
}

fn studio_view(state: Signal<State>, mut studio: Signal<Studio>, api: Api) -> Element {
    let s = studio.read().clone();
    let session = s.session.clone();
    let locked = s.locked();
    let capture = s.capture();
    let selected = s.selected_clip();
    let fresh_export = current_export(&s).cloned();
    let gesture_api = api.clone();
    let release_api = api.clone();
    let cancel_api = api.clone();
    let shortcut_api = api.clone();
    rsx! {
        style {{include_str!("recording.css")}}
        section {class:"record-studio","aria-label":"Recording studio",
            onpointerup:move |_|release(studio,state,release_api.clone(),false),
            onpointercancel:move |_|release(studio,state,cancel_api.clone(),true),
            onkeydown:move |e| {
                if e.is_auto_repeating() || e.modifiers().ctrl() || e.modifiers().meta() || e.modifiers().alt() {return;}
                if let Key::Character(key) = e.key()
                    && let Ok(index) = key.parse::<usize>()
                    && (1..=OVERLAY_SLOTS).contains(&index)
                {
                    let session = studio.peek().session.clone();
                    if let Some(session) = session
                        && let Some(card) = session.overlays.get(index-1)
                    {
                        e.prevent_default();
                        let next = (session.active_overlay.as_deref()!=Some(&card.id)).then(||card.id.clone());
                        overlay(studio,shortcut_api.clone(),next);
                    }
                }
            },
            div {class:"record-heading",
                div {span {class:"eyebrow","YOUR CAMERA. YOUR WORDS."}h1 {"A space to record."}}
                div {class:"record-session-picker",
                    label {r#for:"record-session","Saved recordings"}
                    select {id:"record-session",disabled:locked,value:session.as_ref().map(|s|s.id.clone()).unwrap_or_default(),onkeydown:move |e|e.stop_propagation(),onchange:{let api=api.clone();move |e|open_session(studio,api.clone(),e.value())},
                        option {value:"","Choose a recording"}
                        for item in &s.sessions {option {value:"{item.id}","{item.name}"}}
                    }
                    button {disabled:locked,onclick:{let api=api.clone();move |_|create(studio,api.clone())},"New recording"}
                }
            }
            if !s.error.is_empty() {div {class:"record-alert",role:"alert",span {"{s.error}"}button {"aria-label":"Dismiss studio error",onclick:move |_|studio.write().error.clear(),"×"}}}
            if let Some(session) = &session {
                div {class:"record-toolbar",
                    label {class:"record-name",r#for:"record-name","Recording name" input {id:"record-name",maxlength:120,disabled:locked,value:"{session.name}",onkeydown:move |e|e.stop_propagation(),oninput:move |e|change(studio,|s|s.name=e.value())}}
                    div {class:"record-orientation",role:"group","aria-label":"Video shape",
                        for (value,label) in [("16:9","Horizontal 16:9"),("9:16","Vertical 9:16")] {
                            button {disabled:locked,class:if session.orientation==value {"active"}else{""},"aria-pressed":session.orientation==value,onclick:move |_|change(studio,|s|s.orientation=value.into()),"{label}"}
                        }
                    }
                    span {class:if s.dirty(){"save-status dirty"}else{"save-status"},role:"status",if s.request {"Working…"}else if s.dirty(){"Unsaved studio changes"}else{"Saved on this computer"}}
                    button {disabled:locked||!s.dirty(),onclick:{let api=api.clone();move |_|save(studio,api.clone())},"Save changes"}
                    button {disabled:locked||!s.dirty(),onclick:move |_|{let mut s=studio.write();if let (Some(saved),Some(session))=(s.saved.clone(),s.session.as_mut()){session.name=saved.name;session.orientation=saved.orientation;session.clips=saved.clips;session.overlays=saved.overlays;}},"Reload saved"}
                }
                div {class:"record-grid",
                    aside {class:"record-bin card","aria-label":"Recorded take bin",
                        div {class:"section-heading",h2 {"Your takes"}span {class:"mono","{session.takes.len()} saved"}}
                        if session.takes.is_empty() {div {class:"record-bin-empty",span {class:"record-empty-icon","◉"}p {"Your first take starts here."}small {"Hold and release to save a short clip, or tap for a longer take."}}}
                        div {class:"record-takes",
                            for (index,take) in session.takes.iter().enumerate() {
                                {
                                    let take=take.clone();let path=media_url(&api,&session.id,&format!("takes/{}",take.id));
                                    let existing=session.clips.iter().find(|c|c.take_id==take.id).cloned();
                                    rsx! {div {class:"record-take",
                                        button {disabled:locked,class:"record-take-pick",onclick:move |_|{if let Some(clip)=existing.clone(){studio.write().select(&clip);}else{let clip=RecordingClip{id:Uuid::new_v4().to_string(),take_id:take.id.clone(),name:take.name.clone(),r#in:0.,out:take.duration};change(studio,|s|s.clips.push(clip.clone()));studio.write().select(&clip);}},
                                            div {class:"record-take-thumb",video {src:"{path}",preload:"metadata",muted:true,"aria-hidden":"true"}span {"{index+1}"}small {"{fmt(take.duration)}"}}
                                            strong {"{take.name}"}
                                        }
                                        if take.interrupted {small {class:"error","Recovered interrupted take"}}
                                    }}
                                }
                            }
                        }
                        p {class:"hint","Original takes stay intact. Select a take to preview it or add it back to your sequence."}
                    }
                    div {class:"record-camera-column",
                        div {class:"record-device-row",
                            label {r#for:"record-camera","Camera" select {id:"record-camera",disabled:capture||s.request,value:"{s.camera}",onkeydown:move |e|e.stop_propagation(),onchange:move |e|studio.write().camera=e.value(),
                                if s.devices.cameras.is_empty() {option {value:"","No camera connected"}}
                                for device in &s.devices.cameras {option {value:"{device.id}","{device.name}"}}
                            }}
                            label {r#for:"record-microphone","Microphone" select {id:"record-microphone",disabled:capture||s.request,value:"{s.microphone}",onkeydown:move |e|e.stop_propagation(),onchange:move |e|studio.write().microphone=e.value(),
                                option {value:"","Off — record without microphone"}
                                for device in &s.devices.microphones {option {value:"{device.id}","{device.name}"}}
                            }}
                            button {disabled:capture||s.discovering,onclick:{let api=api.clone();move |_|refresh_devices(studio,api.clone())},if s.discovering {"Checking cameras…"}else{"Refresh devices"}}
                        }
                        div {class:"record-camera-card",
                            div {class:if session.orientation=="9:16" {"record-frame portrait"}else{"record-frame"},
                                if session.active.is_some() {
                                    if let Some(active)=&session.active {img {key:"{active.take_id}",class:"record-live-image",src:"{preview_url(&api,&session.id)}",alt:"Live camera preview while recording"}}
                                } else if s.preview_export && fresh_export.is_some() {
                                    if let Some(job)=&fresh_export {video {key:"export-{job.id}",id:"record-sequence-preview",class:"record-clip-preview",src:media_url(&api,&session.id,&format!("exports/{}",job.id)),controls:true,preload:"auto","aria-label":"Full sequence preview"}}
                                } else if let Some(clip)=&selected {
                                    video {key:"clip-{clip.id}-{clip.r#in}-{clip.out}",class:"record-clip-preview",src:format!("{}#t={},{}",media_url(&api,&session.id,&format!("takes/{}",clip.take_id)),clip.r#in,clip.out),controls:true,preload:"metadata","aria-label":"Selected source take preview"}
                                } else {
                                    div {class:"record-camera-empty",span {class:"record-empty-icon","◉"}strong {if s.devices.cameras.is_empty(){"Connect a camera to begin."}else{"Ready when you are."}}p {if s.devices.cameras.is_empty(){"Connect a webcam, refresh devices, then choose your camera and microphone."}else{"Your camera and microphone open only when you press Record. Your video stays on this computer."}}}
                                }
                                if capture {span {class:"record-live-badge",role:"status",span {class:"record-dot"}match s.gesture.phase {CapturePhase::Starting=>"Opening camera…".to_string(),CapturePhase::Stopping=>"Saving take…".to_string(),_=>format!("REC {} · {}",fmt(session.active.as_ref().map(|a|a.seconds).unwrap_or_default()),if s.gesture.long_take{"long take"}else{"hold"})}}}
                                if capture || (selected.is_none()&&!s.preview_export) {
                                    if let Some(card)=session.overlays.iter().find(|o|Some(&o.id)==session.active_overlay.as_ref()) {
                                        div {class:format!("record-overlay-preview {}",card.kind),strong {"{card.text}"}if !card.secondary.is_empty(){span {"{card.secondary}"}}}
                                    }
                                }
                            }
                        }
                        {sequence_actions(studio,api.clone(),true)}
                        if s.preview_export && fresh_export.is_some(){p {class:"hint record-preview-label","Full sequence preview · {session.clips.len()} clips · {fmt(session.clips.iter().map(|c|(c.out-c.r#in).max(0.)).sum())}"}}
                        if !s.devices.message.is_empty(){p {class:"hint record-device-message","{s.devices.message}"}}
                        div {class:"record-controls",
                            button {id:"record-gesture",class:if capture {"record-trigger recording"}else{"record-trigger"},disabled:s.request||s.external_job||s.exporting()||s.camera.is_empty()||s.gesture.phase==CapturePhase::Stopping,"aria-label":if capture{"Stop recording"}else{"Record: hold for clip or tap for long take"},
                                onpointerdown:move |e| {
                                    if e.is_primary() && e.trigger_button()==Some(MouseButton::Primary) {
                                        e.prevent_default();
                                        if studio.peek().gesture.long_take && studio.peek().capture(){explicit_stop(studio,state,gesture_api.clone());}
                                        else{start(studio,state,gesture_api.clone(),false);}
                                    }
                                },
                                onkeydown:{let api=api.clone();move |e|if !e.is_auto_repeating() && (e.key()==Key::Enter||e.key()==Key::Character(" ".into())){e.prevent_default();if studio.peek().capture(){explicit_stop(studio,state,api.clone());}else{start(studio,state,api.clone(),true);}}},
                                span {class:if capture {"record-trigger-stop"}else{"record-trigger-dot"}}
                            }
                            div {strong {if s.gesture.phase==CapturePhase::Starting {"Opening your camera"}else if s.gesture.phase==CapturePhase::Stopping {"Saving your take"}else if capture&&s.gesture.long_take {"Tap to stop and save"}else if capture {"Release to save your clip"}else {"Hold = clip · Tap = long take"}}
                                p {class:"hint",if capture {"Recording is active. Stop and save before leaving this section."}else{"Use the button or the accessible start and stop controls."}}
                                div {class:"actions",
                                    button {id:"record-explicit-start",disabled:locked||s.camera.is_empty(),onclick:{let api=api.clone();move |_|start(studio,state,api.clone(),true)},"Start long take"}
                                    button {id:"record-explicit-stop",disabled:!capture||s.gesture.phase==CapturePhase::Stopping,onclick:{let api=api.clone();move |_|explicit_stop(studio,state,api.clone())},"Stop and save"}
                                }
                            }
                        }
                        if selected.is_some()&&!capture&&!s.preview_export {p {class:"hint","This is one source take. Choose Preview sequence to watch your trimmed clips and on-screen cards together."}}
                    }
                    aside {class:"record-overlays card","aria-label":"Manual text overlays",
                        div {class:"section-heading",h2 {"On-screen cards"}span {class:"mono","1–9"}}
                        p {class:"hint","Prepare your words, then tap a card or press its number while recording. Tap it again to hide it."}
                        for (index,card) in session.overlays.iter().enumerate() {
                            {let card=card.clone();let id=card.id.clone();let current=session.active_overlay.as_deref()==Some(&card.id);rsx!{
                                div {class:if current{"record-overlay-card active"}else{"record-overlay-card"},
                                    button {class:"record-overlay-show",disabled:s.request||s.external_job||s.exporting()||matches!(s.gesture.phase,CapturePhase::Starting|CapturePhase::Stopping),"aria-pressed":current,"aria-keyshortcuts":format!("{}",index+1),onclick:{let api=api.clone();let id=id.clone();move |_|overlay(studio,api.clone(),(!current).then(||id.clone()))},
                                        kbd {"{index+1}"}span {strong {"{card.label}"}small {match card.kind.as_str(){"quote"=>"Quote","list"=>"List","stat"=>"Statistic",_=>"Lower third"}}}span {class:"record-card-status",if current{"LIVE"}else{"SHOW"}}
                                    }
                                    details {class:"record-card-editor",summary {"Edit card"}
                                        label {"Label" input {disabled:locked,maxlength:60,value:"{card.label}",onkeydown:move |e|e.stop_propagation(),oninput:{let id=id.clone();move |e|change(studio,|s|if let Some(c)=s.overlays.iter_mut().find(|c|c.id==id){c.label=e.value();})}}}
                                        label {"Style" select {disabled:locked,value:"{card.kind}",onkeydown:move |e|e.stop_propagation(),onchange:{let id=id.clone();move |e|change(studio,|s|if let Some(c)=s.overlays.iter_mut().find(|c|c.id==id){c.kind=e.value();})},option {value:"lowerThird","Lower third"}option {value:"quote","Quote"}option {value:"list","List"}option {value:"stat","Statistic"}}}
                                        label {"Main text" textarea {disabled:locked,maxlength:600,rows:3,value:"{card.text}",onkeydown:move |e|e.stop_propagation(),oninput:{let id=id.clone();move |e|change(studio,|s|if let Some(c)=s.overlays.iter_mut().find(|c|c.id==id){c.text=e.value();})}}}
                                        label {"Supporting text" textarea {disabled:locked,maxlength:200,rows:2,value:"{card.secondary}",onkeydown:move |e|e.stop_propagation(),oninput:{let id=id.clone();move |e|change(studio,|s|if let Some(c)=s.overlays.iter_mut().find(|c|c.id==id){c.secondary=e.value();})}}}
                                        button {disabled:locked||current,onclick:{let id=id.clone();move |_|change(studio,|s|s.overlays.retain(|c|c.id!=id))},"Remove card"}
                                    }
                                }
                            }}
                        }
                        button {class:"record-add-card",disabled:locked||session.overlays.len()>=OVERLAY_SLOTS,onclick:move |_|change(studio,add_overlay),"+ Add text card"}
                        p {class:"hint","Manual text cards only. Card timing is saved with each original take and included in your export."}
                    }
                }
                section {class:"record-sequence card","aria-label":"Recording sequence",
                    div {class:"section-heading",h2 {"Your sequence"}span {class:"mono","{session.clips.len()} clips · {fmt(session.clips.iter().map(|c|(c.out-c.r#in).max(0.)).sum())} total"}}
                    {sequence_actions(studio,api.clone(),false)}
                    if session.clips.is_empty() {p {class:"hint","Record a take to start your sequence, or select a saved take from the bin."}}
                    div {class:"record-sequence-strip",
                        for (index,clip) in session.clips.iter().enumerate() {
                            {
                                let clip=clip.clone();
                                rsx! {
                                    button {class:if clip.id==s.selected{"record-sequence-clip active"}else{"record-sequence-clip"},disabled:locked,onclick:move |_|studio.write().select(&clip),
                                        span {"{index+1}"}
                                        strong {"{clip.name}"}
                                        small {"{fmt(clip.out-clip.r#in)}"}
                                    }
                                }
                            }
                        }
                    }
                    if let Some(clip)=selected {div {class:"record-clip-tools",
                        strong {"{clip.name}"}
                        button {disabled:locked,onclick:{let id=clip.id.clone();move |_|change(studio,|s|move_clip(s,&id,-1))},"Move left"}
                        button {disabled:locked,onclick:{let id=clip.id.clone();move |_|change(studio,|s|move_clip(s,&id,1))},"Move right"}
                        label {"In (source seconds)" input {r#type:"number",min:0,step:0.04,value:"{clip.r#in:.2}",disabled:locked,onkeydown:move |e|e.stop_propagation(),onchange:{let id=clip.id.clone();move |e|if let Ok(value)=e.value().parse(){change(studio,|s|trim_clip(s,&id,value,true));}}}}
                        label {"Out (source seconds)" input {r#type:"number",min:0,step:0.04,value:"{clip.out:.2}",disabled:locked,onkeydown:move |e|e.stop_propagation(),onchange:{let id=clip.id.clone();move |e|if let Ok(value)=e.value().parse(){change(studio,|s|trim_clip(s,&id,value,false));}}}}
                        label {"Split at (source seconds)" input {id:"record-split-at",r#type:"number",min:clip.r#in,max:clip.out,step:0.04,value:"{s.split_at:.2}",disabled:locked,onkeydown:move |e|e.stop_propagation(),onchange:move |e|if let Ok(value)=e.value().parse(){studio.write().split_at=value;}}}
                        button {disabled:locked,onclick:{let id=clip.id.clone();move |_|{let at=studio.peek().split_at;let mut valid=false;change(studio,|s|valid=split_clip(s,&id,at));if !valid{studio.write().error="Choose a split point at least 0.12 seconds inside this clip.".into();}}},"Split"}
                        button {disabled:locked,onclick:{let id=clip.id.clone();move |_|{change(studio,|s|s.clips.retain(|c|c.id!=id));studio.write().select_last();}},"Remove from sequence"}
                    }}
                }
                section {class:"record-export card","aria-label":"Export recording",
                    div {h2 {"From recording to story"}p {class:"hint","Use Preview sequence or Export MP4 above to prepare your clips and on-screen cards. Then send the finished sequence to Storyfold’s automatic transcript editor."}}
                    if let Some(job)=&session.export {
                        if let Some(error)=&job.error {p {class:"error",role:"alert","{error}"}}
                        if job.status=="done" {
                            if fresh_export.is_none(){p {class:"hint","Your sequence has changed. Preview or export it again to include your latest edits."}}
                            label {class:"check",input {r#type:"checkbox",checked:s.manual,onchange:move |e|studio.write().manual=e.checked()}"Organize topics myself"}
                            div {class:"providers",span {"Speech: {state.read().health.speech_provider}"}span {"Topics: {state.read().health.topic_provider}"}
                                if !s.manual {span {class:"hint","Automatic grouping sends transcript text to the configured topic provider. Video and audio stay local when local Whisper is selected."}}
                                if state.read().health.speech_provider!="Local Whisper" {span {class:"hint","The configured speech provider may receive audio. Check the provider before sending."}}
                            }
                            button {id:"record-send-editor",class:"primary",disabled:locked||s.dirty()||job.revision!=session.revision||!state.read().health.transcription,onclick:{let api=api.clone();move |_|send_to_editor(studio,state,api.clone())},"Send to transcript editor"}
                        }
                    }
                    if let Some(error)=&session.error {p {class:"error",role:"alert","{error}"}}
                }
                {
                    rsx! {
                        crate::youtube::YouTubePanel {
                            key:"{session.id}",state,recording:session.clone(),locked:s.request||capture||s.exporting(),draft_dirty:s.dirty(),
                            on_busy:move |value|{if studio.peek().external_job!=value{studio.write().external_job=value;}},
                            on_imported:{let api=api.clone();move |_|reload_import(studio,api.clone())}
                        }
                    }
                }
            } else {
                section {class:"record-welcome card",span {class:"record-empty-icon","◉"}h2 {"Make room for a fresh take."}p {"Create a recording, choose your camera, and speak in short clips or one long take. Bring on-screen cards into the moment, then shape your sequence."}button {class:"primary",disabled:s.request,onclick:{let api=api.clone();move |_|create(studio,api.clone())},"Create a recording"}p {class:"hint","Camera and microphone access starts only when you press Record."}
                    if s.devices.cameras.is_empty()&&s.loaded{p {class:"hint","No camera is connected. You can prepare your cards now and connect a camera later."}}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hotplug_updates_choices_preserves_selection_off_and_drafts() {
        fn devices(ids: &[&str]) -> RecordingDevices {
            RecordingDevices {
                cameras: ids
                    .iter()
                    .map(|id| RecordingDevice {
                        id: (*id).into(),
                        name: (*id).into(),
                    })
                    .collect(),
                microphones: vec![RecordingDevice {
                    id: "default".into(),
                    name: "Microphone".into(),
                }],
                ..Default::default()
            }
        }
        let mut studio = Studio::default();
        studio.update_devices(devices(&[]), true);
        assert!(studio.camera.is_empty());
        assert_eq!(studio.microphone, "default");
        studio.microphone.clear();
        studio.update_devices(devices(&["video0", "video2"]), false);
        assert_eq!(studio.camera, "video0");
        assert!(studio.microphone.is_empty());
        studio.camera = "video2".into();
        studio.update_devices(devices(&["video0", "video2"]), false);
        assert_eq!(studio.camera, "video2");
        studio.session = Some(session());
        studio.saved = studio.session.as_ref().map(edit);
        studio.session.as_mut().unwrap().name = "Keep this unsaved draft".into();
        studio.update_devices(devices(&["video0"]), false);
        assert_eq!(studio.camera, "video0");
        assert!(studio.microphone.is_empty());
        assert!(studio.dirty());
        assert_eq!(
            studio.session.as_ref().unwrap().name,
            "Keep this unsaved draft"
        );
        studio.gesture.phase = CapturePhase::Starting;
        studio.update_devices(devices(&[]), false);
        assert_eq!(studio.camera, "video0");
        studio.gesture = CaptureGesture::default();
        studio.update_devices(devices(&[]), false);
        assert!(studio.camera.is_empty());
    }
    fn session() -> RecordingSession {
        RecordingSession {
            id: "test".into(),
            name: "Test".into(),
            orientation: "16:9".into(),
            takes: vec![RecordedTake {
                id: "take".into(),
                duration: 12.,
                ..Default::default()
            }],
            clips: vec![
                RecordingClip {
                    id: "a".into(),
                    take_id: "take".into(),
                    name: "A".into(),
                    r#in: 1.,
                    out: 9.,
                },
                RecordingClip {
                    id: "b".into(),
                    take_id: "take".into(),
                    name: "B".into(),
                    r#in: 9.,
                    out: 12.,
                },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn release_during_start_is_remembered_and_stop_runs_once() {
        let mut g = CaptureGesture::default();
        assert!(g.start(false, Instant::now()));
        assert!(!g.release(Duration::from_millis(700), false));
        assert!(g.stop_requested);
        assert!(g.started());
        assert_eq!(g.phase, CapturePhase::Stopping);
        assert!(!g.request_stop());
        assert!(!g.start(false, Instant::now()));
    }
    #[test]
    fn tap_becomes_long_take_but_cancel_always_stops() {
        let mut g = CaptureGesture::default();
        g.start(false, Instant::now());
        assert!(!g.release(Duration::from_millis(80), false));
        assert!(g.long_take);
        assert!(!g.started());
        assert!(g.request_stop());
        let mut g = CaptureGesture::default();
        g.start(false, Instant::now());
        g.release(Duration::from_millis(80), true);
        assert!(g.started());
    }
    #[test]
    fn explicit_stop_during_pending_start_never_leaves_hidden_capture() {
        let mut g = CaptureGesture::default();
        g.start(true, Instant::now());
        assert!(!g.request_stop());
        assert!(g.started());
        assert_eq!(g.phase, CapturePhase::Stopping);
    }
    #[test]
    fn split_trim_reorder_preserve_sources_and_exact_ranges() {
        let mut s = session();
        let original = s.takes.clone();
        assert!(split_clip(&mut s, "a", 4.));
        assert_eq!(s.clips.len(), 3);
        assert_eq!(s.clips[0].out, s.clips[1].r#in);
        assert_ne!(s.clips[0].id, s.clips[1].id);
        assert_eq!(s.clips[1].take_id, "take");
        assert!(!split_clip(&mut s, "a", f64::NAN));
        assert!(!split_clip(&mut s, "a", 4.));
        trim_clip(&mut s, "b", 99., false);
        assert_eq!(s.clips[2].out, 12.);
        trim_clip(&mut s, "b", -9., true);
        assert_eq!(s.clips[2].r#in, 0.);
        move_clip(&mut s, "b", -1);
        assert_eq!(s.clips[1].id, "b");
        assert_eq!(s.takes, original);
    }
    #[test]
    fn studio_draft_and_capture_guards_are_independent() {
        let mut s = Studio::default();
        s.accept(session());
        assert!(!s.dirty());
        s.session.as_mut().unwrap().name = "Edited".into();
        assert!(s.dirty());
        assert!(!s.capture());
        s.gesture.start(true, Instant::now());
        assert!(s.capture());
        let app = State {
            recording_dirty: s.dirty(),
            recording_capture: s.capture(),
            ..Default::default()
        };
        assert!(app.dirty());
    }
    #[test]
    fn late_poll_cannot_replace_a_new_session_or_completed_mutation() {
        let mut s = Studio::default();
        s.accept(session());
        let epoch = s.epoch;
        let remote = s.session.clone().unwrap();
        assert!(s.can_reconcile(epoch, &remote));
        s.accept(remote.clone());
        assert!(!s.can_reconcile(epoch, &remote));
        let mut other = remote.clone();
        other.id = "other".into();
        assert!(!s.can_reconcile(s.epoch, &other));
        s.session.as_mut().unwrap().revision = 2;
        assert!(!s.can_reconcile(s.epoch, &remote));
        s.request = true;
        assert!(!s.can_reconcile(s.epoch, s.session.as_ref().unwrap()));
    }
    #[test]
    fn sidebar_adds_exactly_nine_numbered_cards() {
        let mut recording = session();
        recording.overlays.clear();
        for _ in 0..12 {
            add_overlay(&mut recording);
        }
        assert_eq!(recording.overlays.len(), 9);
        assert_eq!(recording.overlays[8].label, "Card 9");
        let ids: std::collections::HashSet<_> = recording.overlays.iter().map(|c| &c.id).collect();
        assert_eq!(ids.len(), 9);
    }
    #[test]
    fn prepared_sequence_is_reused_only_for_saved_completed_current_revision() {
        let mut recording = session();
        recording.revision = 3;
        recording.export = Some(RecordingExport {
            id: "prepared".into(),
            status: "done".into(),
            revision: 3,
            ..Default::default()
        });
        let mut s = Studio::default();
        s.accept(recording);
        assert_eq!(current_export(&s).unwrap().id, "prepared");
        s.session.as_mut().unwrap().clips[0].out = 8.;
        assert!(
            current_export(&s).is_none(),
            "A trim draft must invalidate the previous sequence"
        );
        s.session.as_mut().unwrap().clips[0].out = 9.;
        assert!(current_export(&s).is_some());
        s.session.as_mut().unwrap().revision = 4;
        s.saved = s.session.as_ref().map(edit);
        assert!(!s.dirty());
        assert!(
            current_export(&s).is_none(),
            "Saved edits need a new prepared sequence"
        );
        let session = s.session.as_mut().unwrap();
        session.export.as_mut().unwrap().revision = 4;
        for status in ["rendering", "error"] {
            s.session.as_mut().unwrap().export.as_mut().unwrap().status = status.into();
            assert!(current_export(&s).is_none());
        }
    }
    #[test]
    fn preview_and_export_controls_exist_before_the_first_preparation() {
        fn Preview() -> Element {
            let studio = use_signal(|| {
                let mut s = Studio::default();
                let mut recording = session();
                recording.clips.clear();
                s.accept(recording);
                s
            });
            sequence_actions(studio, Api::new("http://127.0.0.1:4311").unwrap(), true)
        }
        let html = dioxus_ssr::render_element(rsx! {Preview{}});
        assert!(html.contains("record-preview-sequence"));
        assert!(html.contains("record-export-mp4"));
        assert!(html.contains("Preview sequence"));
        assert!(html.contains("Export MP4"));
        assert!(html.contains("Record or import a take"));
        assert!(html.contains("disabled"));
    }
    #[test]
    fn pending_and_active_capture_explain_why_preview_and_export_must_wait() {
        let mut s = Studio::default();
        s.accept(session());
        for phase in [
            CapturePhase::Starting,
            CapturePhase::Recording,
            CapturePhase::Stopping,
        ] {
            s.gesture.phase = phase;
            assert_eq!(
                sequence_unavailable(&s),
                Some("Stop and save your recording before previewing or exporting.")
            );
        }
    }
    #[test]
    fn live_preview_uses_one_stable_stream_and_record_page_is_accessible() {
        let api = Api::new("http://127.0.0.1:4311").unwrap();
        let url = reqwest::Url::parse(&preview_url(&api, "session")).unwrap();
        assert_eq!(url.path(), "/api/recordings/session/media/preview.mjpeg");
        assert_eq!(url.query(), None);
        fn Preview() -> Element {
            use_context_provider(|| Api::new("http://127.0.0.1:4311").unwrap());
            let state = use_signal(State::default);
            rsx! {RecordingStudio{state,active:false}}
        }
        let html = dioxus_ssr::render_element(rsx! {Preview{}});
        assert!(html.contains("Recording studio"));
        assert!(html.contains("Create a recording"));
        assert!(html.contains("Camera and microphone access starts only"));
    }
    #[test]
    fn populated_record_page_exposes_real_capture_and_sequence_controls() {
        fn Preview() -> Element {
            use_context_provider(|| Api::new("http://127.0.0.1:4311").unwrap());
            let state = use_signal(State::default);
            let studio = use_signal(|| {
                let mut s = Studio {
                    loaded: true,
                    ..Default::default()
                };
                s.accept(session());
                s
            });
            studio_view(state, studio, Api::new("http://127.0.0.1:4311").unwrap())
        }
        let html = dioxus_ssr::render_element(rsx! {Preview{}});
        for label in [
            "record-camera",
            "record-microphone",
            "No camera connected",
            "Start long take",
            "Stop and save",
            "Your sequence",
            "Move left",
            "Split",
            "Preview sequence",
            "Export MP4",
        ] {
            assert!(html.contains(label), "Missing {label}");
        }
        assert!(!html.contains("synthetic"));
        assert!(html.contains("record-preview-sequence"));
        assert!(html.contains("record-export-mp4"));
        assert!(!html.contains("record-sequence-preview"));
    }
}
