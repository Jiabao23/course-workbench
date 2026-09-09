use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub title: String,
    pub source_kind: String,
    pub source: String,
    pub bvid: Option<String>,
    pub page: Option<u32>,
    pub duration_ms: u64,
    pub audio_path: Option<String>,
    pub active_version_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub id: String,
    pub asset_id: String,
    pub version: u32,
    pub source_kind: String,
    pub model: Option<String>,
    pub language: String,
    pub segments: Vec<Segment>,
    pub created_at: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub asset_id: String,
    pub title: String,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub error: Option<String>,
    pub model: String,
    pub device: String,
    pub mode: String,
    pub preset: String,
    pub chunk_done: u32,
    pub chunk_total: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    pub segment_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub asset_id: String,
    pub transcript_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub citations: Vec<Citation>,
    pub question: Option<String>,
    pub created_at: String,
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub asset_id: String,
    pub asset_title: String,
    pub transcript_id: String,
    pub segment_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub total_mb: u64,
    pub free_mb: u64,
    pub driver: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemResources {
    pub cpu_name: String,
    pub logical_cores: u32,
    pub ram_total_mb: u64,
    pub ram_available_mb: u64,
    pub disk_free_mb: u64,
    pub gpu: Option<GpuInfo>,
    pub cuda_available: bool,
    pub python_available: bool,
    pub torch_version: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRecommendation {
    pub model: String,
    pub device: String,
    pub threads: u32,
    pub gpu_concurrency: u32,
    pub max_gpu_concurrency: u32,
    pub reason: String,
    pub warnings: Vec<String>,
}
