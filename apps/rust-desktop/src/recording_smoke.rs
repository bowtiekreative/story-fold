//! Opt-in native studio test. Refuses every camera except the debug synthetic source.
use crate::{State, api::Api, dom};
use dioxus::prelude::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use storyfold_core::recording::{RecordingDevices, RecordingSession};

pub async fn run(mut state: Signal<State>, destination: PathBuf) {
    let result = tokio::time::timeout(Duration::from_secs(130), exercise())
        .await
        .unwrap_or_else(|_| Err("Native recording check timed out.".into()));
    let artifact = match result {
        Ok(checks) => json!({"ok":true,"runtime":"Rust/Dioxus desktop","checks":checks}),
        Err(error) => json!({"ok":false,"runtime":"Rust/Dioxus desktop","error":error}),
    };
    if let Some(parent) = destination.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&artifact) {
        let _ = tokio::fs::write(destination, bytes).await;
    }
    if std::env::var_os("STORYFOLD_SMOKE_EXIT").is_some() {
        // The test has no physical device permission. Still leave a synthetic capture
        // visible on failure until the backend has explicitly acknowledged Stop.
        if state.peek().recording_capture {
            let _ = click("document.getElementById('record-explicit-stop')").await;
            for _ in 0..100 {
                if !state.peek().recording_capture {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        if !state.peek().recording_capture {
            state.write().pending = None;
            let window = dioxus::desktop::window();
            window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
            window.close();
        }
    }
}
async fn js_bool(script: &str) -> Result<bool, String> {
    Ok(dom::eval(script, None).await?.as_bool().unwrap_or(false))
}
async fn click(expression: &str) -> Result<(), String> {
    if js_bool(&format!(
        "const b={expression};if(!b||b.disabled)return false;b.click();return true;"
    ))
    .await?
    {
        Ok(())
    } else {
        Err(format!(
            "Native recording action was unavailable: {expression}"
        ))
    }
}
async fn wait(script: &str, attempts: usize) -> Result<(), String> {
    for _ in 0..attempts {
        if js_bool(script).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "Native recording view did not reach expected state: {script}"
    ))
}
async fn input(selector: &str, text: &str) -> Result<(), String> {
    let result = dom::eval("const [selector,text]=await dioxus.recv();const e=document.querySelector(selector);if(!e||e.disabled)return false;e.value=text;e.dispatchEvent(new Event('input',{bubbles:true}));return true;",Some(json!([selector,text]))).await?;
    if result == true {
        Ok(())
    } else {
        Err(format!("Native recording field missing: {selector}"))
    }
}
async fn current_id() -> Result<String, String> {
    dom::eval(
        "return document.getElementById('record-session')?.value||'';",
        None,
    )
    .await?
    .as_str()
    .filter(|v| !v.is_empty())
    .map(str::to_owned)
    .ok_or_else(|| "No native recording session selected.".into())
}
async fn session(api: &Api, id: &str) -> Result<RecordingSession, String> {
    api.get(&format!("/recordings/{id}")).await
}
async fn wait_takes(api: &Api, id: &str, count: usize) -> Result<RecordingSession, String> {
    for _ in 0..300 {
        let value = session(api, id).await?;
        if value.active.is_none() && value.takes.len() == count {
            return Ok(value);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "Native capture did not finalize {count} synthetic takes."
    ))
}
async fn pointer(kind: &str) -> Result<(), String> {
    let result=dom::eval("const kind=await dioxus.recv();const b=document.getElementById('record-gesture');if(!b||b.disabled)return false;b.dispatchEvent(new PointerEvent(kind,{bubbles:true,pointerId:923,isPrimary:true,pointerType:'mouse',button:0,buttons:kind==='pointerdown'?1:0}));return true;",Some(json!(kind))).await?;
    if result == true {
        Ok(())
    } else {
        Err("Native hold/tap control is unavailable.".into())
    }
}
async fn stream_frames(api: &Api, id: &str) -> Result<usize, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| "Could not prepare the generated preview stream check.")?;
    let mut response = client
        .get(api.url(&format!("/recordings/{id}/media/preview.mjpeg")))
        .send()
        .await
        .map_err(|_| "The live preview stream was unavailable.")?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let boundary = content_type
        .split("boundary=")
        .nth(1)
        .ok_or("The live preview did not return a multipart stream.")?
        .trim()
        .trim_matches('"')
        .to_owned();
    let marker = format!("--{boundary}").into_bytes();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The generated live preview stream stopped before three frames.")?
    {
        bytes.extend_from_slice(&chunk);
        let frames = bytes
            .windows(marker.len())
            .filter(|part| *part == marker)
            .count();
        if frames >= 3 {
            return Ok(frames);
        }
        if bytes.len() > 8_000_000 {
            return Err("Generated preview stream framing was invalid.".into());
        }
    }
    Err("Generated preview stream ended before three frames.".into())
}
async fn wait_prepared(
    api: &Api,
    id: &str,
    previous: Option<&str>,
) -> Result<RecordingSession, String> {
    for _ in 0..900 {
        let s = session(api, id).await?;
        if let Some(job) = &s.export {
            if job.status == "done"
                && job.revision == s.revision
                && previous != Some(job.id.as_str())
            {
                return Ok(s);
            }
            if job.status == "error" {
                return Err(job
                    .error
                    .clone()
                    .unwrap_or_else(|| "Native sequence preparation failed.".into()));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("Native sequence preview did not become ready.".into())
}
async fn exercise() -> Result<Value, String> {
    let api = consume_context::<Api>();
    let devices: RecordingDevices = api.get("/recording-devices").await?;
    if !devices.cameras.iter().any(|d| d.id == "synthetic")
        || devices.cameras.iter().any(|d| d.id != "synthetic")
    {
        return Err("Native recording test requires an isolated backend with only the debug synthetic camera. No hardware was opened.".into());
    }
    eprintln!("Storyfold native check: startup");
    tokio::time::sleep(Duration::from_millis(700)).await;
    if !js_bool("return document.querySelector('.header nav button.active')?.textContent.trim()==='New project'&&!document.querySelector('.editor');").await?{return Err("Startup did not show a fresh workspace.".into());}
    let _seed: RecordingSession = api
        .send(
            reqwest::Method::POST,
            "/recordings",
            Some(json!({"name":"Previous generated recording","orientation":"16:9"})),
        )
        .await?;
    click("Array.from(document.querySelectorAll('.header nav button')).find(b=>b.textContent.trim()==='Projects')").await?;
    wait("return !!document.querySelector('.projects-page [aria-label=\"Saved recordings\"] .project-card');",100).await?;
    if !js_bool("return !document.querySelector('.projects-page video');").await? {
        return Err("Projects unexpectedly opened media.".into());
    }
    click("Array.from(document.querySelectorAll('.header nav button')).find(b=>b.textContent.trim()==='Record')").await?;
    wait(
        "return !document.querySelector('.record-studio').closest('[hidden]');",
        50,
    )
    .await?;
    wait("return Array.from(document.querySelectorAll('#record-camera option')).some(o=>o.value==='synthetic')||!!document.querySelector('.record-welcome');",100).await?;
    if !js_bool("return !!document.querySelector('.record-welcome')&&!document.getElementById('record-sequence-preview');").await?{return Err("Record implicitly opened a saved recording.".into());}
    let before: Vec<RecordingSession> = api.get("/recordings").await?;
    click("Array.from(document.querySelectorAll('.record-session-picker button')).find(b=>b.textContent.trim()==='New recording')").await?;
    let mut created = false;
    for _ in 0..100 {
        if let Ok(id)=current_id().await
            && !before.iter().any(|s|s.id==id)
            && js_bool("return !!document.getElementById('record-gesture')&&!document.getElementById('record-gesture').disabled;").await?
        {created=true;break;}
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !created {
        return Err("Native New recording did not open a fresh synthetic session.".into());
    }
    eprintln!("Storyfold native check: fresh recording created");
    let id = current_id().await?;
    if before.iter().any(|s| s.id == id) {
        return Err("Native recording test refused to alter a pre-existing session.".into());
    }
    if !js_bool("const p=document.querySelector('.youtube-panel');return !!p&&!!document.getElementById('youtube-url')&&!!document.getElementById('youtube-start')&&!!document.getElementById('youtube-end')&&document.getElementById('youtube-privacy')?.value==='private'&&document.getElementById('youtube-upload')?.disabled&&!!document.getElementById('youtube-connect')&&Array.from(p.querySelectorAll('summary')).some(s=>s.textContent.includes('Google app'));").await? {
        return Err("Native YouTube controls did not expose URL import, Private default, connection setup, and guarded upload.".into());
    }
    if !js_bool("const e=document.getElementById('record-microphone');if(!Array.from(e.options).some(o=>o.value==='synthetic-audio'))return false;e.value='synthetic-audio';e.dispatchEvent(new Event('change',{bubbles:true}));return true;").await? {
        return Err("Native recording test refused to use a physical microphone.".into());
    }
    input("#record-name", "Native recording smoke").await?;
    click("Array.from(document.querySelectorAll('.record-toolbar button')).find(b=>b.textContent.trim()==='Save changes')").await?;
    wait("return document.querySelector('.record-toolbar .save-status').textContent==='Saved on this computer';",100).await?;
    if session(&api, &id).await?.name != "Native recording smoke" {
        return Err("Native studio title was not saved.".into());
    }
    click("document.querySelector('.record-add-card')").await?;
    click("Array.from(document.querySelectorAll('.record-toolbar button')).find(b=>b.textContent.trim()==='Save changes')").await?;
    wait("return !document.querySelector('.record-toolbar .save-status').classList.contains('dirty');",100).await?;
    eprintln!("Storyfold native check: hold recording");
    // Hold and release saves one short clip; release is delivered through native IPC.
    pointer("pointerdown").await?;
    tokio::time::sleep(Duration::from_millis(850)).await;
    pointer("pointerup").await?;
    eprintln!("Storyfold native check: first take finalizing");
    let first = wait_takes(&api, &id, 1).await?;
    if first.clips.len() != 1 || first.takes[0].duration <= 0.1 {
        return Err("Hold did not create one valid sequence clip.".into());
    }
    wait(
        "return !document.getElementById('record-explicit-start').disabled;",
        100,
    )
    .await?;
    // A quick tap stays active until the explicit accessible Stop action.
    pointer("pointerdown").await?;
    tokio::time::sleep(Duration::from_millis(25)).await;
    pointer("pointerup").await?;
    wait(
        "return document.querySelector('.record-live-badge')?.textContent.includes('long take');",
        100,
    )
    .await?;
    wait("const img=document.querySelector('.record-live-image');return !!img&&img.naturalWidth>=1280&&img.src.endsWith('/preview.mjpeg');",100).await?;
    let live_width = dom::eval(
        "return document.querySelector('.record-live-image').naturalWidth;",
        None,
    )
    .await?
    .as_u64()
    .unwrap_or_default();
    let streamed_frames = stream_frames(&api, &id).await?;
    if streamed_frames < 3 {
        return Err("Live preview did not deliver multiple frames on one stream.".into());
    }
    click("document.querySelector('.record-overlay-show')").await?;
    wait(
        "return document.querySelector('.record-overlay-card').classList.contains('active');",
        100,
    )
    .await?;
    click("Array.from(document.querySelectorAll('.header nav button')).find(b=>b.textContent.trim()==='New project')").await?;
    if !js_bool("return document.querySelector('.header nav button.active')?.textContent.trim()==='Record';").await?{return Err("Native recording could be hidden by navigation.".into());}
    tokio::time::sleep(Duration::from_millis(500)).await;
    if session(&api, &id).await?.active.is_none() {
        return Err("A tap incorrectly stopped the long take on release.".into());
    }
    click("document.getElementById('record-explicit-stop')").await?;
    eprintln!("Storyfold native check: second take finalizing");
    let second = wait_takes(&api, &id, 2).await?;
    if !second.takes[1].marks.iter().any(|m| m.overlay_id.is_some()) {
        return Err("Native overlay trigger did not save a timing mark.".into());
    }
    wait(
        "return !document.getElementById('record-explicit-start').disabled;",
        100,
    )
    .await?;
    wait(
        "return document.querySelectorAll('.record-sequence-clip').length===2;",
        100,
    )
    .await?;
    click("Array.from(document.querySelectorAll('.record-clip-tools button')).find(b=>b.textContent.trim()==='Split')").await?;
    wait(
        "return document.querySelectorAll('.record-sequence-clip').length===3;",
        30,
    )
    .await?;
    click("Array.from(document.querySelectorAll('.record-clip-tools button')).find(b=>b.textContent.trim()==='Move left')").await?;
    wait("return document.querySelector('.record-toolbar .save-status').classList.contains('dirty');",30).await?;
    eprintln!("Storyfold native check: preparing sequence");
    let before_preview = session(&api, &id).await?;
    if before_preview.export.is_some() {
        return Err(
            "Native first Preview check requires an unprepared generated recording.".into(),
        );
    }
    click("document.getElementById('record-preview-sequence')").await?;
    wait(
        "return !!document.getElementById('record-sequence-progress');",
        30,
    )
    .await?;
    eprintln!("Storyfold native check: waiting for render");
    let rendered = wait_prepared(&api, &id, None).await?;
    if rendered.clips.len() != 3
        || rendered.takes.len() != 2
        || rendered.revision <= before_preview.revision
    {
        return Err(
            "Preview sequence did not automatically save dirty edits and preserve source takes."
                .into(),
        );
    }
    wait("const v=document.getElementById('record-sequence-preview');return !!v&&v.currentSrc.includes('/exports/')&&v.readyState>=2&&v.duration>0;",100).await?;
    wait("const v=document.getElementById('record-sequence-preview');return !!v&&!v.paused&&v.currentTime>0.05;",100).await?;
    click("document.getElementById('record-expand-preview')").await?;
    wait("return !!document.querySelector('.record-preview-expanded')&&document.getElementById('record-sequence-preview')?.readyState>=2;",30).await?;
    let expanded_width = dom::eval(
        "return document.getElementById('record-sequence-preview').getBoundingClientRect().width;",
        None,
    )
    .await?
    .as_f64()
    .unwrap_or(0.);
    if expanded_width < 900. {
        return Err("Expanded sequence preview did not grow.".into());
    }
    if !js_bool("const b=document.getElementById('record-expand-preview');b.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));return true;").await?{return Err("Expanded Escape unavailable.".into());}
    wait(
        "return !document.querySelector('.record-preview-expanded');",
        30,
    )
    .await?;
    eprintln!("Storyfold native check: sequence playing");
    let first_export = rendered.export.as_ref().unwrap().id.clone();
    wait(
        "return !document.getElementById('record-preview-sequence').disabled;",
        100,
    )
    .await?;
    click("document.getElementById('record-preview-sequence')").await?;
    wait(
        "return !document.getElementById('record-preview-sequence').disabled;",
        100,
    )
    .await?;
    let cached = session(&api, &id).await?;
    if cached.export.as_ref().map(|j| &j.id) != Some(&first_export) {
        return Err("An unchanged sequence was unnecessarily prepared again.".into());
    }
    if !js_bool("const label=Array.from(document.querySelectorAll('.record-clip-tools label')).find(l=>l.textContent.startsWith('Out (source seconds)'));const input=label?.querySelector('input');if(!input)return false;input.value=(Number(input.value)-0.12).toFixed(2);input.dispatchEvent(new Event('change',{bubbles:true}));return true;").await? {return Err("Native trim field is missing.".into());}
    wait("return !document.getElementById('record-sequence-preview')&&document.querySelector('.record-toolbar .save-status').classList.contains('dirty');",50).await?;
    click("document.getElementById('record-preview-sequence')").await?;
    let rendered = wait_prepared(&api, &id, Some(&first_export)).await?;
    wait("const v=document.getElementById('record-sequence-preview');return !!v&&v.readyState>=2&&v.currentSrc.includes('/exports/');",100).await?;
    let duration: f64 = rendered.clips.iter().map(|c| c.out - c.r#in).sum();
    let actual = dom::eval(
        "return document.getElementById('record-sequence-preview')?.duration||0;",
        None,
    )
    .await?
    .as_f64()
    .unwrap_or_default();
    if (duration - actual).abs() > 0.15 {
        return Err("The preview duration does not match the complete edited sequence.".into());
    }
    let mut export_covered = false;
    if let Some(path) = std::env::var_os("STORYFOLD_RECORD_EXPORT_PATH") {
        wait(
            "return !document.getElementById('record-export-mp4').disabled;",
            100,
        )
        .await?;
        click("document.getElementById('record-export-mp4')").await?;
        wait("return Array.from(document.querySelectorAll('.record-sequence-actions [role=status]')).some(e=>e.textContent.includes('MP4 exported'));",300).await?;
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|_| "Native Export MP4 did not write the supplied test destination.")?;
        if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
            return Err("Native Export MP4 did not save a valid MP4 file.".into());
        }
        let after = session(&api, &id).await?;
        if after.export.as_ref().map(|j| &j.id) != rendered.export.as_ref().map(|j| &j.id) {
            return Err("Export MP4 did not reuse the current prepared sequence.".into());
        }
        export_covered = true;
    }
    // Switching views preserves a studio draft and does not affect transcript/source state.
    input("#record-name", "Native draft preserved").await?;
    click("Array.from(document.querySelectorAll('.header nav button')).find(b=>b.textContent.trim()==='Your build')").await?;
    click("Array.from(document.querySelectorAll('.header nav button')).find(b=>b.textContent.trim()==='Record')").await?;
    if !js_bool("return document.getElementById('record-name').value==='Native draft preserved';")
        .await?
    {
        return Err("Native studio draft was lost on navigation.".into());
    }
    click("Array.from(document.querySelectorAll('.record-toolbar button')).find(b=>b.textContent.trim()==='Reload saved')").await?;
    Ok(
        json!({"syntheticOnly":true,"freshStartup":true,"projectsTab":true,"savedProjectNotAutoOpened":true,"immediateProgressVisible":true,"expandedPreview":true,"expandedEscape":true,"sessionId":id,"nativeNavigation":true,"titleSaveReadback":true,"holdClip":true,"tapLongTake":true,"explicitStop":true,"overlayTimingMark":true,"captureNavigationGuard":true,"sequenceSplitReorder":true,"sourceTakesPreserved":true,"renderedMp4Ready":true,"previewFirstUseAutoSave":true,"previewCachedCurrent":true,"stalePreviewInvalidated":true,"fullSequenceDuration":duration,"previewNaturalWidth":live_width,"streamFrames":streamed_frames,"exportButtonCovered":export_covered,"draftSurvivesNavigation":true,"youtubeImportControls":true,"youtubePrivateDefault":true,"youtubeUploadGuard":true,"youtubeConnectSetupControls":true,"takes":rendered.takes.len(),"clips":rendered.clips.len()}),
    )
}
