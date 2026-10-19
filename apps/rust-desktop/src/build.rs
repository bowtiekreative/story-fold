use crate::{State, api::Api};
use dioxus::prelude::*;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Dashboard {
    #[serde(default)]
    pipeline: Pipeline,
    #[serde(default)]
    control: Control,
    #[serde(default)]
    run: Option<Check>,
    #[serde(default)]
    runs: Vec<Check>,
    #[serde(default)]
    proposals: Vec<Proposal>,
    #[serde(default)]
    history: History,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Pipeline {
    #[serde(default)]
    name: String,
    #[serde(default)]
    milestone: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    tasks: Vec<Task>,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Task {
    id: String,
    title: String,
    acceptance: String,
    lane: String,
    status: String,
    #[serde(default)]
    evidence: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Control {
    #[serde(default)]
    paused: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Check {
    #[serde(default)]
    id: String,
    kind: String,
    status: String,
    #[serde(default)]
    output: String,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct History {
    #[serde(default)]
    commits: Vec<Commit>,
    #[serde(default)]
    status: String,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Commit {
    id: String,
    at: String,
    message: String,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Source {
    path: String,
    content: String,
    hash: String,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Proposal {
    id: String,
    path: String,
    summary: String,
    before: String,
    after: String,
    status: String,
    #[serde(default)]
    commit: Option<String>,
}
#[derive(Clone, Debug, Default)]
struct BuildState {
    dashboard: Dashboard,
    files: Vec<String>,
    site: Value,
    source: Option<Source>,
    draft: String,
    summary: String,
    review: Option<Proposal>,
    loaded: bool,
    error: String,
}
impl BuildState {
    fn accepts_refresh(&self, proposal: &Proposal) -> bool {
        self.source.as_ref().is_some_and(|s| {
            s.path == proposal.path && (self.draft == s.content || self.draft == proposal.after)
        })
    }
}

#[component]
pub fn Build(mut state: Signal<State>, active: bool) -> Element {
    let api = use_context::<Api>();
    let mut local = use_signal(BuildState::default);
    let mut panel = use_signal(|| "Overview".to_string());
    let mut filter = use_signal(String::new);
    let mut title = use_signal(String::new);
    let mut acceptance = use_signal(String::new);
    let poll_api = api.clone();
    use_future(move || {
        let api = poll_api.clone();
        async move {
            loop {
                match api.get::<Dashboard>("/catalyst").await {
                    Ok(data) => {
                        let mut b = local.write();
                        b.dashboard = data;
                        b.loaded = true;
                        if b.error
                            .starts_with("Cannot reach the local Storyfold service.")
                        {
                            b.error.clear();
                        }
                    }
                    Err(e) => local.write().error = e,
                }
                if local.peek().files.is_empty()
                    && let Ok(files) = api.get::<Vec<String>>("/catalyst/files").await
                {
                    local.write().files = files;
                }
                if local.peek().site.is_null()
                    && let Ok(site) = api.get::<Value>("/catalyst/site").await
                {
                    local.write().site = site;
                }
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
        }
    });
    use_effect(move || {
        let b = local.read();
        let dirty = b.source.as_ref().is_some_and(|s| s.content != b.draft);
        if state.peek().source_dirty != dirty {
            state.write().source_dirty = dirty;
        }
        if state.peek().build_dialog != b.review.is_some() {
            state.write().build_dialog = b.review.is_some();
        }
    });
    let b = local.read().clone();
    let data = b.dashboard;
    let control_api = api.clone();
    let build_api = api.clone();
    let test_api = api.clone();
    let cancel_api = api.clone();
    let task_api = api.clone();
    let paused = data.control.paused;
    let check = data.run.clone().or_else(|| data.runs.first().cloned());
    let running = check.as_ref().is_some_and(|c| c.status == "running");
    let completed = data
        .pipeline
        .tasks
        .iter()
        .filter(|t| t.status == "done")
        .count();
    let pending = data
        .proposals
        .iter()
        .filter(|p| p.status == "review")
        .count();
    rsx! {
        div {class:"build", "aria-label":"Build control panel",
            div {class:"build-body",inert:b.review.is_some().then_some(true),
            div {class:"build-heading",div {span {class:"eyebrow","YOUR BUILD, IN THE OPEN"}h1 {"Your direction. Every change in view."}}
                button {class:if paused {"primary"}else{""},disabled:state.read().busy,onclick:move |_|{
                    let api=control_api.clone();act(state,local,api,Method::POST,"/catalyst/control".into(),Some(json!({"paused":!paused})),if paused {"Build direction resumed."}else{"Build direction paused. Active checks can be cancelled separately."});
                },if paused {"Resume build"}else{"Pause build"}}
            }
            if !b.error.is_empty() && active {div {class:"banner error",role:"alert",span {"{b.error}"}button {onclick:move |_|local.write().error.clear(),"Dismiss"}}}
            nav {class:"build-tabs","aria-label":"Build sections",for name in ["Overview","Tasks","Source","Pages","History","Activity"] {button {class:if *panel.read()==name {"active"}else{""},onclick:move |_|panel.set(name.into()),"{name}"}}}
            if !b.loaded {p {role:"status","Connecting to your local build service…"}}
            if *panel.read()=="Overview" {
                div {class:"build-overview",
                    section {class:"card",span {class:"eyebrow","CURRENT MILESTONE"}h2 {"{data.pipeline.milestone}"}p {"{data.pipeline.message}"}progress {max:data.pipeline.tasks.len().max(1),value:completed,"aria-label":"Build task completion"}p {"{completed} of {data.pipeline.tasks.len()} tasks complete · {pending} changes waiting for review"}
                        p {class:"hint",if paused {"Your saved build direction is paused. This control is cooperative; send a chat message to steer an active agent turn immediately."}else{"Build direction is active. Your tasks and acceptance criteria are saved in the project."}}
                    }
                    section {class:"card",h2 {"Check the real build"}p {"Run the configured Rust checks and read their output here."}
                        div {class:"actions",
                            button {disabled:running||state.read().busy,onclick:move |_|act(state,local,build_api.clone(),Method::POST,"/catalyst/checks".into(),Some(json!({"kind":"build"})),"Build check started."),"Run build"}
                            button {disabled:running||state.read().busy,onclick:move |_|act(state,local,test_api.clone(),Method::POST,"/catalyst/checks".into(),Some(json!({"kind":"test"})),"Test check started."),"Run tests"}
                            button {disabled:!running||state.read().busy,onclick:move |_|act(state,local,cancel_api.clone(),Method::POST,"/catalyst/checks/cancel".into(),None,"Check cancellation requested."),"Cancel check"}
                        }
                        if let Some(check)=check.clone() {div {class:"check-status",strong {"{check.kind} · {check.status}"}pre {class:"log","{check.output}"}}}
                    }
                }
            }
            if *panel.read()=="Tasks" {
                section {class:"card",h2 {"Tell the build what matters"}
                    div {class:"field-grid",label {"Task title", input {value:"{title}",maxlength:160,oninput:move |e|title.set(e.value())}}label {"Done when", input {value:"{acceptance}",maxlength:1000,oninput:move |e|acceptance.set(e.value())}}}
                    button {class:"primary",disabled:state.read().busy||title.read().trim().is_empty()||acceptance.read().trim().is_empty(),onclick:move |_|{
                        let api=task_api.clone();let body=json!({"title":title.peek().trim(),"acceptance":acceptance.peek().trim(),"lane":"Next"});state.write().busy=true;
                        spawn(async move{let result=api.send::<Value>(Method::POST,"/catalyst/tasks",Some(body)).await;state.write().busy=false;match result {Ok(_)=>{title.set(String::new());acceptance.set(String::new());state.write().notice="Task added to your roadmap.".into();if let Ok(data)=api.get::<Dashboard>("/catalyst").await{local.write().dashboard=data;}},Err(e)=>local.write().error=e}});
                    },"Add task"}
                }
                div {class:"task-grid",for lane in ["Now","Next","Later"] {section {class:"card",h2 {"{lane}"}for task in data.pipeline.tasks.iter().filter(|t|t.lane==lane).cloned() {TaskCard {key:"{task.id}",task,state,local}}}}}
            }
            if *panel.read()=="Source" {
                div {class:"source-grid",
                    aside {class:"card file-list",label {r#for:"file-filter","Find a source file"}input {id:"file-filter",r#type:"search",value:"{filter}",oninput:move |e|filter.set(e.value()),placeholder:"apps/, packages/, docs/…"}
                        for file in b.files.iter().filter(|f|f.to_lowercase().contains(&filter.read().to_lowercase())).cloned() {
                            {let api=api.clone();let name=file.clone();rsx!{button {class:if b.source.as_ref().is_some_and(|s|s.path==file){"file active"}else{"file"},onclick:move |_|{
                                if local.peek().source.as_ref().is_some_and(|s|s.content!=local.peek().draft){local.write().error="Review or discard this source draft before opening another file.".into();return;}
                                let api=api.clone();let file=file.clone();state.write().busy=true;
                                spawn(async move {let result=api.source::<Source>(&file).await;state.write().busy=false;match result {Ok(source)=>{let mut b=local.write();b.draft=source.content.clone();b.source=Some(source);b.summary.clear();b.error.clear();},Err(e)=>local.write().error=e}});
                            },"{name}"}}}
                        }
                    }
                    section {class:"card source-editor",
                        if let Some(source)=b.source.clone() {
                            div {class:"section-heading",h2 {"{source.path}"}span {class:"badge",if source.content!=b.draft {"Unsaved draft"}else{"Current file"}}}
                            p {class:"hint","Edit a draft, review both versions, then apply it to the real file. The server checks for stale content, private data and Git safety before saving."}
                            textarea {class:"code",rows:24,spellcheck:false,"aria-label":"Source file draft",value:"{b.draft}",oninput:move |e|local.write().draft=e.value()}
                            label {r#for:"change-summary","Describe this change"}input {id:"change-summary",value:"{b.summary}",maxlength:180,oninput:move |e|local.write().summary=e.value(),placeholder:"What should this change do?"}
                            div {class:"actions",
                                {let api=api.clone();rsx!{button {class:"primary",disabled:state.read().busy||source.content==b.draft,onclick:move |_|{
                                    let api=api.clone();let b=local.peek().clone();let Some(source)=b.source else{return;};state.write().busy=true;
                                    spawn(async move {let result=api.send::<Proposal>(Method::POST,"/catalyst/proposals",Some(json!({"path":source.path,"after":b.draft,"hash":source.hash,"summary":if b.summary.trim().is_empty(){format!("Update {}",source.path)}else{b.summary}}))).await;state.write().busy=false;match result {Ok(proposal)=>local.write().review=Some(proposal),Err(e)=>local.write().error=e}});
                                },"Review change"}}}
                                button {disabled:state.read().busy||source.content==b.draft,onclick:move |_|{let original=local.peek().source.as_ref().map(|s|s.content.clone());if let Some(original)=original{local.write().draft=original;}},"Discard draft"}
                            }
                        } else {h2 {"Every file is yours."}p {"Choose a non-private source file to inspect or edit."}}
                    }
                }
            }
            if *panel.read()=="Pages" {
                section {class:"card",h2 {"Your application map"}p {{b.site["stack"].as_str().unwrap_or("Local project")}}
                    if let Some(pages)=b.site["pages"].as_array() {for page in pages {article {class:"page-row",strong {{page["name"].as_str().unwrap_or("Page")}}span {class:"mono",{page["route"].as_str().unwrap_or("")}}p {{page["desc"].as_str().unwrap_or("")}}small {{page["file"].as_str().unwrap_or("")}}}}}
                    if let Some(notes)=b.site["notes"].as_array() {for note in notes {p {class:"hint",{note.as_str().unwrap_or("")}}}}
                }
            }
            if *panel.read()=="History" {
                section {class:"card",h2 {"Reviewed source changes"}
                    if data.proposals.is_empty() {p {"Your reviewed changes will appear here."}}
                    for proposal in data.proposals.clone() {ProposalRow {key:"{proposal.id}",proposal,state,local}}
                }
                section {class:"card",h2 {"Git history"}pre {class:"log","{data.history.status}"}for commit in data.history.commits {div {class:"commit",code {"{commit.id}"}strong {"{commit.message}"}small {"{commit.at}"}}}}
            }
            if *panel.read()=="Activity" {
                section {class:"card",h2 {"Checks and evidence"}for run in data.runs {details {summary {"{run.kind} · {run.status} · {run.id}"}pre {class:"log","{run.output}"}}}for task in data.pipeline.tasks {if !task.evidence.is_empty(){details {summary {"{task.title}"}for evidence in task.evidence {p {"{evidence}"}}}}}}
            }
            }
            if let Some(proposal)=b.review.clone() {Review {proposal,state,local}}
        }
    }
}

fn act(
    mut state: Signal<State>,
    mut local: Signal<BuildState>,
    api: Api,
    method: Method,
    path: String,
    body: Option<Value>,
    notice: &'static str,
) {
    state.write().busy = true;
    local.write().error.clear();
    spawn(async move {
        let result = api.send::<Value>(method, &path, body).await;
        state.write().busy = false;
        match result {
            Ok(_) => {
                state.write().notice = notice.into();
                if let Ok(data) = api.get::<Dashboard>("/catalyst").await {
                    local.write().dashboard = data;
                }
            }
            Err(e) => local.write().error = e,
        }
    });
}
#[component]
fn TaskCard(task: Task, state: Signal<State>, local: Signal<BuildState>) -> Element {
    let api = use_context::<Api>();
    let lane_api = api.clone();
    let status_path = format!("/catalyst/tasks/{}", task.id);
    let lane_path = status_path.clone();
    rsx! {article {class:"task",strong {"{task.title}"}p {"Done when: {task.acceptance}"}
        label {"Status", select {value:"{task.status}","aria-label":"Task status",disabled:state.read().busy,onchange:move |e|act(state,local,api.clone(),Method::PATCH,status_path.clone(),Some(json!({"status":e.value()})),"Task status saved."),option {value:"todo","To do"}option {value:"doing","Doing"}option {value:"review","In review"}option {value:"done","Done"}}}
        label {"Priority", select {value:"{task.lane}","aria-label":"Task priority",disabled:state.read().busy,onchange:move |e|act(state,local,lane_api.clone(),Method::PATCH,lane_path.clone(),Some(json!({"lane":e.value()})),"Task priority saved."),option {value:"Now","Now"}option {value:"Next","Next"}option {value:"Later","Later"}}}
        for evidence in task.evidence {small {"{evidence}"}}
    }}
}
#[component]
fn ProposalRow(
    proposal: Proposal,
    mut state: Signal<State>,
    mut local: Signal<BuildState>,
) -> Element {
    let api = use_context::<Api>();
    let discard_api = api.clone();
    let discard_id = proposal.id.clone();
    let undo_id = proposal.id.clone();
    let review = proposal.clone();
    rsx! {article {class:"proposal-row",div {strong {"{proposal.summary}"}p {class:"mono","{proposal.path}"}small {"{proposal.status} " {proposal.commit.as_deref().unwrap_or("")}}}
        div {class:"actions",if proposal.status=="review" {
            button {onclick:move |_|local.write().review=Some(review.clone()),"Review"}
            button {disabled:state.read().busy,onclick:move |_|act(state,local,discard_api.clone(),Method::POST,format!("/catalyst/proposals/{discard_id}/discard"),None,"Proposed change discarded."),"Discard"}
        }
        if proposal.status=="applied" {button {disabled:state.read().busy,onclick:move |_|act(state,local,api.clone(),Method::POST,format!("/catalyst/proposals/{undo_id}/undo"),None,"Change undone through a new reviewed Git commit."),"Undo change"}}
        }
    }}
}
#[component]
fn Review(proposal: Proposal, mut state: Signal<State>, mut local: Signal<BuildState>) -> Element {
    let api = use_context::<Api>();
    let discard_api = api.clone();
    let id = proposal.id.clone();
    let discard_id = id.clone();
    let applied = proposal.clone();
    rsx! {div {class:"modal-backdrop",section {class:"modal wide",role:"dialog","aria-modal":"true","aria-labelledby":"review-title",onkeydown:move |e|if e.key()==Key::Escape&&!state.peek().busy{e.stop_propagation();local.write().review=None;},
        h2 {id:"review-title","Review: {proposal.summary}"}p {class:"mono","{proposal.path}"}
        div {class:"diff",section {h3 {"Before"}pre {"{proposal.before}"}}section {h3 {"After"}pre {"{proposal.after}"}}}
        p {class:"hint","Applying writes this version to the real source file and records a local Git commit. Reopen the app or run the build check to see code changes."}
        div {class:"actions",
            button {class:"primary",autofocus:true,disabled:state.read().busy||proposal.status!="review",onclick:move |_|{
                let api=api.clone();let id=id.clone();let applied=applied.clone();state.write().busy=true;
                spawn(async move {
                    let result=api.send::<Value>(Method::POST,&format!("/catalyst/proposals/{id}/apply"),None).await;state.write().busy=false;
                    match result {Ok(_)=>{
                        if local.peek().accepts_refresh(&applied)&& let Ok(source)=api.source::<Source>(&applied.path).await {let mut b=local.write();if b.accepts_refresh(&applied){b.draft=source.content.clone();b.source=Some(source);}}
                        local.write().review=None;state.write().notice="Applied to the source and saved in Git. History has an undo action.".into();
                        if let Ok(data)=api.get::<Dashboard>("/catalyst").await{local.write().dashboard=data;}
                    },Err(e)=>local.write().error=e}
                });
            },"Apply reviewed change"}
            button {disabled:state.read().busy,onclick:move |_|{act(state,local,discard_api.clone(),Method::POST,format!("/catalyst/proposals/{discard_id}/discard"),None,"Proposed change discarded.");local.write().review=None;},"Discard proposal"}
            button {disabled:state.read().busy,onclick:move |_|local.write().review=None,"Back to draft"}
        }
    }}}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_contract_retains_acceptance_and_evidence() {
        let task:Task=serde_json::from_value(json!({"id":"one","title":"Keep edits safe","acceptance":"Conflict leaves the draft intact","status":"review","lane":"Now","evidence":["Revision test passed"]})).unwrap();
        assert_eq!(task.acceptance, "Conflict leaves the draft intact");
        assert_eq!(task.evidence.len(), 1);
    }
    #[test]
    fn applying_a_proposal_never_overwrites_a_newer_or_unrelated_source_draft() {
        let proposal = Proposal {
            path: "apps/example.rs".into(),
            after: "reviewed".into(),
            ..Default::default()
        };
        let mut state = BuildState {
            source: Some(Source {
                path: proposal.path.clone(),
                content: "before".into(),
                ..Default::default()
            }),
            draft: "reviewed".into(),
            ..Default::default()
        };
        assert!(state.accepts_refresh(&proposal));
        state.draft = "new edits typed while applying".into();
        assert!(!state.accepts_refresh(&proposal));
        state.source.as_mut().unwrap().path = "apps/another.rs".into();
        state.draft = "reviewed".into();
        assert!(!state.accepts_refresh(&proposal));
    }
}
