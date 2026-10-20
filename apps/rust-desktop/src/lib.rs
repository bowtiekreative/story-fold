#![allow(non_snake_case)]
pub mod api;
mod build;
mod dom;
mod editor;
pub mod model;
mod player;
#[cfg(debug_assertions)]
mod preview_probe;
mod projects;
mod recording;
#[cfg(debug_assertions)]
mod recording_smoke;
#[cfg(debug_assertions)]
mod smoke;
mod youtube;

use api::Api;
use dioxus::prelude::*;
use model::*;
use reqwest::Method;
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use storyfold_core::recording::RecordingSession;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Page {
    #[default]
    Library,
    Projects,
    Editor,
    Record,
    Build,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Destination {
    Page(Page),
    Project(String),
    Recording(Option<String>),
    Reload,
    Quit,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RecordingRequest {
    pub id: Option<String>,
    pub nonce: u64,
}
#[derive(Clone, Default)]
pub struct State {
    pub page: Page,
    pub current: Option<Draft>,
    pub active_id: String,
    pub library: Vec<Summary>,
    pub recording_library: Vec<RecordingSession>,
    pub recording_request: Option<RecordingRequest>,
    pub recording_request_nonce: u64,
    pub recording_busy: bool,
    pub health: Health,
    pub error: String,
    pub notice: String,
    pub busy: bool,
    pub saving: bool,
    pub source_dirty: bool,
    pub recording_dirty: bool,
    pub recording_capture: bool,
    pub build_dialog: bool,
    pub pending: Option<Destination>,
    pub export_job: Option<ExportJob>,
}
impl State {
    pub fn dirty(&self) -> bool {
        self.source_dirty || self.recording_dirty || self.current.as_ref().is_some_and(Draft::dirty)
    }
    fn needs_confirmation(&self, destination: &Destination) -> bool {
        match destination {
            Destination::Quit => self.dirty(),
            Destination::Reload => self.current.as_ref().is_some_and(Draft::dirty),
            Destination::Project(id) => {
                id != &self.active_id && self.current.as_ref().is_some_and(Draft::dirty)
            }
            Destination::Recording(_) => self.recording_dirty,
            Destination::Page(_) => false,
        }
    }
}

pub fn launch() {
    launch_with_backend(None);
}
pub fn launch_with_backend(mut backend: Option<std::process::Child>) {
    use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_custom_event_handler(move |event, _| {
                    if matches!(event, dioxus::desktop::tao::event::Event::LoopDestroyed)
                        && let Some(mut child) = backend.take()
                    {
                        stop_owned_backend(&mut child);
                    }
                })
                .with_window(
                    WindowBuilder::new()
                        .with_title("Storyfold")
                        .with_inner_size(LogicalSize::new(1440., 960.))
                        .with_min_inner_size(LogicalSize::new(900., 640.)),
                )
                .with_navigation_handler(|url| {
                    if url == "about:blank" {
                        return true;
                    }
                    reqwest::Url::parse(url).is_ok_and(|u| {
                        u.scheme() == "dioxus"
                            || matches!(u.scheme(), "http" | "https")
                                && matches!(
                                    u.host_str(),
                                    Some("dioxus.localhost" | "dioxus.index.html")
                                )
                    })
                }),
        )
        .launch(App);
}

pub fn App() -> Element {
    let api = use_context_provider(|| {
        Api::new(
            &std::env::var("STORYFOLD_SERVER").unwrap_or_else(|_| "http://127.0.0.1:4311".into()),
        )
        .expect("STORYFOLD_SERVER must be a local HTTP address")
    });
    let mut state = use_signal(State::default);
    #[cfg(debug_assertions)]
    use_future(move || async move {
        if let Some(path) = std::env::var_os("STORYFOLD_PREVIEW_PROBE_FILE") {
            preview_probe::run(state, PathBuf::from(path)).await;
        } else if let Some(path) = std::env::var_os("STORYFOLD_RECORD_SMOKE_FILE") {
            recording_smoke::run(state, PathBuf::from(path)).await;
        } else if let Some(path) = std::env::var_os("STORYFOLD_SMOKE_FILE") {
            smoke::run(state, PathBuf::from(path)).await;
        }
    });
    let poll_api = api.clone();
    use_future(move || {
        let api = poll_api.clone();
        async move {
            loop {
                if let Ok(library) = api.get::<Vec<Summary>>("/projects").await
                    && state.peek().library != library
                {
                    state.write().library = library;
                }
                if let Ok(library) = api.get::<Vec<RecordingSession>>("/recordings").await
                    && state.peek().recording_library != library
                {
                    state.write().recording_library = library;
                }
                match api.get::<Health>("/health").await {
                    Ok(health) => {
                        if state.peek().health != health {
                            state.write().health = health;
                        }
                        if state
                            .peek()
                            .error
                            .starts_with("Cannot reach the local Storyfold service.")
                        {
                            state.write().error.clear();
                        }
                    }
                    Err(e) => {
                        if state.peek().library.is_empty() {
                            state.write().error = e;
                        }
                    }
                }
                let id = state.peek().active_id.clone();
                let needs_refresh = !id.is_empty()
                    && state
                        .peek()
                        .current
                        .as_ref()
                        .is_none_or(|d| d.project.status != "ready");
                if needs_refresh {
                    match api.get::<Project>(&format!("/projects/{id}")).await {
                        Ok(project) => {
                            let mut s = state.write();
                            if s.active_id == id {
                                s.current = Some(Draft::new(project));
                            }
                        }
                        Err(e) => state.write().error = e,
                    }
                }
                let job = state.peek().export_job.clone().filter(ExportJob::running);
                if let Some(job) = job {
                    match api
                        .get::<ExportJob>(&format!(
                            "/projects/{}/exports/{}",
                            job.project_id, job.id
                        ))
                        .await
                    {
                        Ok(job) => state.write().export_job = Some(job),
                        Err(e) => state.write().error = e,
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    });
    use_effect(move || {
        let dirty = state.read().dirty() || state.read().recording_capture;
        if let Some(window) = try_consume_context::<dioxus::desktop::DesktopContext>() {
            window.set_close_behavior(if dirty {
                dioxus::desktop::WindowCloseBehaviour::WindowHides
            } else {
                dioxus::desktop::WindowCloseBehaviour::WindowCloses
            });
        }
    });
    // Native close requests must not silently throw away an unsaved transcript or source draft.
    dioxus::desktop::use_wry_event_handler(move |event, _| {
        if matches!(
            event,
            dioxus::desktop::tao::event::Event::WindowEvent {
                event: dioxus::desktop::tao::event::WindowEvent::CloseRequested,
                ..
            }
        ) && (state.peek().dirty() || state.peek().recording_capture)
        {
            if state.peek().recording_capture {
                state.write().error =
                    "Stop and save your recording before closing Storyfold.".into();
            } else {
                state.write().pending = Some(Destination::Quit);
            }
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                dioxus::desktop::window().set_visible(true);
                dioxus::desktop::window().set_focus();
            });
        }
    });
    let s = state.read().clone();
    let heading = match s.page {
        Page::Library => "New project",
        Page::Projects => "Your projects",
        Page::Editor => "Your story",
        Page::Record => "Recording studio",
        Page::Build => "Your build",
    };
    rsx! {
        style { {include_str!("style.css")} }
        div {class:"shell",
            div {inert:s.pending.is_some().then_some(true),
            header {class:"header",inert:s.build_dialog.then_some(true),
                button {class:"brand",onclick:move |_|navigate(state,Destination::Page(Page::Library)),span {class:"mark","sf"} "Storyfold"}
                span {class:"tagline","A little less footage. A lot more story."}
                nav {"aria-label":"Main navigation",
                    button {class:if s.page==Page::Library {"active"}else{""},onclick:move |_|navigate(state,Destination::Page(Page::Library)),"New project"}
                    button {class:if s.page==Page::Projects {"active"}else{""},onclick:move |_|navigate(state,Destination::Page(Page::Projects)),"Projects"}
                    if s.current.is_some() {button {class:if s.page==Page::Editor {"active"}else{""},onclick:move |_|navigate(state,Destination::Page(Page::Editor)),"Editor"}}
                    button {class:if s.page==Page::Record {"active"}else{""},onclick:move |_|navigate(state,Destination::Page(Page::Record)),"Record"}
                    button {class:if s.page==Page::Build {"active"}else{""},onclick:move |_|navigate(state,Destination::Page(Page::Build)),"Your build"}
                }
            }
            if !s.error.is_empty() {div {class:"banner error",role:"alert",span {"{s.error}"}button {"aria-label":"Dismiss error",onclick:move |_|state.write().error.clear(),"×"}}}
            if !s.notice.is_empty() {div {class:"banner notice",role:"status",span {"{s.notice}"}button {"aria-label":"Dismiss notice",onclick:move |_|state.write().notice.clear(),"×"}}}
            main {"aria-label":"{heading}",
                // Import choices stay intact while browsing saved projects.
                div {hidden:s.page!=Page::Library,Library {state}}
                if s.page==Page::Projects {projects::Projects {state}}
                if s.page==Page::Editor {
                    if let Some(draft)=&s.current {
                        if draft.project.status=="ready" {editor::Editor {state}}
                        else {Processing {state}}
                    } else {div {class:"loading",role:"status","Opening your interview…"}}
                }
                // Keep source drafts mounted when switching views. Polling never replaces their text.
                div {hidden:s.page!=Page::Build,build::Build {state,active:s.page==Page::Build}}
                div {hidden:s.page!=Page::Record,recording::RecordingStudio {state,active:s.page==Page::Record}}
            }
            }
            if let Some(destination)=s.pending.clone() {
                div {class:"modal-backdrop",
                    section {class:"modal",role:"alertdialog","aria-modal":"true","aria-labelledby":"discard-title",onkeydown:move |e|if e.key()==Key::Escape {e.stop_propagation();state.write().pending=None;},
                        h2 {id:"discard-title","Keep your changes?"}
                        p {"Your transcript, recording studio, or source draft has unsaved edits. Save the draft before leaving, or discard it."}
                        div {class:"actions",
                            button {class:"primary",autofocus:true,onclick:move |_|state.write().pending=None,"Keep editing"}
                            button {disabled:s.busy||s.saving,onclick:move |_| {state.write().pending=None;go(state,destination.clone());},"Discard and continue"}
                        }
                    }
                }
            }
        }
    }
}

pub fn navigate(mut state: Signal<State>, destination: Destination) {
    if state.peek().recording_capture {
        state.write().error = "Stop and save your recording before leaving the studio.".into();
        return;
    }
    if state.peek().busy || state.peek().saving {
        state.write().error = "Wait for the current request to finish before leaving.".into();
        return;
    }
    if matches!(&destination, Destination::Recording(_))
        && (state.peek().recording_busy || state.peek().recording_request.is_some())
    {
        state.write().error =
            "Wait for the studio request or export to finish before opening another recording."
                .into();
        return;
    }
    // Views retain drafts; replacing a project or closing can discard work.
    if state.peek().needs_confirmation(&destination) {
        state.write().pending = Some(destination);
    } else {
        go(state, destination);
    }
}
fn go(mut state: Signal<State>, destination: Destination) {
    match destination {
        Destination::Page(page) => state.write().page = page,
        Destination::Project(id) => {
            let mut s = state.write();
            if s.active_id == id && s.current.is_some() {
                s.page = Page::Editor;
                return;
            }
            s.active_id = id;
            s.current = None;
            s.page = Page::Editor;
            s.error.clear();
            s.export_job = None;
        }
        Destination::Recording(id) => {
            let mut s = state.write();
            s.recording_request_nonce = s.recording_request_nonce.wrapping_add(1);
            s.recording_request = Some(RecordingRequest {
                id,
                nonce: s.recording_request_nonce,
            });
            s.page = Page::Record;
            s.error.clear();
        }
        Destination::Reload => {
            state.write().current = None;
        }
        Destination::Quit => {
            let window = dioxus::desktop::window();
            window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
            window.close();
        }
    }
}

pub fn stop_owned_backend(child: &mut std::process::Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    #[cfg(unix)]
    // The Child owns this unreaped PID, so it cannot refer to an unrelated recycled process.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGINT);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    for _ in 0..40 {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn change(mut state: Signal<State>, f: impl FnOnce(&mut Project)) {
    if let Some(draft) = state.write().current.as_mut() {
        draft.change(f);
    }
}
pub async fn save(mut state: Signal<State>, api: Api) -> Result<(), String> {
    let Some(draft) = state.peek().current.clone() else {
        return Ok(());
    };
    if !draft.dirty() {
        return Ok(());
    }
    if state.peek().saving {
        return Err("A save is already in progress. Please try again when it finishes.".into());
    }
    state.write().saving = true;
    let result = api
        .send::<Project>(
            Method::PATCH,
            &format!("/projects/{}", draft.project.id),
            Some(draft.payload()),
        )
        .await;
    state.write().saving = false;
    match result {
        Ok(next) => {
            let mut s = state.write();
            if let Some(current) = s.current.as_mut().filter(|d| d.project.id == next.id) {
                current.saved(next.revision, draft.generation);
            }
            s.error.clear();
            Ok(())
        }
        Err(error) => {
            state.write().error = error.clone();
            Err(error)
        }
    }
}

#[component]
fn Library(state: Signal<State>) -> Element {
    let api = use_context::<Api>();
    let mut files = use_signal(Vec::<PathBuf>::new);
    let mut transcript = use_signal(|| None::<PathBuf>);
    let mut name = use_signal(String::new);
    let mut manual = use_signal(|| false);
    let s = state.read().clone();
    let start_api = api.clone();
    let demo_api = api.clone();
    rsx! {
        div {class:"library",
            section {class:"intro",
                span {class:"eyebrow","NEW PROJECT"}
                h1 {"Your words." br {} em {"A story worth sharing."}}
                p {"Bring your interview. Whisper transcribes it on this computer, then topic folders help you find the story. Edit by sentence and keep every camera angle."}
                div {class:"pills",span {"Local speech timing"}span {"Up to 3 angles"}span {"Sentence editing"}}
                div {class:"actions",
                    button {onclick:move |_|navigate(state,Destination::Recording(None)),"Start a new recording"}
                    button {onclick:move |_|navigate(state,Destination::Page(Page::Projects)),"Open a saved project"}
                }
            }
            section {class:"card import",
                h2 {"Start with an interview"}
                label {r#for:"interview-name","Interview name"}
                input {id:"interview-name",value:"{name}",maxlength:120,oninput:move |e|name.set(e.value()),placeholder:"A conversation worth keeping"}
                button {disabled:s.busy,onclick:move |_| {spawn(async move {
                    if let Some(chosen)=rfd::AsyncFileDialog::new().set_title("Choose one to three camera videos").add_filter("Video",&["mp4","mov","mkv","webm","mxf","avi"]).pick_files().await {
                        if chosen.len()>3 {state.write().error="Choose up to three camera files.".into();return;}
                        let chosen:Vec<_>=chosen.into_iter().map(|f|f.path().to_path_buf()).collect();
                        if name.peek().trim().is_empty()&& let Some(stem)=chosen.first().and_then(|p|p.file_stem()) {name.set(stem.to_string_lossy().into_owned());}
                        files.set(chosen);
                    }
                });},"Choose camera videos"}
                for (i,file) in files.read().iter().enumerate() {
                    div {class:"file-row",span {class:"badge","CAM {['A','B','C'][i]}"}span {"{file.file_name().unwrap_or_default().to_string_lossy()}"}
                        button {"aria-label":"Remove camera video",onclick:move |_| {files.write().remove(i);},"×"}}
                }
                p {class:"hint","The first file is camera A and the master audio. Up to 4 GB and two hours per camera."}
                label {class:"check",input {r#type:"checkbox",checked:*manual.read(),onchange:move |e|manual.set(e.checked())} "Organize topics myself"}
                details {summary {"Optional timed transcript"}
                    p {class:"hint","Whisper runs automatically. Import JSON only when you already have words with start and end times."}
                    button {onclick:move |_| {spawn(async move {if let Some(file)=rfd::AsyncFileDialog::new().add_filter("Timed transcript JSON",&["json"]).pick_file().await {transcript.set(Some(file.path().to_path_buf()));}});},"Choose transcript JSON"}
                    if let Some(file)=transcript.read().as_ref() {div {class:"file-row",span {"{file.file_name().unwrap_or_default().to_string_lossy()}"}button {onclick:move |_|transcript.set(None),"Remove"}}}
                }
                div {class:"providers",
                    span {"Speech: {s.health.speech_provider}"}
                    span {"Topics: {s.health.topic_provider}"}
                    if !s.health.transcription {span {class:"hint","Speech setup is not ready. A timed transcript can be imported while setup finishes."}}
                    if !*manual.read() && s.health.topics {span {class:"hint","Automatic grouping sends transcript text to the configured topic provider. Video and audio stay local."}}
                }
                div {class:"actions",
                    button {class:"primary",disabled:s.busy||files.read().is_empty(),onclick:move |_| {
                        let api=start_api.clone();let chosen=files.peek().clone();let timed=transcript.peek().clone();let title=name.peek().trim().to_owned();let manual=*manual.peek();
                        state.write().busy=true;state.write().error.clear();
                        spawn(async move {let result=api.upload::<Value>(&chosen,timed.as_deref(),if title.is_empty(){"Untitled interview"}else{&title},manual).await;state.write().busy=false;match result {Ok(data)=>{if let Some(id)=data["projectId"].as_str(){navigate(state,Destination::Project(id.into()));}},Err(e)=>state.write().error=e}});
                    },if s.busy {"Importing…"}else{"Make my story"}}
                    button {disabled:s.busy,onclick:move |_|{let api=demo_api.clone();state.write().busy=true;spawn(async move {let result=api.send::<Value>(Method::POST,"/projects/sample",None).await;state.write().busy=false;match result {Ok(data)=>{if let Some(id)=data["projectId"].as_str(){navigate(state,Destination::Project(id.into()));}},Err(e)=>state.write().error=e}});},"Try the sample"}
                }
            }
        }
    }
}

#[component]
fn Processing(state: Signal<State>) -> Element {
    let api = use_context::<Api>();
    let Some(project) = state.read().current.as_ref().map(|d| d.project.clone()) else {
        return rsx! {p {"Opening…"}};
    };
    let last = project.progress.last().cloned().unwrap_or_default();
    let cancel_api = api.clone();
    let id = project.id.clone();
    let cancel_id = id.clone();
    rsx! {
        section {class:"processing card",span {class:"eyebrow","YOUR WORDS, TAKING SHAPE"}
            h1 {if project.status=="error" {"Let’s pick up where we left off."}else{"Listening to every word."}}
            h2 {"{project.name}"}
            if let Some(error)=project.error {p {class:"error",role:"alert","{error}"}}
            progress {max:100,value:last.pct,"aria-label":"Processing progress"}
            p {role:"status","{last.detail}"}
            ol {class:"steps",for (key,title) in [("uploading","Prepare footage"),("transcribing","Transcribe speech"),("segmenting","Find sentences"),("syncing","Align cameras"),("grouping","Group topics"),("ready","Ready to edit")] {
                li {class:if last.step==key {"current"}else{""},strong {"{title}"}small {{project.progress.iter().rev().find(|p|p.step==key).map(|p|p.detail.as_str()).unwrap_or("Waiting")}}}
            }}
            for warning in project.warnings {p {class:"hint","{warning}"}}
            div {class:"actions",
                if project.status=="error" {button {class:"primary",disabled:state.read().busy,onclick:move |_|{let api=api.clone();let id=id.clone();state.write().busy=true;spawn(async move {let result=api.send::<Value>(Method::POST,&format!("/projects/{id}/resume"),None).await;state.write().busy=false;match result {Ok(_)=>state.write().current=None,Err(e)=>state.write().error=e}});},"Resume processing"}}
                else {button {disabled:state.read().busy,onclick:move |_|{let api=cancel_api.clone();let id=cancel_id.clone();state.write().busy=true;spawn(async move {let result=api.send::<Value>(Method::POST,&format!("/projects/{id}/cancel"),None).await;state.write().busy=false;if let Err(e)=result{state.write().error=e;}});},"Cancel processing"}}
                button {onclick:move |_|navigate(state,Destination::Page(Page::Projects)),"Back to projects"}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_is_new_project_even_when_saved_projects_exist() {
        let state = State {
            library: vec![Summary {
                id: "old-interview".into(),
                ..Default::default()
            }],
            recording_library: vec![RecordingSession {
                id: "old-recording".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(state.page, Page::Library);
        assert!(state.current.is_none());
        assert!(state.active_id.is_empty());
        assert!(state.recording_request.is_none());
        assert!(!state.recording_capture);
    }

    #[test]
    fn new_and_open_recording_confirm_dirty_studio_but_tabs_retain_drafts() {
        let state = State {
            recording_dirty: true,
            ..Default::default()
        };
        assert!(state.needs_confirmation(&Destination::Recording(None)));
        assert!(state.needs_confirmation(&Destination::Recording(Some("saved".into()))));
        assert!(state.needs_confirmation(&Destination::Quit));
        assert!(!state.needs_confirmation(&Destination::Page(Page::Projects)));
        assert!(!state.needs_confirmation(&Destination::Page(Page::Library)));
        assert!(!State::default().needs_confirmation(&Destination::Recording(None)));
    }

    #[test]
    fn interview_replace_still_confirms_and_keeps_existing_project_when_switching_tabs() {
        let mut draft = Draft::new(Project {
            id: "current".into(),
            ..Default::default()
        });
        draft.change(|p| p.name = "Unsaved words".into());
        let state = State {
            active_id: "current".into(),
            current: Some(draft),
            ..Default::default()
        };
        assert!(state.needs_confirmation(&Destination::Project("different".into())));
        assert!(state.needs_confirmation(&Destination::Reload));
        assert!(!state.needs_confirmation(&Destination::Project("current".into())));
        assert!(!state.needs_confirmation(&Destination::Page(Page::Projects)));
        assert_eq!(
            state.current.as_ref().unwrap().project.name,
            "Unsaved words"
        );
    }

    #[test]
    fn native_library_renders_accessible_import_actions() {
        fn Preview() -> Element {
            use_context_provider(|| Api::new("http://127.0.0.1:4311").unwrap());
            let state = use_signal(State::default);
            rsx! {Library {state}}
        }
        let html = dioxus_ssr::render_element(rsx! {Preview {}});
        assert!(html.contains("Choose camera videos"));
        assert!(html.contains("Organize topics myself"));
        assert!(html.contains("interview-name"));
        assert!(html.contains("Try the sample"));
    }
}
