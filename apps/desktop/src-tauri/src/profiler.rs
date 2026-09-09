use super::{
    asr::{AsrEngine, WhisperWorker},
    process::{self, ProcessControl},
    settings::{find_program, AppSettings},
};
use anyhow::Result;
use course_core::{resources::recommend, GpuInfo, ResourceRecommendation, SystemResources};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use sysinfo::{Disks, System};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceReport {
    pub resources: SystemResources,
    pub recommendation: ResourceRecommendation,
    pub models: Vec<Value>,
    pub dependencies: Value,
    pub engine_version: Option<String>,
}

pub trait ResourceProfiler: Send + Sync {
    fn profile(&self, settings: &AppSettings) -> Result<ResourceReport>;
}
pub struct LocalProfiler {
    pub worker_path: std::path::PathBuf,
}

pub fn basic_resources(settings: &AppSettings) -> SystemResources {
    let system = System::new_all();
    let disks = Disks::new_with_refreshed_list();
    let directory =
        dunce::canonicalize(settings.data_path()).unwrap_or_else(|_| settings.data_path());
    let disk = disks
        .iter()
        .filter(|disk| directory.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len());
    SystemResources {
        cpu_name: system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().to_owned())
            .unwrap_or_else(|| "未知 CPU".into()),
        logical_cores: system.cpus().len().max(1) as u32,
        ram_total_mb: system.total_memory() / 1024 / 1024,
        ram_available_mb: system.available_memory() / 1024 / 1024,
        disk_free_mb: disk
            .map(|disk| disk.available_space() / 1024 / 1024)
            .unwrap_or(0),
        gpu: None,
        cuda_available: false,
        python_available: false,
        torch_version: None,
        warnings: vec![],
    }
}

impl ResourceProfiler for LocalProfiler {
    fn profile(&self, settings: &AppSettings) -> Result<ResourceReport> {
        let mut resources = basic_resources(settings);
        let control = ProcessControl::default();
        let smi = find_program(&["nvidia-smi.exe", "nvidia-smi"]);
        if !smi.is_empty() {
            let mut command = process::command(&smi)?;
            command.args([
                "--id=0",
                "--query-gpu=name,memory.total,memory.free,driver_version",
                "--format=csv,noheader,nounits",
            ]);
            if let Ok(output) = process::capture(
                command,
                &control,
                &settings.data_path().join("logs/gpu-probe.log"),
                Duration::from_secs(12),
            ) {
                if let Some(line) = output.lines().next() {
                    let fields: Vec<_> = line.split(',').map(str::trim).collect();
                    if fields.len() == 4 {
                        resources.gpu = Some(GpuInfo {
                            name: fields[0].into(),
                            total_mb: fields[1].parse().unwrap_or(0),
                            free_mb: fields[2].parse().unwrap_or(0),
                            driver: fields[3].into(),
                        });
                    }
                }
            }
        }
        let mut models = vec![];
        let mut dependencies = json!({});
        let mut engine_version = None;
        if !settings.python_path.is_empty() {
            let worker = WhisperWorker {
                path: self.worker_path.clone(),
            };
            match worker.execute(
                settings,
                json!({"command":"probe","job_id":"resource-probe"}),
                &control,
                &mut |_| Ok(()),
            ) {
                Ok(probe) => {
                    dependencies = probe["dependencies"].clone();
                    engine_version = probe["engine_version"].as_str().map(str::to_owned);
                    resources.python_available = dependencies["whisper"].as_bool().unwrap_or(false)
                        && dependencies["torch"].as_bool().unwrap_or(false)
                        && dependencies["numpy"].as_bool().unwrap_or(false);
                    resources.cuda_available = probe["cuda_available"].as_bool().unwrap_or(false)
                        && resources.python_available;
                    resources.torch_version = probe["torch_version"].as_str().map(str::to_owned);
                    if let Some(gpu) = resources.gpu.as_mut() {
                        if let Some(free) = probe["gpu_free_mb"]
                            .as_u64()
                            .filter(|_| resources.cuda_available)
                        {
                            gpu.free_mb = gpu.free_mb.min(free);
                        }
                    }
                    if resources.gpu.is_none() && probe["gpu_name"].is_string() {
                        resources.gpu = Some(GpuInfo {
                            name: probe["gpu_name"].as_str().unwrap_or_default().into(),
                            total_mb: probe["gpu_total_mb"].as_u64().unwrap_or(0),
                            free_mb: probe["gpu_free_mb"].as_u64().unwrap_or(0),
                            driver: "未检测到驱动版本".into(),
                        });
                    }
                    models = probe["models"].as_array().cloned().unwrap_or_default();
                    for warning in probe["warnings"].as_array().into_iter().flatten() {
                        if let Some(message) = warning.as_str() {
                            resources.warnings.push(message.into());
                        }
                    }
                }
                Err(error) => resources.warnings.push(format!("识别环境不可用：{error}")),
            }
        }
        if !resources.python_available {
            resources.warnings.push(
                "未找到可用 Whisper 环境。字幕导入、校对和搜索仍可使用；请在设置中配置识别环境。"
                    .into(),
            );
        }
        if resources.gpu.is_some() && !resources.cuda_available {
            resources
                .warnings
                .push("检测到显卡，但当前 Python/CUDA/驱动组合不可用；可选择 CPU。".into());
        }
        if resources.gpu.is_none() {
            resources
                .warnings
                .push("首版实测支持 CPU 与 NVIDIA CUDA；其他 GPU 后端尚未验证。".into());
        }
        if settings.ffmpeg_path.is_empty() {
            resources
                .warnings
                .push("尚未配置 FFmpeg，音频转换不可用。".into());
        }
        let recommendation = recommend(&resources, &settings.preset);
        Ok(ResourceReport {
            resources,
            recommendation,
            models,
            dependencies,
            engine_version,
        })
    }
}
