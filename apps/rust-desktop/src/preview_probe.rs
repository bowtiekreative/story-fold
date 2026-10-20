//! Explicit read-only native-player diagnostic. Never captures, edits or exports media.
use crate::{State, api::Api, dom};
use dioxus::prelude::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

pub async fn run(mut state: Signal<State>, destination: PathBuf) {
    let result = exercise(state).await;
    let artifact = match result {
        Ok(value) => value,
        Err(_) => json!({"ok":false,"error":"Native playback probe did not complete"}),
    };
    if let Some(parent) = destination.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(destination, serde_json::to_vec_pretty(&artifact).unwrap()).await;
    if std::env::var_os("STORYFOLD_SMOKE_EXIT").is_some() && !state.peek().recording_capture {
        state.write().pending = None;
        let window = dioxus::desktop::window();
        window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
        window.close();
    }
}
async fn exercise(_state: Signal<State>) -> Result<Value, String> {
    let id =
        std::env::var("STORYFOLD_PREVIEW_PROBE_ID").map_err(|_| "Missing diagnostic project")?;
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid diagnostic project")?;
    let api = Api::new(
        &std::env::var("STORYFOLD_SERVER").unwrap_or_else(|_| "http://127.0.0.1:4311".into()),
    )?;
    let session: storyfold_core::recording::RecordingSession =
        api.get(&format!("/recordings/{id}")).await?;
    if session.active.is_some() {
        return Err("Probe refuses active recording".into());
    }
    let orientation = session.orientation.clone();
    let export = session
        .export
        .filter(|j| j.status == "done")
        .ok_or("Probe requires an existing completed MP4")?;
    let url = api.url(&format!("/recordings/{id}/media/exports/{}", export.id));
    // Probe decoding independently of revision selection; never rebuild a stale export.
    for _ in 0..100 {
        if dom::eval("return !!document.querySelector('.header');", None).await? == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    dom::eval("const [url,orientation]=await dioxus.recv();const v=document.createElement('video');v.id='record-sequence-preview';v.controls=true;v.muted=true;v.preload='auto';v.className='record-clip-preview';const column=document.createElement('div');column.style.cssText='position:fixed;inset:40px;width:min(740px,calc(100vw - 80px));z-index:1000';const card=document.createElement('div');card.className='record-camera-card';const frame=document.createElement('div');frame.className='record-frame'+(orientation==='9:16'?' portrait':'');frame.append(v);card.append(frame);column.append(card);document.body.append(column);v.src=url;try{await v.play();}catch(_){}return true;",Some(json!([url,orientation]))).await?;
    let mut snapshot = Value::Null;
    for _ in 0..150 {
        snapshot=dom::eval("const v=document.getElementById('record-sequence-preview');if(!v)return {mounted:false};const r=v.getBoundingClientRect(),s=getComputedStyle(v);return {mounted:true,ready:v.readyState,network:v.networkState,error:v.error?.code||0,width:v.videoWidth,height:v.videoHeight,displayWidth:r.width,displayHeight:r.height,opacity:s.opacity,hidden:!!v.closest('[hidden]'),time:v.currentTime,paused:v.paused,frames:v.getVideoPlaybackQuality?.().totalVideoFrames??v.webkitDecodedFrameCount??null};",None).await?;
        if snapshot["error"].as_u64().unwrap_or(0) > 0
            || snapshot["time"].as_f64().unwrap_or(0.) > 0.2
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let ok = snapshot["ready"].as_u64().unwrap_or(0) >= 2
        && snapshot["width"].as_u64().unwrap_or(0) > 0
        && snapshot["displayHeight"].as_f64().unwrap_or(0.) > 0.
        && snapshot["time"].as_f64().unwrap_or(0.) > 0.2;
    Ok(json!({"ok":ok,"readOnly":true,"directExistingMp4":true,"muted":true,"player":snapshot}))
}
