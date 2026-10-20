//! Local recording studio models. Source takes are immutable; clips edit their ranges.
/// Recording sidebar slots, shared by native controls and runtime validation.
pub const OVERLAY_SLOTS: usize = 9;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSession {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub revision: u64,
    pub orientation: String,
    pub takes: Vec<RecordedTake>,
    pub clips: Vec<RecordingClip>,
    pub overlays: Vec<RecordingOverlay>,
    pub active: Option<ActiveRecording>,
    pub active_overlay: Option<String>,
    pub export: Option<RecordingExport>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedTake {
    pub id: String,
    pub name: String,
    pub duration: f64,
    pub has_audio: bool,
    pub marks: Vec<OverlayMark>,
    pub interrupted: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingClip {
    pub id: String,
    pub take_id: String,
    pub name: String,
    pub r#in: f64,
    pub out: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingOverlay {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub text: String,
    pub secondary: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayMark {
    pub at: f64,
    pub overlay_id: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRecording {
    pub take_id: String,
    pub seconds: f64,
    pub microphone: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingExport {
    pub id: String,
    pub status: String,
    pub pct: f64,
    #[serde(default)]
    pub detail: String,
    pub revision: u64,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDevices {
    pub cameras: Vec<RecordingDevice>,
    pub microphones: Vec<RecordingDevice>,
    pub message: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecordingDevice {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingEdit {
    pub revision: u64,
    pub name: String,
    pub orientation: String,
    pub clips: Vec<RecordingClip>,
    pub overlays: Vec<RecordingOverlay>,
}
