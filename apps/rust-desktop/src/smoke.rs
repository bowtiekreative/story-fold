//! Opt-in native application test driver. It only creates and edits a fresh synthetic sample.
use crate::{State, api::Api, dom};
use dioxus::prelude::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

pub async fn run(mut state: Signal<State>, destination: PathBuf) {
    let result = exercise(state).await;
    let document = dom::eval("return document.documentElement.outerHTML;", None)
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned));
    let artifact = match result {
        Ok(evidence) => {
            json!({"ok":true,"runtime":"Rust/Dioxus desktop","checks":evidence,"html":document})
        }
        Err(error) => {
            json!({"ok":false,"runtime":"Rust/Dioxus desktop","error":error,"html":document})
        }
    };
    if let Some(parent) = destination.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&artifact) {
        let _ = tokio::fs::write(destination, bytes).await;
    }
    if std::env::var_os("STORYFOLD_SMOKE_EXIT").is_some() {
        state.write().pending = None;
        let window = dioxus::desktop::window();
        window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
        window.close();
    }
}
async fn exercise(state: Signal<State>) -> Result<Value, String> {
    tokio::time::sleep(Duration::from_millis(700)).await;
    let library=click("Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()==='Try the sample')").await?;
    if !library {
        return Err("Native library did not expose the sample action.".into());
    }
    wait(
        || {
            state
                .peek()
                .current
                .as_ref()
                .is_some_and(|d| d.project.status == "ready")
        },
        180,
    )
    .await?;
    let project = state
        .peek()
        .current
        .as_ref()
        .ok_or("Sample did not open.")?
        .project
        .clone();
    if !project.sample {
        return Err("Test driver refused to edit a non-sample project.".into());
    }
    let original = project
        .sentences
        .first()
        .ok_or("Sample transcript is empty.")?
        .text
        .clone();
    let edited = format!("{original} [native UI smoke]");
    if !click("document.querySelector('.topic-actions button')").await? {
        return Err("Topic append action is missing.".into());
    }
    tokio::time::sleep(Duration::from_millis(120)).await;
    let edited_result = dom::eval(
        "const text=await dioxus.recv();const input=document.getElementById('sentence-text');if(!input)return false;input.value=text;input.dispatchEvent(new Event('input',{bubbles:true}));return true;",
        Some(json!(&edited))).await;
    if edited_result.ok() != Some(json!(true)) {
        return Err("Sentence editor did not mount.".into());
    }
    wait(
        || {
            state.peek().current.as_ref().is_some_and(|d| {
                d.project
                    .sentences
                    .first()
                    .is_some_and(|s| s.text == edited)
                    && d.dirty()
            })
        },
        10,
    )
    .await?;
    if !click("Array.from(document.querySelectorAll('.editor-toolbar button')).find(b=>b.textContent.trim()==='Save')").await?{return Err("Save action is missing.".into());}
    wait(
        || {
            state
                .peek()
                .current
                .as_ref()
                .is_some_and(|d| !d.dirty() && d.project.revision > project.revision)
        },
        30,
    )
    .await?;
    let api = consume_context::<Api>();
    let saved: Value = api.get(&format!("/projects/{}", project.id)).await?;
    if saved["sentences"][0]["text"] != edited
        || saved["timeline"].as_array().map(Vec::len) != Some(project.timeline.len() + 1)
    {
        return Err("Native edit/save did not match persisted project data.".into());
    }
    click("Array.from(document.querySelectorAll('.editor-toolbar button')).find(b=>b.textContent.trim()==='Undo')").await?;
    wait(
        || {
            state.peek().current.as_ref().is_some_and(|d| {
                d.project
                    .sentences
                    .first()
                    .is_some_and(|s| s.text == original)
            })
        },
        10,
    )
    .await?;
    click("Array.from(document.querySelectorAll('.editor-toolbar button')).find(b=>b.textContent.trim()==='Redo')").await?;
    wait(
        || {
            state.peek().current.as_ref().is_some_and(|d| {
                d.project
                    .sentences
                    .first()
                    .is_some_and(|s| s.text == edited)
            })
        },
        10,
    )
    .await?;
    click("Array.from(document.querySelectorAll('.editor-toolbar button')).find(b=>b.textContent.trim()==='Save')").await?;
    wait(
        || state.peek().current.as_ref().is_some_and(|d| !d.dirty()),
        30,
    )
    .await?;
    let media = dom::eval(
        "return ['master-audio','camera-0','camera-1'].every(id=>!!document.getElementById(id));",
        None,
    )
    .await
    .ok()
        == Some(json!(true));
    if !media {
        return Err("Independent master audio and camera elements are missing.".into());
    }
    if !click("document.querySelector('.transport button')").await? {
        return Err("Native preview play action is missing.".into());
    }
    let mut decoding = false;
    let initial_audio = project.sentences.first().map(|s| s.r#in).unwrap_or(0.);
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let result = dom::eval(
            "const start=await dioxus.recv();const a=document.getElementById('master-audio');const videos=['camera-0','camera-1'].map(id=>document.getElementById(id));const v=videos.find(v=>v&&v.style.opacity==='1');return !!a&&!!v&&a.readyState>=2&&v.readyState>=2&&!a.error&&!v.error&&!a.paused&&a.currentTime>start+0.3&&v.currentTime>0.1;",
            Some(json!(initial_audio))).await;
        decoding = result.ok() == Some(json!(true));
        if decoding {
            break;
        }
    }
    if !decoding {
        let detail=dom::eval("const a=document.getElementById('master-audio');return {audio:a?{ready:a.readyState,time:a.currentTime,paused:a.paused,error:a.error?.code}:null,previewError:document.querySelector('.player .error')?.textContent};",None).await.ok();
        return Err(format!(
            "Native H.264/AAC preview did not advance: {}",
            detail.unwrap_or(Value::Null)
        ));
    }
    click("document.querySelector('.transport button')").await?;
    click("Array.from(document.querySelectorAll('header nav button')).find(b=>b.textContent.trim()==='Your build')").await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let build=dom::eval("return !!document.querySelector('.build .build-tabs') && Array.from(document.querySelectorAll('.build button')).some(b=>b.textContent.trim()==='Run build');",None).await.ok()==Some(json!(true));
    if !build {
        return Err("Native build control panel did not open.".into());
    }
    click("Array.from(document.querySelectorAll('header nav button')).find(b=>b.textContent.trim()==='Editor')").await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    // Leave the test-owned sample in its original, useful demo state.
    crate::change(state, |p| {
        p.sentences[0].text = original;
        p.timeline = project.timeline.clone();
    });
    crate::save(state, api).await?;
    Ok(
        json!({"library":true,"sampleProcessing":true,"sentenceEdit":true,"appendTopic":true,"saveReadback":true,"undoRedo":true,"independentMediaElements":media,"nativeH264AacPlayback":decoding,"buildNavigation":build,"projectId":project.id,"revision":saved["revision"]}),
    )
}
async fn click(expression: &str) -> Result<bool, String> {
    dom::eval(&format!("const button={expression};if(!button||button.disabled)return false;button.click();return true;"),None)
        .await.map(|v|v==true).map_err(|error|format!("Native WebView action {expression} failed: {error:?}"))
}
async fn wait(mut condition: impl FnMut() -> bool, seconds: u64) -> Result<(), String> {
    for _ in 0..seconds * 10 {
        if condition() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("Native UI action did not reach its expected state before the test timeout.".into())
}
