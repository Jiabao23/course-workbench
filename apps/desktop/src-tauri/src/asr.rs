use super::{
    process::{self, ProcessControl},
    settings::AppSettings,
};
use anyhow::{anyhow, ensure, Context, Result};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub trait AsrEngine: Send + Sync {
    fn execute(
        &self,
        settings: &AppSettings,
        request: Value,
        control: &ProcessControl,
        callback: &mut dyn FnMut(&Value) -> Result<()>,
    ) -> Result<Value>;
}

pub struct WhisperWorker {
    pub path: std::path::PathBuf,
}
impl AsrEngine for WhisperWorker {
    fn execute(
        &self,
        settings: &AppSettings,
        mut request: Value,
        control: &ProcessControl,
        callback: &mut dyn FnMut(&Value) -> Result<()>,
    ) -> Result<Value> {
        ensure!(
            self.path.is_file(),
            "识别程序资源缺失，请重新安装课程工作台"
        );
        request["protocol_version"] = json!(1);
        request["model_dir"] = json!(settings.model_dir);
        let job_id = request["job_id"].as_str().unwrap_or("probe").to_owned();
        ensure!(
            job_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "任务标识不合法"
        );
        let mut command = process::command(&settings.python_path)?;
        command.arg(&self.path);
        if matches!(
            request["command"].as_str(),
            Some("detect_speech" | "detector_identity")
        ) && !settings.quality_packages_dir.is_empty()
        {
            command.env("PYTHONPATH", &settings.quality_packages_dir);
        }
        let input = format!("{}\n", serde_json::to_string(&request)?);
        let log = settings
            .data_path()
            .join("logs")
            .join(format!("worker-{job_id}.log"));
        let timeout = if request["command"] == "probe" {
            Duration::from_secs(90)
        } else if request["command"] == "detector_identity" {
            Duration::from_secs(15)
        } else if request["command"] == "detect_speech" {
            Duration::from_secs(2 * 3600)
        } else {
            Duration::from_secs(48 * 3600)
        };
        let mut done = None;
        let mut worker_error = None;
        let result = process::run_lines(
            command,
            Some(input.as_bytes()),
            control,
            &log,
            timeout,
            |line| {
                let message: Value = serde_json::from_str(line)
                    .context("识别进程输出了无效协议消息，请查看诊断日志")?;
                ensure!(message["protocol_version"] == 1, "识别进程协议版本不匹配");
                ensure!(
                    message["job_id"].as_str().unwrap_or("") == job_id,
                    "识别进程返回了其他任务的消息"
                );
                match message["type"].as_str() {
                    Some("done") => done = Some(message.clone()),
                    Some("error") => {
                        worker_error = Some(format!(
                            "{}: {}",
                            message["code"].as_str().unwrap_or("worker_error"),
                            message["message"].as_str().unwrap_or("识别失败")
                        ))
                    }
                    _ => (),
                }
                callback(&message)
            },
        );
        if let Some(error) = worker_error {
            return Err(anyhow!(error));
        }
        result?;
        done.context("识别进程未返回完成结果")
    }
}

pub fn transcribe_request(
    settings: &AppSettings,
    job_id: &str,
    audio: &Path,
    model: &str,
    device: &str,
    checkpoint_dir: &Path,
) -> Value {
    json!({"command":"transcribe","job_id":job_id,"audio_path":audio,"model":model,"device":device,
        "checkpoint_dir":checkpoint_dir,"threads":settings.threads,"language":settings.language,
        "prompt":settings.prompt,"chunk_seconds":300,"allow_download":false})
}
