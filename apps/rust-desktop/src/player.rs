use crate::{api::Api, model::*};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playback {
    pub time: f64,
    pub playing: bool,
    pub story: bool,
    pub error: String,
}
#[derive(Deserialize)]
struct Clock {
    time: f64,
    ready: bool,
    seeking: bool,
    paused: bool,
    #[serde(default)]
    error: bool,
}
#[derive(Clone, PartialEq, Serialize)]
struct Command {
    video: String,
    audio: String,
    key: String,
    video_time: f64,
    audio_time: f64,
    playing: bool,
    transform: String,
    next_video: Option<String>,
    next_time: f64,
}

#[component]
pub fn Player(
    project: Rc<Project>,
    selected_id: String,
    mut playback: Signal<Playback>,
) -> Element {
    let api = use_context::<Api>();
    let mut expanded = use_signal(|| false);
    let latest = use_hook(|| {
        Rc::new(RefCell::new((
            project.clone(),
            Rc::new(Sequence::new(&project)),
            selected_id.clone(),
        )))
    });
    {
        let mut cached = latest.borrow_mut();
        if !Rc::ptr_eq(&cached.0, &project) {
            cached.1 = Rc::new(Sequence::new(&project));
            cached.0 = project.clone();
        }
        cached.2 = selected_id.clone();
    }
    let sq = latest.borrow().1.clone();
    use_future(move || {
        let latest = latest.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(if playback.peek().playing {
                    40
                } else {
                    160
                }))
                .await;
                let intent = playback.peek().clone();
                if !intent.playing {
                    continue;
                }
                let result=crate::dom::eval(r#"const a=document.getElementById('master-audio');return a?{time:a.currentTime||0,ready:a.readyState>=2,seeking:a.seeking,paused:a.paused,error:!!a.error}:null;"#,None).await;
                let Ok(value) = result else {
                    continue;
                };
                let Ok(clock) = serde_json::from_value::<Clock>(value) else {
                    continue;
                };
                if clock.error {
                    let mut p = playback.write();
                    p.playing = false;
                    p.error="The preview audio could not be decoded. Check the local media service, then retry.".into();
                    continue;
                }
                if !clock.ready || clock.seeking || clock.paused {
                    continue;
                }
                let (project, sq, id) = latest.borrow().clone();
                let selected = project.sentences.iter().find(|s| s.id == id);
                let (next, length) = if intent.story {
                    let (audio, _) = sq.frame(intent.time);
                    let Some(audio) = audio else {
                        playback.write().playing = false;
                        continue;
                    };
                    (audio.a_start + clock.time - audio.sentence.r#in, sq.total)
                } else if let Some(sentence) = selected {
                    (clock.time - sentence.r#in, sentence.out - sentence.r#in)
                } else {
                    (0., 0.)
                };
                // Ignore stale clock replies after a seek, pause or selection change.
                if playback.peek().playing != intent.playing
                    || (playback.peek().time - intent.time).abs() > 0.001
                    || playback.peek().story != intent.story
                {
                    continue;
                }
                if next >= length - 0.005 {
                    let mut p = playback.write();
                    p.time = length.max(0.);
                    p.playing = false;
                } else if next >= intent.time - 0.08 {
                    playback.write().time = next.max(0.);
                }
            }
        }
    });
    use_drop(move || {
        let _ = document::eval(
            "for(const id of ['master-audio','camera-0','camera-1']){document.getElementById(id)?.pause();}",
        );
    });
    let play = playback.read().clone();
    let (audio_clip, video_clip) = sq.frame(play.time);
    let selected = project.sentences.iter().find(|s| s.id == selected_id);
    let sentence = if play.story {
        video_clip.map(|c| &c.sentence)
    } else {
        selected
    };
    let audio_sentence = if play.story {
        audio_clip.map(|c| &c.sentence)
    } else {
        selected
    };
    let camera = sentence
        .and_then(|s| project.cameras.iter().find(|c| c.id == s.cam))
        .or(project.cameras.first());
    let audio_time = if play.story {
        audio_clip
            .map(|c| c.sentence.r#in + play.time - c.a_start)
            .unwrap_or(0.)
    } else {
        selected.map(|s| s.r#in + play.time).unwrap_or(0.)
    };
    let video_time = if play.story {
        video_clip
            .map(|c| c.sentence.r#in + play.time - c.v_start)
            .unwrap_or(0.)
    } else {
        audio_time
    };
    let length = if play.story {
        sq.total
    } else {
        selected.map(|s| s.out - s.r#in).unwrap_or(0.)
    };
    let crop = camera.and_then(|c| c.virtual_crop.as_ref());
    let transform = crop
        .map(|c| {
            format!(
                "scale({}) translate({}%,{}%)",
                c.scale,
                (0.5 - c.x) * 40.,
                (0.5 - c.y) * 40.
            )
        })
        .unwrap_or_default();
    let key = if play.story {
        video_clip
            .map(|c| format!("{}:{}:{}", c.item_index, c.v_start, c.sentence.cam))
            .unwrap_or_default()
    } else {
        format!(
            "sentence:{selected_id}:{}",
            camera.map(|c| c.id.as_str()).unwrap_or("A")
        )
    };
    let next = video_clip
        .and_then(|c| sq.clips.iter().position(|v| std::ptr::eq(v, c)))
        .and_then(|i| sq.clips.get(i + 1));
    let next_camera =
        next.and_then(|c| project.cameras.iter().find(|cam| cam.id == c.sentence.cam));
    let command = Command {
        video: api.url(&format!(
            "/projects/{}/media/{}",
            project.id,
            camera.map(|c| c.id.as_str()).unwrap_or("A")
        )),
        audio: api.url(&format!("/projects/{}/media/A", project.id)),
        key,
        video_time: (video_time + camera.map(|c| c.offset).unwrap_or(0.)).max(0.),
        audio_time: audio_time.max(0.),
        playing: play.playing && sentence.is_some(),
        transform,
        next_video: if play.story {
            next_camera.map(|cam| api.url(&format!("/projects/{}/media/{}", project.id, cam.id)))
        } else {
            None
        },
        next_time: next
            .map(|c| c.sentence.r#in + next_camera.map(|cam| cam.offset).unwrap_or(0.))
            .unwrap_or(0.)
            .max(0.),
    };
    // Eval only transports Rust-computed media commands. Text, API keys, editing and timing logic stay in Rust.
    use_effect(use_reactive((&command,), move |(command,)| {
        spawn(async move {
            if let Ok(value) = crate::dom::eval(
                include_str!("transport.js"),
                Some(serde_json::json!(command)),
            )
            .await
                && let Some(error) = value.as_str().filter(|s| !s.is_empty())
            {
                let mut p = playback.write();
                p.playing = false;
                p.error = error.into();
            }
        });
    }));
    let lcut = play.story
        && audio_clip
            .zip(video_clip)
            .is_some_and(|(a, v)| a.a_start != v.a_start || a.item_index != v.item_index);
    rsx! {
        section {class:if *expanded.read(){"player preview-expanded"}else{"player"},"aria-label":"Story preview",onkeydown:move |e|if e.key()==Key::Escape&&*expanded.peek(){e.prevent_default();expanded.set(false);},
            div {id:"story-preview-frame",class:"viewer",style:format!("aspect-ratio:{}",project.export_settings.ratio.replace(':',"/")),
                button {id:"story-expand-preview",class:"preview-expand","aria-controls":"story-preview-frame","aria-expanded":*expanded.read(),onclick:move |_|expanded.toggle(),if *expanded.read(){"Exit expanded view"}else{"Expand preview"}}
                video {id:"camera-0",muted:true,playsinline:true,preload:"auto","aria-label":"Camera preview"}
                video {id:"camera-1",muted:true,playsinline:true,preload:"auto","aria-hidden":"true"}
                audio {id:"master-audio",preload:"auto"}
                if sentence.is_some() {
                    span {class:"camera-badge","CAM " {camera.map(|c|c.id.as_str()).unwrap_or("A")}}
                    if project.export_settings.captions {div {class:"caption",{audio_sentence.map(|s|s.text.as_str()).unwrap_or("")}}}
                    if lcut {span {class:"lcut","L-cut · master audio carries over"}}
                } else {div {class:"empty-view",strong {"A story starts with a sentence."}span {"Select a sentence, or add a topic folder to the timeline."}}}
            }
            div {class:"transport",
                button {"aria-label":if play.playing {"Pause preview"}else{"Play preview"},disabled:length<=0.,onclick:move |_|{let mut p=playback.write();if p.time>=length-0.03{p.time=0.;}p.playing= !p.playing;p.error.clear();},if play.playing {"Pause"}else{"Play"}}
                input {r#type:"range",min:0,max:length.max(0.01),step:0.01,value:play.time,"aria-label":"Preview position",oninput:move |e|if let Ok(t)=e.value().parse::<f64>(){playback.write().time=t;}}
                span {class:"mono","{fmt(play.time)} / {fmt(length)}"}
            }
            if !play.error.is_empty() {p {class:"error",role:"alert","{play.error}"}}
        }
    }
}
