//! Saved projects are listed without opening or playing their media.
use crate::{Destination, Page, State, navigate};
use dioxus::prelude::*;

#[component]
pub fn Projects(state: Signal<State>) -> Element {
    let s = state.read().clone();
    let locked = s.busy || s.saving || s.recording_capture;
    rsx! {
        div {class:"library projects-page",
            section {class:"intro",
                span {class:"eyebrow","YOUR SAVED WORK"}
                h1 {"Your projects."}
                p {"Choose a project to continue. Storyfold starts with a fresh workspace; saved interviews and recordings stay here until you open them."}
            }
            section {class:"card",
                h2 {"Start something new"}
                p {class:"hint","Import an interview or create an empty recording project. Your camera opens only when you press Record."}
                div {class:"actions",
                    button {class:"primary",disabled:locked,onclick:move |_|navigate(state,Destination::Page(Page::Library)),"New interview"}
                    button {disabled:locked||s.recording_busy,onclick:move |_|navigate(state,Destination::Recording(None)),"New recording"}
                }
            }
            section {class:"interviews","aria-label":"Saved interviews",
                h2 {"Interviews"}
                if s.library.is_empty() {p {class:"hint","No saved interviews yet. Import footage from New project to create one."}}
                div {class:"project-grid",
                    for project in &s.library {
                        button {class:"project-card",disabled:locked,onclick:{let id=project.id.clone();move |_|navigate(state,Destination::Project(id.clone()))},
                            span {class:"eyebrow","{project.status}"}
                            strong {"{project.name}"}
                            span {"{project.sentences} sentences"}
                            span {"Open interview →"}
                        }
                    }
                }
            }
            section {class:"interviews","aria-label":"Saved recordings",
                h2 {"Recordings"}
                if s.recording_library.is_empty() {p {class:"hint","No saved recordings yet. Start a new recording project to create one."}}
                div {class:"project-grid",
                    for project in &s.recording_library {
                        button {class:"project-card",disabled:locked||s.recording_busy,onclick:{let id=project.id.clone();move |_|navigate(state,Destination::Recording(Some(id.clone())))},
                            span {class:"eyebrow",if project.active.is_some(){"RECORDING IN PROGRESS"}else{"RECORDING"}}
                            strong {"{project.name}"}
                            span {"{project.takes.len()} takes · {project.clips.len()} clips · {project.orientation}"}
                            span {"Open recording →"}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Summary;
    use storyfold_core::recording::RecordingSession;

    #[test]
    fn projects_list_both_kinds_without_media_players() {
        fn Preview() -> Element {
            let state = use_signal(|| State {
                library: vec![Summary {
                    id: "interview-id".into(),
                    name: "Saved interview".into(),
                    status: "ready".into(),
                    sentences: 4,
                }],
                recording_library: vec![RecordingSession {
                    id: "recording-id".into(),
                    name: "Saved recording".into(),
                    orientation: "16:9".into(),
                    ..Default::default()
                }],
                ..Default::default()
            });
            rsx! {Projects {state}}
        }
        let html = dioxus_ssr::render_element(rsx! {Preview {}});
        for text in [
            "Saved interview",
            "Saved recording",
            "Open interview",
            "Open recording",
            "New interview",
            "New recording",
        ] {
            assert!(html.contains(text), "{text}");
        }
        assert!(!html.contains("<video"));
        assert!(!html.contains("<audio"));
    }
}
