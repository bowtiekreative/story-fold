//! Explicit read-only native-player diagnostic. Never captures, edits or exports media.
use crate::{State, api::Api, dom};
use dioxus::prelude::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

pub async fn run(mut state: Signal<State>, destination: PathBuf) {
    let result = tokio::time::timeout(Duration::from_secs(45), exercise(state)).await;
    let artifact = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => json!({"ok":false,"error":error}),
        Err(_) => json!({"ok":false,"error":"Native playback probe timed out"}),
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
async fn exercise(state: Signal<State>) -> Result<Value, String> {
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
    if std::env::var_os("STORYFOLD_PREVIEW_PROBE_STUDIO").is_some() {
        if export.revision != session.revision {
            return Err("Studio probe requires an already current export".into());
        }
        return studio_preview(state, id).await;
    }
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

async fn studio_preview(state: Signal<State>, id: String) -> Result<Value, String> {
    crate::navigate(state, crate::Destination::Recording(Some(id)));
    for _ in 0..150 {
        if dom::eval("const b=document.getElementById('record-preview-sequence');if(!b||b.disabled)return false;b.click();return true;",None).await? == true {break;}
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for _ in 0..150 {
        if dom::eval("const v=document.getElementById('record-sequence-preview');return !!v&&v.readyState>=2&&!v.paused&&v.currentTime>0.1;",None).await? == true {break;}
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    dom::eval("const v=document.getElementById('record-sequence-preview');if(!v)throw Error('Sequence player missing');window.storyfoldProbeVideo=v;v.loop=true;v.storyfoldProbeSource=v.currentSrc;v.storyfoldProbeLoads=0;v.addEventListener('loadstart',()=>v.storyfoldProbeLoads++);return true;",None).await?;
    let pause_hit = pointer_control("record-toggle-playback").await?;
    let paused_ack = wait_playback(true).await?;
    let paused = player_snapshot().await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let paused_still = player_snapshot().await?;
    let play_hit = pointer_control("record-toggle-playback").await?;
    let playing_ack = wait_playback(false).await?;
    let before = player_snapshot().await?;
    let enter = pointer_expand().await?;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let expanded = player_snapshot().await?;
    let exit = pointer_expand().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let compact = player_snapshot().await?;
    let reenter = pointer_expand().await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let second_expanded = player_snapshot().await?;
    dom::eval("const b=document.getElementById('record-expand-preview');b.focus();b.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));return true;",None).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let escape = player_snapshot().await?;
    let frames = |v: &Value| v["frames"].as_u64().unwrap_or(0);
    let all_playing = [&expanded, &compact, &second_expanded, &escape]
        .iter()
        .all(|v| {
            v["sameVideo"] == true
                && v["sameSource"] == true
                && v["reloads"] == 0
                && v["paused"] == false
                && v["buttonVisible"] == true
        });
    let ok = pause_hit["hit"] == true
        && play_hit["hit"] == true
        && paused_ack
        && playing_ack
        && paused["sameVideo"] == true
        && paused["paused"] == true
        && (paused_still["time"].as_f64().unwrap_or(0.) - paused["time"].as_f64().unwrap_or(0.))
            .abs()
            < 0.05
        && enter["hit"] == true
        && exit["hit"] == true
        && reenter["hit"] == true
        && expanded["expanded"] == true
        && second_expanded["expanded"] == true
        && compact["expanded"] == false
        && escape["expanded"] == false
        && expanded["controlOutsideVideo"] == true
        && expanded["width"].as_f64().unwrap_or(0.) > 900.
        && expanded["height"].as_f64().unwrap_or(0.) > 500.
        && all_playing
        && frames(&expanded) > frames(&before)
        && frames(&compact) > frames(&expanded)
        && frames(&escape) > frames(&second_expanded);
    Ok(
        json!({"ok":ok,"readOnly":true,"studioControls":true,"pointerHitPause":pause_hit,"pauseAcknowledged":paused_ack,"paused":paused,"pausedStill":paused_still,"pointerHitPlay":play_hit,"playAcknowledged":playing_ack,"pointerHitEnter":enter,"pointerHitExit":exit,"pointerHitReenter":reenter,"before":before,"expanded":expanded,"compact":compact,"secondExpanded":second_expanded,"escape":escape}),
    )
}

async fn pointer_expand() -> Result<Value, String> {
    pointer_control("record-expand-preview").await
}

async fn pointer_control(id: &str) -> Result<Value, String> {
    dom::eval("const id=await dioxus.recv();const b=document.getElementById(id);if(!b)throw Error('Player control missing');b.scrollIntoView({block:'center'});const r=b.getBoundingClientRect();const target=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);const hit=!!target&&b.contains(target);if(hit){b.focus();for(const type of ['pointerdown','pointerup','click'])target.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerId:481,isPrimary:true,pointerType:'mouse',button:0,buttons:type==='pointerdown'?1:0,clientX:r.x+r.width/2,clientY:r.y+r.height/2}));}return {hit,element:target?.tagName,button:b.textContent};",Some(json!(id))).await
}

async fn wait_playback(paused: bool) -> Result<bool, String> {
    for _ in 0..20 {
        if dom::eval("const paused=await dioxus.recv();const v=document.getElementById('record-sequence-preview'),b=document.getElementById('record-toggle-playback');return !!v&&v.paused===paused&&b?.textContent.includes(paused?'Play':'Pause');",Some(json!(paused))).await? == true {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(false)
}

async fn player_snapshot() -> Result<Value, String> {
    dom::eval("const v=document.getElementById('record-sequence-preview');if(!v)return null;const raw=v.getVideoPlaybackQuality?.().totalVideoFrames??v.webkitDecodedFrameCount??0;const previous=v.storyfoldProbeFrames??{raw:0,total:0};const frames=previous.total+(raw>=previous.raw?raw-previous.raw:raw);v.storyfoldProbeFrames={raw,total:frames};const r=v.getBoundingClientRect(),b=document.getElementById('record-expand-preview'),br=b.getBoundingClientRect();return {sameVideo:v===window.storyfoldProbeVideo,sameSource:v.currentSrc===v.storyfoldProbeSource,reloads:v.storyfoldProbeLoads,expanded:!!document.querySelector('.record-preview-expanded'),time:v.currentTime,paused:v.paused,width:r.width,height:r.height,viewportWidth:innerWidth,viewportHeight:innerHeight,buttonVisible:br.y>=0&&br.bottom<=innerHeight,controlOutsideVideo:!document.getElementById('record-preview-frame').contains(b),playButton:document.getElementById('record-toggle-playback')?.textContent,nativeControls:v.controls,error:v.error?.code||0,rawDecodedFrames:raw,frames};",None).await
}
