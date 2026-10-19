use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VirtualCrop {
    pub scale: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub proxy_path: String,
    pub offset: f64,
    pub duration: f64,
    #[serde(rename = "virtual", default, skip_serializing_if = "Option::is_none")]
    pub virtual_crop: Option<VirtualCrop>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub id: String,
    pub n: u32,
    pub text: String,
    pub words: Vec<Word>,
    pub start: f64,
    pub end: f64,
    pub r#in: f64,
    pub out: f64,
    pub topic_id: String,
    pub cam: String,
    pub lcut: f64,
    pub skip: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub id: String,
    pub name: String,
    pub accent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TimelineItem {
    Group {
        #[serde(rename = "topicId")]
        topic_id: String,
    },
    Clip {
        #[serde(rename = "sentenceId")]
        sentence_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportSettings {
    pub ratio: String,
    pub captions: bool,
    pub chapters: bool,
}
impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            ratio: "16:9".into(),
            captions: true,
            chapters: true,
        }
    }
}
impl ExportSettings {
    pub fn validate(&self) -> Result<(), String> {
        if ["16:9", "9:16", "1:1"].contains(&self.ratio.as_str()) {
            Ok(())
        } else {
            Err("Choose a supported aspect ratio.".into())
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub step: String,
    pub detail: String,
    pub pct: f64,
    pub at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub created_at: String,
    pub status: String,
    pub cameras: Vec<Camera>,
    pub sentences: Vec<Sentence>,
    pub topics: Vec<Topic>,
    pub timeline: Vec<TimelineItem>,
    pub export_settings: ExportSettings,
    pub progress: Vec<Progress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub sample: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSentence {
    pub id: String,
    pub text: String,
    pub topic_id: String,
    pub cam: String,
    pub r#in: f64,
    pub out: f64,
    pub lcut: f64,
    pub skip: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEdit {
    pub revision: u64,
    pub name: String,
    pub sentences: Vec<EditSentence>,
    pub topics: Vec<Topic>,
    pub timeline: Vec<TimelineItem>,
    pub export_settings: ExportSettings,
    #[serde(default)]
    pub offsets: HashMap<String, f64>,
}

pub fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
pub fn valid_topic(topic: &Topic) -> bool {
    identifier(&topic.id, 80)
        && !topic.name.trim().is_empty()
        && topic.name.chars().count() <= 80
        && ["violet", "coral", "teal", "amber", "sky", "lime"].contains(&topic.accent.as_str())
        && topic
            .summary
            .as_ref()
            .is_none_or(|s| s.chars().count() <= 1000)
}

pub fn apply_edit(project: &Project, edit: ProjectEdit) -> Result<Project, String> {
    if project.revision != edit.revision {
        return Err("This project changed in another window. Reload before saving.".into());
    }
    if project.status != "ready" {
        return Err("Wait until processing finishes before editing.".into());
    }
    if edit.name.trim().is_empty() || edit.name.chars().count() > 120 {
        return Err("Name the project using up to 120 characters.".into());
    }
    if edit.sentences.len() != project.sentences.len()
        || edit.sentences.len() > 20000
        || edit
            .sentences
            .iter()
            .map(|s| &s.id)
            .collect::<HashSet<_>>()
            .len()
            != project.sentences.len()
    {
        return Err("Every original sentence must be present exactly once.".into());
    }
    if edit.topics.is_empty()
        || edit.topics.len() > 100
        || edit.topics.iter().any(|t| !valid_topic(t))
        || edit
            .topics
            .iter()
            .map(|t| &t.id)
            .collect::<HashSet<_>>()
            .len()
            != edit.topics.len()
    {
        return Err("Use unique, valid topic folders.".into());
    }
    edit.export_settings.validate()?;
    if edit.timeline.len() > 2000 {
        return Err("The story has too many timeline items.".into());
    }
    let originals: HashMap<_, _> = project.sentences.iter().map(|s| (&s.id, s)).collect();
    let topics: HashSet<_> = edit.topics.iter().map(|t| &t.id).collect();
    let mut next = project.clone();
    next.sentences = edit.sentences.into_iter().map(|s| {
        let Some(original) = originals.get(&s.id) else { return Err("Unknown sentence.".into()); };
        if !topics.contains(&s.topic_id) || !project.cameras.iter().any(|c| c.id == s.cam) { return Err("Unknown topic or camera.".into()); }
        if s.text.trim().is_empty() || s.text.chars().count() > 4000 || ![s.r#in,s.out,s.lcut].iter().all(|v| v.is_finite())
            || s.lcut < 0. || s.lcut > 2.5 || s.r#in < (original.start-1.5).max(0.)-0.001
            || s.out > (original.end+1.5).min(project.cameras.first().map_or(0., |c| c.duration))+0.001 || s.out-s.r#in < 0.299 {
            return Err("A cut must stay within 1.5 seconds of its sentence and be at least 0.3 seconds long.".into());
        }
        let mut clip = (**original).clone();
        clip.text = s.text.trim().into(); clip.topic_id = s.topic_id; clip.cam = s.cam;
        clip.r#in=s.r#in; clip.out=s.out; clip.lcut=s.lcut; clip.skip=s.skip;
        Ok(clip)
    }).collect::<Result<_,String>>()?;
    if edit.timeline.iter().any(|i| match i {
        TimelineItem::Group { topic_id } => !topics.contains(topic_id),
        TimelineItem::Clip { sentence_id } => !originals.contains_key(sentence_id),
    }) {
        return Err("Timeline refers to an unknown sentence or topic.".into());
    }
    if edit.offsets.iter().any(|(cam, v)| {
        !["A", "B", "C"].contains(&cam.as_str()) || !v.is_finite() || v.abs() > 7200.
    }) {
        return Err("Invalid camera offset.".into());
    }
    for cam in &mut next.cameras {
        cam.offset = if cam.id == "A" || cam.virtual_crop.is_some() {
            0.
        } else {
            edit.offsets.get(&cam.id).copied().unwrap_or(cam.offset)
        };
    }
    next.name = edit.name.trim().into();
    next.topics = edit.topics;
    next.timeline = edit.timeline;
    next.export_settings = edit.export_settings;
    next.revision += 1;
    Ok(next)
}
