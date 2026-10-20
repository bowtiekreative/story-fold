//! Explicit YouTube URL import and channel upload controls. Secrets stay in the service.
use crate::{Page, State, api::Api};
use dioxus::prelude::*;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use storyfold_core::recording::RecordingSession;

#[derive(Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    configured: bool,
    connected: bool,
    channel: Option<Channel>,
    downloader_available: bool,
    connecting: bool,
    message: Option<String>,
}
#[derive(Clone, Default, PartialEq, Deserialize)]
struct Channel {
    id: String,
    title: String,
}
#[derive(Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    id: String,
    kind: String,
    recording_id: String,
    status: String,
    stage: String,
    progress: f64,
    error: Option<String>,
    url: Option<String>,
    bytes_sent: u64,
    bytes_total: u64,
}
impl Job {
    fn active(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "running")
    }
    fn retryable(&self) -> bool {
        matches!(self.status.as_str(), "cancelled" | "error" | "interrupted")
    }
}
#[derive(Clone, Default)]
struct YouTubeState {
    status: Status,
    jobs: Vec<Job>,
    request: bool,
    error: String,
    epoch: u64,
}
impl YouTubeState {
    fn active(&self) -> bool {
        self.jobs.iter().any(Job::active)
    }
}
fn busy(mut state: Signal<YouTubeState>, value: bool) {
    let mut s = state.write();
    s.request = value;
    if value {
        s.epoch = s.epoch.wrapping_add(1);
    }
}
fn operation(mut state: Signal<YouTubeState>, api: Api, path: String, body: Value) {
    if state.peek().request {
        return;
    }
    busy(state, true);
    spawn(async move {
        let result = api.send::<Job>(Method::POST, &path, Some(body)).await;
        busy(state, false);
        match result {
            Ok(job) => {
                let mut s = state.write();
                s.jobs.retain(|j| j.id != job.id);
                s.jobs.insert(0, job);
                s.error.clear();
            }
            Err(error) => state.write().error = error,
        }
    });
}
async fn open_google(url: String) -> Result<(), String> {
    let parsed = reqwest::Url::parse(&url)
        .map_err(|_| "The service returned an invalid Google sign-in URL.")?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("accounts.google.com")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("The service returned an unrecognized Google sign-in address.".into());
    }
    tokio::task::spawn_blocking(move || {
        open::that(url)
            .map_err(|_| "Could not open the system browser for Google sign-in.".to_owned())
    })
    .await
    .map_err(|_| "Could not launch Google sign-in.".to_owned())?
}
fn connect(mut state: Signal<YouTubeState>, api: Api) {
    if state.peek().request || state.peek().active() {
        return;
    }
    busy(state, true);
    spawn(async move {
        let result = async {
            let result: Value = api
                .send(Method::POST, "/youtube/connect", Some(json!({})))
                .await?;
            open_google(
                result["url"]
                    .as_str()
                    .ok_or("The local service did not return a Google sign-in address.")?
                    .to_owned(),
            )
            .await
        }
        .await;
        busy(state, false);
        if let Err(error) = result {
            state.write().error = error;
        }
    });
}
fn configure(mut state: Signal<YouTubeState>, api: Api) {
    if state.peek().request || state.peek().active() {
        return;
    }
    busy(state, true);
    spawn(async move {
        let result = async {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Choose Google Desktop app client JSON")
                .add_filter("Google client JSON", &["json"])
                .pick_file()
                .await
            else {
                return Ok(None);
            };
            if tokio::fs::metadata(file.path())
                .await
                .map_err(|_| "Could not read the selected client JSON.")?
                .len()
                > 65_536
            {
                return Err(
                    "Choose the Google Desktop app client JSON, smaller than 64 KB.".into(),
                );
            }
            // Read and send only the selected JSON. Neither its private fields nor its path
            // become component state, logs, rendered markup, or source artifacts.
            let bytes = tokio::fs::read(file.path())
                .await
                .map_err(|_| "Could not read the selected client JSON.")?;
            let config: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "That file is not valid Google client JSON.")?;
            let result: Status = api
                .send(Method::POST, "/youtube/config", Some(config))
                .await?;
            Ok::<_, String>(Some(result))
        }
        .await;
        busy(state, false);
        match result {
            Ok(Some(status)) => {
                state.write().status = status;
                state.write().error.clear();
            }
            Ok(None) => {}
            Err(error) => state.write().error = error,
        }
    });
}
fn disconnect(mut state: Signal<YouTubeState>, api: Api) {
    if state.peek().request || state.peek().active() {
        return;
    }
    busy(state, true);
    spawn(async move {
        let result = api
            .send::<Status>(Method::POST, "/youtube/disconnect", Some(json!({})))
            .await;
        busy(state, false);
        match result {
            Ok(status) => state.write().status = status,
            Err(error) => state.write().error = error,
        }
    });
}
fn open_video(mut state: Signal<YouTubeState>, url: String) {
    let valid = reqwest::Url::parse(&url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("www.youtube.com")
            && u.path() == "/watch"
            && u.username().is_empty()
            && u.password().is_none()
    });
    if !valid {
        state.write().error =
            "The uploaded video link was not a recognized YouTube watch address.".into();
        return;
    }
    spawn(async move {
        if tokio::task::spawn_blocking(move || open::that(url))
            .await
            .ok()
            .and_then(Result::ok)
            .is_none()
        {
            state.write().error = "Could not open the uploaded video in your browser.".into();
        }
    });
}
fn import_valid(url: &str, start: f64, end: f64) -> bool {
    !url.trim().is_empty()
        && start.is_finite()
        && end.is_finite()
        && start >= 0.
        && end > start
        && end - start <= 600.
}

