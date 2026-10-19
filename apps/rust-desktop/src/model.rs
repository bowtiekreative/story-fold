use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub use storyfold_core::{Camera, Project, Sentence, Sequence, TimelineItem, Topic, fmt};
pub use storyfold_core::{ExportSettings as Settings, VirtualCrop as Crop};

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub sentences: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    #[serde(default)]
    pub speech_provider: String,
    #[serde(default)]
    pub topic_provider: String,
    #[serde(default)]
    pub transcription: bool,
    #[serde(default)]
    pub topics: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportJob {
    pub id: String,
    pub project_id: String,
    pub status: String,
    #[serde(default)]
    pub pct: f64,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub revision: u64,
}
impl ExportJob {
    pub fn running(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "rendering")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub project: Project,
    pub generation: u64,
    pub saved_generation: u64,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
}
impl Draft {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            generation: 0,
            saved_generation: 0,
            undo: vec![],
            redo: vec![],
        }
    }
    pub fn dirty(&self) -> bool {
        self.generation != self.saved_generation
    }
    pub fn change(&mut self, f: impl FnOnce(&mut Project)) {
        let previous = self.project.clone();
        f(&mut self.project);
        if self.project != previous {
            self.undo.push(previous);
            if self.undo.len() > 20 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.generation += 1;
        }
    }
    pub fn undo(&mut self) {
        if let Some(mut previous) = self.undo.pop() {
            previous.revision = self.project.revision;
            self.redo
                .push(std::mem::replace(&mut self.project, previous));
            self.generation += 1;
        }
    }
    pub fn redo(&mut self) {
        if let Some(mut next) = self.redo.pop() {
            next.revision = self.project.revision;
            self.undo.push(std::mem::replace(&mut self.project, next));
            self.generation += 1;
        }
    }
    pub fn saved(&mut self, revision: u64, generation: u64) {
        self.project.revision = revision;
        self.saved_generation = generation;
    }
    pub fn payload(&self) -> Value {
        let offsets: BTreeMap<_, _> = self
            .project
            .cameras
            .iter()
            .map(|c| (c.id.clone(), c.offset))
            .collect();
        json!({"revision":self.project.revision,"name":self.project.name,
            "sentences":self.project.sentences,"topics":self.project.topics,
            "timeline":self.project.timeline,"exportSettings":self.project.export_settings,"offsets":offsets})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn example() -> Project {
        Project {
            id: "test".into(),
            name: "Interview".into(),
            revision: 4,
            status: "ready".into(),
            sentences: vec![
                Sentence {
                    id: "one".into(),
                    n: 1,
                    cam: "A".into(),
                    text: "One".into(),
                    topic_id: "topic".into(),
                    r#in: 1.,
                    out: 4.,
                    lcut: 1.,
                    ..Default::default()
                },
                Sentence {
                    id: "two".into(),
                    n: 2,
                    cam: "B".into(),
                    text: "Two".into(),
                    topic_id: "topic".into(),
                    start: 5.,
                    r#in: 5.,
                    out: 8.,
                    ..Default::default()
                },
            ],
            timeline: vec![TimelineItem::Group {
                topic_id: "topic".into(),
            }],
            ..Default::default()
        }
    }
    #[test]
    fn save_ack_does_not_overwrite_newer_draft_or_undo_revision() {
        let mut draft = Draft::new(example());
        draft.change(|p| p.name = "First".into());
        let saved = draft.generation;
        draft.change(|p| p.name = "Second".into());
        draft.saved(5, saved);
        assert!(draft.dirty());
        assert_eq!(draft.project.name, "Second");
        draft.undo();
        assert_eq!(draft.project.name, "First");
        assert_eq!(draft.project.revision, 5);
        draft.redo();
        assert_eq!(draft.project.name, "Second");
        assert_eq!(draft.project.revision, 5);
    }
    #[test]
    fn api_payload_retains_original_timestamps_and_unknown_fields() {
        let mut p = example();
        p.sentences[0].words = vec![storyfold_core::Word {
            text: "One".into(),
            start: 1.,
            end: 2.,
        }];
        p.sentences[0]
            .extra
            .insert("customField".into(), json!("retained"));
        let payload = Draft::new(p).payload();
        assert_eq!(payload["sentences"][0]["words"][0]["text"], "One");
        assert_eq!(payload["revision"], 4);
        assert_eq!(payload["sentences"][0]["customField"], "retained");
        assert_eq!(payload["timeline"][0]["topicId"], "topic");
    }
}
