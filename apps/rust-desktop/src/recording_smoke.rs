//! Opt-in native studio test. Refuses every camera except the debug synthetic source.
use crate::{State, api::Api, dom};
use dioxus::prelude::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use storyfold_core::recording::{RecordingDevices, RecordingSession};

pub async fn run(mut state: Signal<State>, destination: PathBuf) {
    let result = exercise().await;
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
async fn exercise() -> Result<Value, String> {
    let api = consume_context::<Api>();
    let devices: RecordingDevices = api.get("/recording-devices").await?;
    if !devices.cameras.iter().any(|d| d.id == "synthetic")
        || devices.cameras.iter().any(|d| d.id != "synthetic")
    {
        return Err("Native recording test requires an isolated backend with only the debug synthetic camera. No hardware was opened.".into());
    }
    tokio::time::sleep(Duration::from_millis(700)).await;
    click("Array.from(document.querySelectorAll('.header nav button')).find(b=>b.textContent.trim()==='Record')").await?;
    wait(
        "return !document.querySelector('.record-studio').closest('[hidden]');",
        50,
    )
    .await?;
    wait("return Array.from(document.querySelectorAll('#record-camera option')).some(o=>o.value==='synthetic')||document.querySelector('.record-welcome');",100).await?;
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
    // Hold and release saves one short clip; release is delivered through native IPC.
    pointer("pointerdown").await?;
    tokio::time::sleep(Duration::from_millis(850)).await;
    pointer("pointerup").await?;
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
    click("document.querySelector('.record-overlay-show')").await?;
    wait(
        "return document.querySelector('.record-overlay-card').classList.contains('active');",
        100,
    )
    .await?;
    click("Array.from(document.querySelectorAll('.header nav button')).find(b=>b.textContent.trim()==='Interviews')").await?;
    if !js_bool("return document.querySelector('.header nav button.active')?.textContent.trim()==='Record';").await?{return Err("Native recording could be hidden by navigation.".into());}
    tokio::time::sleep(Duration::from_millis(500)).await;
    if session(&api, &id).await?.active.is_none() {
        return Err("A tap incorrectly stopped the long take on release.".into());
    }
    click("document.getElementById('record-explicit-stop')").await?;
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
    click("Array.from(document.querySelectorAll('.record-toolbar button')).find(b=>b.textContent.trim()==='Save changes')").await?;
    wait("return !document.querySelector('.record-toolbar .save-status').classList.contains('dirty');",100).await?;
    let edited = session(&api, &id).await?;
    if edited.clips.len() != 3 || edited.takes.len() != 2 {
        return Err("Native sequence split/reorder did not preserve its source takes.".into());
    }
    click("document.getElementById('record-render')").await?;
    let mut rendered = None;
    for _ in 0..900 {
        let s = session(&api, &id).await?;
        if let Some(job) = &s.export {
            if job.status == "done" {
                rendered = Some(s);
                break;
            }
            if job.status == "error" {
                return Err(job
                    .error
                    .clone()
                    .unwrap_or_else(|| "Native recording render failed.".into()));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let rendered = rendered.ok_or("Native recording render did not finish.")?;
    wait("return Array.from(document.querySelectorAll('.record-export button')).some(b=>b.textContent.trim()==='Preview rendered video');",100).await?;
    click("Array.from(document.querySelectorAll('.record-export button')).find(b=>b.textContent.trim()==='Preview rendered video')").await?;
    wait("const v=document.querySelector('.record-clip-preview');return !!v&&v.currentSrc.includes('/exports/')&&v.readyState>=2&&v.duration>0;",100).await?;
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
        json!({"syntheticOnly":true,"sessionId":id,"nativeNavigation":true,"titleSaveReadback":true,"holdClip":true,"tapLongTake":true,"explicitStop":true,"overlayTimingMark":true,"captureNavigationGuard":true,"sequenceSplitReorder":true,"sourceTakesPreserved":true,"renderedMp4Ready":true,"draftSurvivesNavigation":true,"youtubeImportControls":true,"youtubePrivateDefault":true,"youtubeUploadGuard":true,"youtubeConnectSetupControls":true,"takes":rendered.takes.len(),"clips":rendered.clips.len()}),
    )
}