#[component]
pub fn YouTubePanel(
    state: Signal<State>,
    recording: RecordingSession,
    locked: bool,
    draft_dirty: bool,
    on_busy: EventHandler<bool>,
    on_imported: EventHandler<()>,
) -> Element {
    let api = use_context::<Api>();
    let mut youtube = use_signal(YouTubeState::default);
    let mut url = use_signal(String::new);
    let mut start = use_signal(|| 0f64);
    let mut end = use_signal(|| 60f64);
    let mut title = use_signal(|| recording.name.clone());
    let mut description = use_signal(String::new);
    let mut privacy = use_signal(|| "private".to_owned());
    let mut made_for_kids = use_signal(|| false);
    let poll_api = api.clone();
    let recording_id = recording.id.clone();
    use_future(move || {
        let api = poll_api.clone();
        let id = recording_id.clone();
        async move {
            let mut completed = std::collections::HashSet::new();
            loop {
                if state.peek().page == Page::Record
                    || youtube.peek().active()
                    || youtube.peek().status.connecting
                {
                    let epoch = youtube.peek().epoch;
                    match api.get::<Status>("/youtube/status").await {
                        Ok(status) => {
                            if !youtube.peek().request && youtube.peek().epoch == epoch {
                                youtube.write().status = status;
                            }
                        }
                        Err(error) => youtube.write().error = error,
                    }
                    let epoch = youtube.peek().epoch;
                    match api.get::<Vec<Job>>("/youtube/jobs").await {
                        Ok(jobs) => {
                            if !youtube.peek().request && youtube.peek().epoch == epoch {
                                for job in &jobs {
                                    if job.kind == "import"
                                        && job.recording_id == id
                                        && job.status == "done"
                                        && completed.insert(job.id.clone())
                                    {
                                        on_imported.call(());
                                    }
                                }
                                youtube.write().jobs = jobs;
                            }
                        }
                        Err(error) => youtube.write().error = error,
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
    use_effect(move || on_busy.call(youtube.read().active() || youtube.read().request));
    let s = youtube.read().clone();
    let active = s.active();
    let mut jobs = s.jobs.clone();
    jobs.sort_by_key(|job| !job.active());
    let blocked = locked || draft_dirty || s.request || active;
    let export = recording
        .export
        .as_ref()
        .filter(|e| e.status == "done" && e.revision == recording.revision);
    let id = recording.id.clone();
    let channel = s
        .status
        .channel
        .as_ref()
        .map(|c| c.title.as_str())
        .unwrap_or("your selected channel");
    let channel_ready = s.status.connected
        && s.status
            .channel
            .as_ref()
            .is_some_and(|c| !c.id.is_empty() && !c.title.trim().is_empty());
    rsx! {
        section {class:"youtube-panel card","aria-label":"YouTube clips and publishing",onkeydown:move|e|e.stop_propagation(),
            div {class:"section-heading",h2 {"YouTube, in your workflow"}span {class:"mono","IMPORT · EDIT · UPLOAD"}}
            p {class:"hint","Bring a clip into your bin, or upload a finished recording after choosing the channel and visibility."}
            if !s.error.is_empty(){div {class:"record-alert",role:"alert",span {"{s.error}"}button {"aria-label":"Dismiss YouTube error",onclick:move|_|youtube.write().error.clear(),"×"}}}
            div {class:"youtube-grid",
                section {class:"youtube-import","aria-labelledby":"youtube-import-title",
                    h3 {id:"youtube-import-title","Bring in a YouTube clip"}
                    label {r#for:"youtube-url","Video URL" input {id:"youtube-url",r#type:"url",value:"{url}",disabled:blocked,placeholder:"https://www.youtube.com/watch?v=…",oninput:move|e|url.set(e.value())}}
                    div {class:"field-grid",
                        label {r#for:"youtube-start","Start (seconds)" input {id:"youtube-start",r#type:"number",min:0,step:0.1,value:"{start}",disabled:blocked,onchange:move|e|if let Ok(value)=e.value().parse(){start.set(value);}}}
                        label {r#for:"youtube-end","End (seconds)" input {id:"youtube-end",r#type:"number",min:0,step:0.1,value:"{end}",disabled:blocked,onchange:move|e|if let Ok(value)=e.value().parse(){end.set(value);}}}
                    }
                    p {class:"hint","Import up to 10 minutes from one video you own or have permission to edit. The selected range is saved as a local source take."}
                    if !s.status.downloader_available {p {class:"hint","YouTube import setup is not ready on this computer."}}
                    if draft_dirty {p {class:"hint","Save your studio changes before importing a clip."}}
                    button {id:"youtube-import",class:"primary",disabled:blocked||!s.status.downloader_available||!import_valid(&url.read(),*start.read(),*end.read()),onclick:{let api=api.clone();let id=id.clone();move|_|operation(youtube,api.clone(),"/youtube/imports".into(),json!({"recordingId":id,"url":url.peek().trim(),"start":*start.peek(),"end":*end.peek()}))},"Import clip into bin"}
                }
                section {class:"youtube-account","aria-labelledby":"youtube-channel-title",
                    h3 {id:"youtube-channel-title","Your YouTube channel"}
                    if channel_ready {
                        div {class:"youtube-channel",span {class:"eyebrow","CONNECTED CHANNEL"}strong {"{channel}"}small {"Uploads use this channel. Visibility is selected separately below."}}
                        button {disabled:blocked,onclick:{let api=api.clone();move|_|disconnect(youtube,api.clone())},"Disconnect channel"}
                    } else {
                        p {class:"hint","Connect in your system browser. Storyfold stores authorization privately on this computer."}
                        button {id:"youtube-connect",disabled:blocked||!s.status.configured||s.status.connecting,onclick:{let api=api.clone();move|_|connect(youtube,api.clone())},if s.status.connecting{"Waiting for Google sign-in…"}else{"Connect YouTube channel"}}
                    }
                    if let Some(message)=&s.status.message {p {class:"hint",role:"status","{message}"}}
                    details {class:"youtube-setup",summary {if s.status.configured{"Change Google app configuration"}else{"One-time Google app setup"}}
                        p {class:"hint","Choose your Google Cloud Desktop app client JSON with YouTube Data API v3 enabled. The file’s credentials stay out of the app’s source and logs."}
                        button {disabled:blocked,onclick:{let api=api.clone();move|_|configure(youtube,api.clone())},"Choose Desktop app client JSON…"}
                    }
                }
                section {class:"youtube-publish","aria-labelledby":"youtube-publish-title",
                    h3 {id:"youtube-publish-title","Upload a finished recording"}
                    label {r#for:"youtube-title","Video title" input {id:"youtube-title",maxlength:100,value:"{title}",disabled:blocked,oninput:move|e|title.set(e.value())}}
                    label {r#for:"youtube-description","Description" textarea {id:"youtube-description",maxlength:5000,rows:3,value:"{description}",disabled:blocked,oninput:move|e|description.set(e.value())}}
                    label {r#for:"youtube-privacy","Visibility" select {id:"youtube-privacy",value:"{privacy}",disabled:blocked,onchange:move|e|privacy.set(e.value()),option {value:"private","Private — only you"}option {value:"unlisted","Unlisted — anyone with the link"}option {value:"public","Public — visible on YouTube"}}}
                    label {class:"check",input {r#type:"checkbox",checked:*made_for_kids.read(),disabled:blocked,onchange:move|e|made_for_kids.set(e.checked())}"This video is made for kids"}
                    if export.is_none(){p {class:"hint","Render your current recording before uploading. Uploads always use the finished MP4 snapshot."}}
                    if channel_ready {p {class:"youtube-upload-review","Channel: {channel} · Visibility: {privacy}"}}
                    button {id:"youtube-upload",class:"primary",disabled:blocked||!channel_ready||export.is_none()||title.read().trim().is_empty(),onclick:{let api=api.clone();let id=id.clone();let export_id=export.map(|e|e.id.clone());let channel_id=s.status.channel.as_ref().map(|c|c.id.clone()).unwrap_or_default();move|_|if let Some(export_id)=&export_id {operation(youtube,api.clone(),"/youtube/uploads".into(),json!({"recordingId":id,"exportId":export_id,"channelId":channel_id,"title":title.peek().trim(),"description":*description.peek(),"privacy":*privacy.peek(),"madeForKids":*made_for_kids.peek()}));}},"Upload {privacy} video"}
                }
            }
            if !s.jobs.is_empty(){section {class:"youtube-jobs","aria-label":"YouTube transfer progress",
                h3 {"Transfers"}
                for job in jobs.iter().filter(|job|job.active()||job.recording_id==recording.id).take(10) {
                    {let job=job.clone();let cancel_id=job.id.clone();let retry_id=job.id.clone();rsx! {
                        div {class:"youtube-job",
                            div {strong {if job.kind=="import"{"Import clip"}else{"Upload video"}}span {"{job.stage}"}small {"{job.status}"}}
                            if job.recording_id!=recording.id {small {"Transfer for another saved recording"}}
                            if job.active(){progress {max:100,value:job.progress,"aria-label":"YouTube transfer progress"}}
                            if job.bytes_total>0{small {"{job.bytes_sent/1_000_000} / {job.bytes_total/1_000_000} MB"}}
                            if let Some(error)=&job.error{p {class:"error",role:"alert","{error}"}}
                            div {class:"actions",
                                if job.active(){button {disabled:s.request,onclick:{let api=api.clone();move|_|operation(youtube,api.clone(),format!("/youtube/jobs/{cancel_id}/cancel"),json!({}))},"Cancel transfer"}}
                                if job.retryable(){button {disabled:blocked,onclick:{let api=api.clone();move|_|operation(youtube,api.clone(),format!("/youtube/jobs/{retry_id}/retry"),json!({}))},"Retry transfer"}}
                                if job.status=="done"&&job.kind=="upload"{if let Some(url)=job.url.clone() {button {onclick:move|_|open_video(youtube,url.clone()),"Open video on YouTube"}}}
                            }
                        }
                    }}
                }
            }}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn oauth_never_opens_an_unrecognized_browser_destination() {
        for url in [
            "http://accounts.google.com/o/oauth2/v2/auth",
            "https://example.com/auth",
            "https://user:secret@accounts.google.com/auth",
            "javascript:alert(1)",
        ] {
            assert!(open_google(url.into()).await.is_err());
        }
    }
    #[test]
    fn import_range_is_explicit_finite_and_at_most_ten_minutes() {
        assert!(import_valid("https://youtu.be/example", 3., 603.));
        for (a, b) in [
            (0., 601.),
            (-1., 10.),
            (2., 2.),
            (f64::NAN, 20.),
            (0., f64::INFINITY),
        ] {
            assert!(!import_valid("video", a, b));
        }
        assert!(!import_valid("", 0., 20.));
    }
    #[test]
    fn panel_defaults_to_private_and_never_claims_an_account_connection() {
        fn Preview() -> Element {
            use_context_provider(|| Api::new("http://127.0.0.1:4311").unwrap());
            let state = use_signal(State::default);
            rsx! {YouTubePanel{state,recording:RecordingSession::default(),locked:false,draft_dirty:false,on_busy:move |_|{},on_imported:move |_|{}}}
        }
        let html = dioxus_ssr::render_element(rsx! {Preview{}});
        assert!(html.contains("Import clip into bin"));
        assert!(html.contains("Connect YouTube channel"));
        assert!(html.contains("Upload private video"));
        assert!(html.contains("Private — only you"));
        assert!(!html.contains("CONNECTED CHANNEL"));
        assert!(html.contains("Render your current recording before uploading"));
    }
}
