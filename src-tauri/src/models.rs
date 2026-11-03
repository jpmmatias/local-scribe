use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Extracting,
    Transcribing,
    Completed,
    Failed,
    Cancelled,
}
impl Status {
    pub fn active(&self) -> bool {
        matches!(self, Self::Queued | Self::Extracting | Self::Transcribing)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionJob {
    pub id: String,
    pub source_path: String,
    pub filename: String,
    pub model: String,
    pub language: String,
    pub status: Status,
    pub progress: Option<f32>,
    pub created_at: String,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTranscriptEvent {
    pub job_id: String,
    pub segment: TranscriptSegment,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub id: String,
    pub job_id: String,
    pub filename: String,
    pub language: String,
    pub model: String,
    pub segments: Vec<TranscriptSegment>,
    pub full_text: String,
    pub created_at: String,
}
impl Transcript {
    pub fn rebuild(&mut self) {
        self.full_text = self
            .segments
            .iter()
            .map(|s| s.text.trim())
            .collect::<Vec<_>>()
            .join("\n");
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub original: Transcript,
    pub edited: Transcript,
    pub source_json: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    pub model: String,
    pub language: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            model: "base".into(),
            language: "pt".into(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Database {
    pub version: u32,
    pub revision: u64,
    pub jobs: Vec<TranscriptionJob>,
    pub records: Vec<Record>,
    pub preferences: Preferences,
}
impl Default for Database {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            jobs: vec![],
            records: vec![],
            preferences: Preferences::default(),
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub revision: u64,
    pub jobs: Vec<TranscriptionJob>,
    pub transcripts: Vec<Transcript>,
    pub preferences: Preferences,
}
impl From<&Database> for Snapshot {
    fn from(d: &Database) -> Self {
        Self {
            revision: d.revision,
            jobs: d.jobs.clone(),
            transcripts: d.records.iter().map(|r| r.edited.clone()).collect(),
            preferences: d.preferences.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub duration_ms: u64,
    pub video: bool,
}

pub fn language_valid(value: &str) -> bool {
    matches!(value, "pt" | "en" | "auto")
}
