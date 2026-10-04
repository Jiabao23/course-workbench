use super::{
    profiler::ResourceReport,
    settings::{atomic_write, AppSettings},
};
use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRecord {
    pub id: String,
    pub asset_id: String,
    pub model: String,
    pub device: String,
    pub threads: u32,
    pub gpu_concurrency: u32,
    pub audio_seconds: f64,
    pub elapsed_seconds: f64,
    pub peak_ram_mb: Option<f64>,
    pub peak_gpu_mb: Option<f64>,
    pub peak_gpu_reserved_mb: Option<f64>,
    pub environment_fingerprint: String,
    pub created_at: String,
    pub success: bool,
    pub error: Option<String>,
    pub tested: bool,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub compute_type: String,
    #[serde(default)]
    pub measurements: Value,
}

pub fn fingerprint(settings: &AppSettings, report: &ResourceReport) -> String {
    let raw = serde_json::json!({"python":settings.python_path,"torch":report.resources.torch_version,
        "gpu":report.resources.gpu.as_ref().map(|g|(&g.name,&g.driver)),"cpu":report.resources.cpu_name,
        "worker":env!("CARGO_PKG_VERSION"),"engine":report.engine_version,
        "asr_engine":settings.asr_engine,"asr_python":settings.asr_python(),"runtime_versions":report.runtime_versions,
        "compute_type":settings.compute_type,"beam_size":settings.beam_size,"threads":settings.threads});
    hex::encode(Sha256::digest(raw.to_string().as_bytes()))
}

pub fn from_result(
    settings: &AppSettings,
    report: &ResourceReport,
    asset_id: &str,
    model: &str,
    device: &str,
    audio_seconds: f64,
    result: &Value,
) -> BenchmarkRecord {
    BenchmarkRecord {
        id: uuid::Uuid::new_v4().to_string(),
        asset_id: asset_id.into(),
        model: model.into(),
        device: device.into(),
        threads: settings.threads,
        gpu_concurrency: 1,
        audio_seconds,
        elapsed_seconds: result["elapsed_seconds"].as_f64().unwrap_or(0.0),
        peak_ram_mb: result["peak_ram_mb"].as_f64(),
        peak_gpu_mb: result["peak_gpu_mb"].as_f64(),
        peak_gpu_reserved_mb: result["peak_gpu_reserved_mb"].as_f64(),
        environment_fingerprint: fingerprint(settings, report),
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        success: true,
        error: None,
        tested: true,
        engine: settings.asr_engine.clone(),
        compute_type: settings.compute_type.clone(),
        measurements: serde_json::json!({"timings":result["timings"],"process_seconds":result["process_seconds"],"gpu_metric":result["gpu_metric"],"checkpoint_key":result["checkpoint_key"],"resumed_chunks":result["resumed_chunks"],"engine_version":result["engine_version"],"runtime_versions":result["runtime_versions"],"model_sha256":result["model_sha256"],"decode_options":result["decode_options"]}),
    }
}

pub fn save(settings: &AppSettings, record: &BenchmarkRecord) -> Result<()> {
    atomic_write(
        &settings
            .data_path()
            .join("benchmarks")
            .join(format!("{}.json", record.id)),
        &serde_json::to_vec_pretty(record)?,
    )
}

pub fn list(settings: &AppSettings) -> Result<Vec<BenchmarkRecord>> {
    let mut records = vec![];
    let directory = settings.data_path().join("benchmarks");
    if !directory.exists() {
        return Ok(records);
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(record) = serde_json::from_slice::<BenchmarkRecord>(&fs::read(path)?) {
                records.push(record);
            }
        }
    }
    records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benchmark_validity_changes_with_runtime_engine_and_decode_options() {
        let settings = AppSettings::default();
        let resources = course_core::SystemResources {
            cpu_name: "fixture".into(),
            logical_cores: 8,
            ram_total_mb: 16000,
            ram_available_mb: 8000,
            disk_free_mb: 8000,
            gpu: None,
            cuda_available: false,
            python_available: true,
            torch_version: None,
            warnings: vec![],
        };
        let mut report = ResourceReport {
            recommendation: course_core::resources::recommend(&resources, "balanced"),
            resources,
            models: vec![],
            dependencies: serde_json::json!({}),
            engine_version: Some("1.0.3".into()),
            runtime_versions: serde_json::json!({"ctranslate2":"3.24.0"}),
        };
        let first = fingerprint(&settings, &report);
        report.runtime_versions = serde_json::json!({"ctranslate2":"4.0"});
        assert_ne!(first, fingerprint(&settings, &report));
        let first = fingerprint(&settings, &report);
        let mut changed = settings;
        changed.beam_size = 5;
        assert_ne!(first, fingerprint(&changed, &report));
    }
}
