use crate::{
    Destination, State,
    api::Api,
    change,
    model::*,
    navigate,
    player::{Playback, Player},
    save,
};
use dioxus::prelude::*;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use std::{path::PathBuf, rc::Rc};

#[derive(Clone, Default, PartialEq, Deserialize)]
struct Peaks {
    #[serde(default)]
    rate: f64,
    #[serde(default)]
    peaks: Vec<f64>,
}

#[component]
pub fn Editor(mut state: Signal<State>) -> Element {
    let api = use_context::<Api>();
    let selected = use_signal(|| {
        state
            .peek()
            .current
            .as_ref()
            .and_then(|d| d.project.sentences.first())
            .map(|s| s.id.clone())
            .unwrap_or_default()
    });
    let mut folder = use_signal(|| "all".to_owned());
    let mut search = use_signal(String::new);
    let mut page = use_signal(|| 0usize);
    let mut new_topic = use_signal(String::new);
    let mut playback = use_signal(Playback::default);
    let mut peaks = use_signal(Peaks::default);
    let peak_api = api.clone();
    use_future(move || {
        let api = peak_api.clone();
        async move {
            let id = state.peek().active_id.clone();
            if let Ok(data) = api.get::<Peaks>(&format!("/projects/{id}/peaks")).await {
                peaks.set(data);
            }
        }
    });
    let Some(draft) = state.read().current.clone() else {
        return rsx! {};
    };
    let project = draft.project;
    let sentence = project
        .sentences
        .iter()
        .find(|s| s.id == *selected.read())
        .cloned();
    let filtered: Vec<_> = project
        .sentences
        .iter()
        .filter(|s| {
            (*folder.read() == "all" || s.topic_id == *folder.read())
                && s.text
                    .to_lowercase()
                    .contains(&search.read().to_lowercase())
        })
        .cloned()
        .collect();
    let total_pages = filtered.len().div_ceil(100).max(1);
    let current_page = (*page.read()).min(total_pages - 1);
    let visible: Vec<_> = filtered
        .into_iter()
        .skip(current_page * 100)
        .take(100)
        .collect();
    let sq = Sequence::new(&project);
    let save_api = api.clone();
    let render_api = api.clone();
    let job = state.read().export_job.clone();
    rsx! {
        div {class:"editor",onkeydown:move |e|{
            // Standard shortcuts are scoped to the editor. Inputs retain their normal text-editing keys.
            if (e.modifiers().ctrl()||e.modifiers().meta())
                && e.key()==Key::Character("s".into()) {e.prevent_default();let api=api.clone();spawn(async move {let _=save(state,api).await;});}
        },
            div {class:"editor-toolbar",
                input {class:"project-title","aria-label":"Project name",maxlength:120,value:"{project.name}",oninput:move |e|change(state,|p|p.name=e.value())}
                span {class:if draft.generation!=draft.saved_generation {"save-status dirty"}else{"save-status"},role:"status",if state.read().saving {"Saving…"}else if draft.generation!=draft.saved_generation {"Unsaved changes"}else{"Saved on this computer"}}
                button {disabled:draft.undo.is_empty()||state.read().saving,onclick:move |_|{playback.write().playing=false;if let Some(d)=state.write().current.as_mut(){d.undo();}},"Undo"}
                button {disabled:draft.redo.is_empty()||state.read().saving,onclick:move |_|{playback.write().playing=false;if let Some(d)=state.write().current.as_mut(){d.redo();}},"Redo"}
                button {class:"primary",disabled:state.read().saving||!state.read().current.as_ref().is_some_and(Draft::dirty),onclick:move |_|{let api=save_api.clone();spawn(async move{let _=save(state,api).await;});},"Save"}
                button {onclick:move |_|navigate(state,Destination::Reload),"Reload saved"}
            }
            div {class:"editor-grid",
                aside {class:"folders card","aria-label":"Topic folders",
                    h2 {"Topic folders"}
                    button {class:if *folder.read()=="all" {"folder active"}else{"folder"},onclick:move |_|{folder.set("all".into());page.set(0);},"All sentences ({project.sentences.len()})"}
                    for topic in project.topics.clone() {TopicRow {key:"{topic.id}",topic,state,folder,page,playback}}
                    label {r#for:"new-topic","New topic"}
                    input {id:"new-topic",placeholder:"Give the folder a name",maxlength:80,value:"{new_topic}",oninput:move |e|new_topic.set(e.value())}
                    button {disabled:new_topic.read().trim().is_empty()||project.topics.len()>=100,onclick:move |_|{
                        let name=new_topic.peek().trim().to_owned();if name.is_empty(){return;}
                        change(state,|p|{let accent=["violet","coral","teal","amber","sky","lime"][p.topics.len()%6];p.topics.push(Topic{id:format!("t-{}",uuid::Uuid::new_v4()),name,accent:accent.into(),summary:None,..Default::default()});});new_topic.set(String::new());
                    },"Add topic"}
                    p {class:"hint","Add a folder to your story to include its sentences in source order."}
                    details {summary {"Camera sync"}
                        p {class:"hint","A is the master audio. Offsets shift the other camera previews and exports."}
                        for camera in project.cameras.clone() {CameraOffset {camera,state,playback}}
                        button {onclick:move |_|{playback.write().playing=false;change(state,|p|{let mut seen=std::collections::HashSet::new();let mut i=0;for s in &mut p.sentences {s.cam=if seen.insert(s.topic_id.clone()) {"A".into()}else{i+=1;p.cameras[i%p.cameras.len()].id.clone()};}});},"Alternate camera angles"}
                    }
                }
                section {class:"sentences card","aria-label":"Transcript",
                    div {class:"section-heading",h2 {"Every sentence"}span {class:"badge","{project.sentences.len()}"}}
                    input {r#type:"search","aria-label":"Search transcript",placeholder:"Find a word or idea…",value:"{search}",oninput:move |e|{search.set(e.value());page.set(0);}}
                    div {class:"sentence-list",for sentence in visible {SentenceRow {key:"{sentence.id}",sentence,state,selected,playback}}}
                    div {class:"pagination",button {disabled:current_page==0,onclick:move |_|page.set(current_page.saturating_sub(1)),"Previous"}span {"{current_page+1} / {total_pages}"}button {disabled:current_page+1>=total_pages,onclick:move |_|page.set(current_page+1),"Next"}}
                }
                section {class:"preview-column",
                    div {class:"view-tabs",button {class:if !playback.read().story {"active"}else{""},onclick:move |_|{playback.set(Playback::default());},"Sentence preview"}button {class:if playback.read().story {"active"}else{""},disabled:sq.total<=0.,onclick:move |_|{playback.set(Playback{story:true,..Default::default()});},"Story preview"}}
                    Player {project:Rc::new(project.clone()),selected_id:selected.read().clone(),playback}
                    if let Some(sentence)=sentence {Inspector {sentence,project:project.clone(),state,selected,playback,peaks:peaks.read().clone()}}
                }
            }
            section {class:"timeline card","aria-label":"Story timeline",
                div {class:"section-heading",h2 {"Your story"}span {class:"mono","{fmt(sq.total)} · {sq.clips.len()} clips"}
                    button {disabled:sq.total<=0.,onclick:move |_|playback.set(Playback{story:true,playing:true,..Default::default()}),"Play story"}
                }
                if project.timeline.is_empty() {p {class:"hint","Add a topic folder or a sentence. Reorder clips with the Up and Down buttons."}}
                div {class:"timeline-list",for (index,item) in project.timeline.iter().cloned().enumerate() {TimelineRow {key:"{index}",item,index,project:project.clone(),state,playback}}}
            }
            section {class:"export card",
                div {class:"section-heading",h2 {"Make it ready to share"}span {class:"hint","Render a saved revision"}}
                div {class:"export-options",
                    label {"Picture shape", select {"aria-label":"Export picture shape",value:"{project.export_settings.ratio}",onchange:move |e|{playback.write().playing=false;change(state,|p|p.export_settings.ratio=e.value());},option {value:"16:9","Landscape · 16:9"}option {value:"9:16","Portrait · 9:16"}option {value:"1:1","Square · 1:1"}}}
                    label {class:"check",input {r#type:"checkbox",checked:project.export_settings.captions,onchange:move |e|change(state,|p|p.export_settings.captions=e.checked())}"Captions"}
                    label {class:"check",input {r#type:"checkbox",checked:project.export_settings.chapters,onchange:move |e|change(state,|p|p.export_settings.chapters=e.checked())}"Chapter markers"}
                    button {class:"primary",disabled:state.read().busy||state.read().saving||job.as_ref().is_some_and(ExportJob::running)||sq.total<=0.,onclick:move |_|{
                        let api=render_api.clone();state.write().busy=true;playback.write().playing=false;
                        spawn(async move{
                            let result=async {
                                save(state,api.clone()).await?;
                                let draft=state.peek().current.clone().ok_or("No interview is open.")?;
                                if draft.dirty(){return Err("Your story changed while saving. Save again before rendering.".into());}
                                api.send::<ExportJob>(Method::POST,&format!("/projects/{}/exports",draft.project.id),Some(json!(draft.project.export_settings))).await
                            }.await;
                            state.write().busy=false;
                            match result {Ok(job)=>state.write().export_job=Some(job),Err(e)=>state.write().error=e}
                        });
                    },"Render and export"}
                }
                if let Some(job)=job.clone() {ExportStatus {job,state}}
                for warning in project.warnings {p {class:"hint","{warning}"}}
            }
        }
    }
}

#[component]
fn TopicRow(
    topic: Topic,
    mut state: Signal<State>,
    mut folder: Signal<String>,
    mut page: Signal<usize>,
    mut playback: Signal<Playback>,
) -> Element {
    let count = state
        .read()
        .current
        .as_ref()
        .map(|d| {
            d.project
                .sentences
                .iter()
                .filter(|s| s.topic_id == topic.id && !s.skip)
                .count()
        })
        .unwrap_or(0);
    let choose_id = topic.id.clone();
    let rename_id = topic.id.clone();
    let add_id = topic.id.clone();
    rsx! {
        div {class:"topic",style:"--accent:var(--{topic.accent})",
            button {class:if *folder.read()==topic.id {"folder active"}else{"folder"},onclick:move |_|{folder.set(choose_id.clone());page.set(0);},span {class:"topic-dot"}span {"{topic.name}"}small {"{count}"}}
            div {class:"topic-actions",button {"aria-label":"Add topic to timeline",onclick:move |_|{playback.write().playing=false;change(state,|p|p.timeline.push(TimelineItem::Group{topic_id:add_id.clone()}));},"+ Add folder to story"}
                details {summary {"Rename"}input {"aria-label":"Topic name",value:"{topic.name}",maxlength:80,onchange:move |e|{let value=e.value().trim().to_owned();if !value.is_empty(){change(state,|p|if let Some(t)=p.topics.iter_mut().find(|t|t.id==rename_id){t.name=value;});}}}}
            }
        }
    }
}
#[component]
fn SentenceRow(
    sentence: Sentence,
    mut state: Signal<State>,
    mut selected: Signal<String>,
    mut playback: Signal<Playback>,
) -> Element {
    let id = sentence.id.clone();
    let add_id = id.clone();
    let active = *selected.read() == id;
    rsx! {
        article {class:if active {"sentence active"}else if sentence.skip {"sentence skipped"}else{"sentence"},
            button {class:"sentence-select",onclick:move |_|{selected.set(id.clone());playback.set(Playback::default());},
                span {class:"sentence-meta",span {"{sentence.n:03}"}span {"{fmt(sentence.r#in)}"}span {"CAM {sentence.cam}"}if sentence.skip {span {"Skipped"}}}
                p {"{sentence.text}"}
            }
            button {class:"add-clip","aria-label":"Add sentence to timeline",onclick:move |_|{playback.write().playing=false;change(state,|p|p.timeline.push(TimelineItem::Clip{sentence_id:add_id.clone()}));},"+ Add to story"}
        }
    }
}

fn sentence_edit(
    state: Signal<State>,
    selected: Signal<String>,
    mut playback: Signal<Playback>,
    f: impl FnOnce(&mut Sentence),
) {
    playback.write().playing = false;
    let id = selected.peek().clone();
    change(state, |p| {
        if let Some(s) = p.sentences.iter_mut().find(|s| s.id == id) {
            f(s);
        }
    });
}
#[component]
fn Inspector(
    sentence: Sentence,
    project: Project,
    mut state: Signal<State>,
    selected: Signal<String>,
    playback: Signal<Playback>,
    peaks: Peaks,
) -> Element {
    let min = (sentence.start - 1.5).max(0.);
    let max = project
        .cameras
        .first()
        .map(|c| c.duration)
        .unwrap_or(sentence.end + 1.5)
        .min(sentence.end + 1.5);
    let wave: Vec<_> = (0..100)
        .map(|i| {
            let time = min + (max - min) * i as f64 / 100.;
            peaks
                .peaks
                .get((time * peaks.rate) as usize)
                .copied()
                .unwrap_or(0.)
                .abs()
                .min(1.)
        })
        .collect();
    rsx! {
        section {class:"inspector card","aria-label":"Selected sentence editor",
            div {class:"section-heading",h2 {"Sentence {sentence.n}"}span {class:"mono","{fmt(sentence.out-sentence.r#in)}"}}
            label {r#for:"sentence-text","Transcript text"}textarea {id:"sentence-text",rows:3,maxlength:4000,value:"{sentence.text}",oninput:move |e|sentence_edit(state,selected,playback,|s|s.text=e.value())}
            div {class:"field-grid",
                label {"Topic", select {"aria-label":"Sentence topic",value:"{sentence.topic_id}",onchange:move |e|sentence_edit(state,selected,playback,|s|s.topic_id=e.value()),for topic in project.topics {option {value:"{topic.id}","{topic.name}"}}}}
                label {"Camera", select {"aria-label":"Sentence camera",value:"{sentence.cam}",onchange:move |e|sentence_edit(state,selected,playback,|s|s.cam=e.value()),for camera in project.cameras {option {value:"{camera.id}","{camera.id} · {camera.name}"}}}}
            }
            div {class:"waveform","aria-label":"Audio waveform",role:"img",for amplitude in wave {span {style:"height:{(amplitude*95.).max(3.)}%"}}}
            div {class:"field-grid three",
                label {"In · seconds", input {r#type:"number",step:0.05,min,max:sentence.out-0.3,value:sentence.r#in,"aria-label":"Sentence in point",onchange:move |e|if let Ok(value)=e.value().parse::<f64>()&& value.is_finite(){sentence_edit(state,selected,playback,|s|s.r#in=value);}}}
                label {"Out · seconds", input {r#type:"number",step:0.05,min:sentence.r#in+0.3,max,value:sentence.out,"aria-label":"Sentence out point",onchange:move |e|if let Ok(value)=e.value().parse::<f64>()&& value.is_finite(){sentence_edit(state,selected,playback,|s|s.out=value);}}}
                label {"L-cut · seconds", input {r#type:"number",step:0.05,min:0,max:2.5,value:sentence.lcut,"aria-label":"Sentence L-cut",onchange:move |e|if let Ok(value)=e.value().parse::<f64>()&& value.is_finite(){sentence_edit(state,selected,playback,|s|s.lcut=value);}}}
            }
            p {class:"hint","Cuts stay within 1.5 seconds of the sentence and last at least 0.3 seconds. An L-cut brings in the next picture while this sentence’s audio continues."}
            label {class:"check",input {r#type:"checkbox",checked:sentence.skip,onchange:move |e|sentence_edit(state,selected,playback,|s|s.skip=e.checked())}"Skip this sentence in the story"}
        }
    }
}

#[component]
fn CameraOffset(camera: Camera, state: Signal<State>, mut playback: Signal<Playback>) -> Element {
    let id = camera.id.clone();
    rsx! {label {class:"offset","{camera.id} · {camera.name}",input {r#type:"number",step:0.01,min:-7200,max:7200,value:camera.offset,disabled:camera.id=="A"||camera.virtual_crop.is_some(),"aria-label":"Camera {camera.id} offset in seconds",onchange:move |e|if let Ok(value)=e.value().parse::<f64>()&& value.is_finite(){playback.write().playing=false;change(state,|p|if let Some(c)=p.cameras.iter_mut().find(|c|c.id==id){c.offset=value;});}}}}
}
#[component]
fn TimelineRow(
    item: TimelineItem,
    index: usize,
    project: Project,
    state: Signal<State>,
    mut playback: Signal<Playback>,
) -> Element {
    let (kind, title) = match &item {
        TimelineItem::Group { topic_id } => (
            "Folder",
            project
                .topics
                .iter()
                .find(|t| &t.id == topic_id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "Missing topic".into()),
        ),
        TimelineItem::Clip { sentence_id } => (
            "Sentence",
            project
                .sentences
                .iter()
                .find(|s| &s.id == sentence_id)
                .map(|s| s.text.clone())
                .unwrap_or_else(|| "Missing sentence".into()),
        ),
    };
    let sq = Sequence::new(&project);
    let clips: Vec<_> = sq.clips.iter().filter(|c| c.item_index == index).collect();
    let start = clips.first().map(|c| c.a_start).unwrap_or(0.);
    let duration: f64 = clips.iter().map(|c| c.a_end - c.a_start).sum();
    rsx! {div {class:"timeline-item",
        button {class:"timeline-label",onclick:move |_|playback.set(Playback{story:true,time:start,..Default::default()}),span {class:"badge","{index+1} · {kind}"}strong {"{title}"}span {class:"mono","{fmt(duration)}"}}
        div {class:"actions",
            button {disabled:index==0,"aria-label":"Move timeline item up",onclick:move |_|{playback.write().playing=false;change(state,|p|p.timeline.swap(index,index-1));},"↑"}
            button {disabled:index+1>=project.timeline.len(),"aria-label":"Move timeline item down",onclick:move |_|{playback.write().playing=false;change(state,|p|p.timeline.swap(index,index+1));},"↓"}
            button {"aria-label":"Remove timeline item",onclick:move |_|{playback.write().playing=false;change(state,|p|{p.timeline.remove(index);});},"Remove"}
        }
    }}
}

#[component]
fn ExportStatus(job: ExportJob, mut state: Signal<State>) -> Element {
    let api = use_context::<Api>();
    let mut saved = use_signal(Vec::<PathBuf>::new);
    rsx! {div {class:"export-status",
        p {role:"status","Revision {job.revision} · {job.status} · {job.detail}"}
        if job.running() {progress {max:100,value:job.pct,"aria-label":"Export progress"}}
        if let Some(error)=job.error {p {class:"error",role:"alert","{error}"}}
        if job.status=="done" {div {class:"actions",for filename in job.files {
            {let api=api.clone();let path=format!("/projects/{}/exports/{}/files/{}",job.project_id,job.id,filename);rsx!{
                button {disabled:state.read().busy,onclick:move |_|{
                    let api=api.clone();let path=path.clone();let name=filename.clone();
                    spawn(async move {if let Some(file)=rfd::AsyncFileDialog::new().set_file_name(name).save_file().await {
                        state.write().busy=true;let destination=file.path().to_path_buf();let result=api.download(&path,&destination).await;state.write().busy=false;
                        match result {Ok(())=>{saved.write().push(destination);state.write().notice="Export saved to your chosen folder.".into();},Err(e)=>state.write().error=e}
                    }});
                },"Save {filename}"}
            }}
        }}}
        for path in saved.read().iter().cloned() {button {onclick:move |_|{if open::that(&path).is_err(){state.write().error="Could not open that file with the system application.".into();}},"Open {path.file_name().unwrap_or_default().to_string_lossy()}"}}
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeline_view_has_keyboard_operable_reorder_controls() {
        fn Preview() -> Element {
            let state = use_signal(State::default);
            let playback = use_signal(Playback::default);
            let project = Project {
                topics: vec![Topic {
                    id: "t".into(),
                    name: "Opening".into(),
                    accent: "teal".into(),
                    summary: None,
                    ..Default::default()
                }],
                timeline: vec![TimelineItem::Group {
                    topic_id: "t".into(),
                }],
                ..Default::default()
            };
            rsx! {TimelineRow{item:project.timeline[0].clone(),index:0,project,state,playback}}
        }
        let html = dioxus_ssr::render_element(rsx! {Preview{}});
        assert!(html.contains("Move timeline item up"));
        assert!(html.contains("Opening"));
        assert!(html.contains("Remove timeline item"));
    }
}
