//! A diagnostic companion for repeatable integration checks. It uses the same
//! Runtime as the desktop; it is not required for normal desktop operation.
use anyhow::{Context, Result};
use course_workbench_lib::{
    service::{CreateJobsRequest, Runtime},
    settings,
};
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let first=args.next().context("usage: cw-diagnostic [--config FILE] diagnose|preview|import|search|export|detail|jobs ...")?;
    let (config, command) = if first == "--config" {
        (
            PathBuf::from(args.next().context("missing config")?),
            args.next().context("missing command")?,
        )
    } else {
        (settings::default_config_path(), first)
    };
    let runtime = Runtime::new(config, course_workbench_lib::worker_path())?;
    let result = match command.as_str() {
        "detect-speech" => serde_json::to_value(runtime.detect_speech(
            &args.next().context("missing asset")?,
            &args.next().context("missing version")?,
        )?)?,
        "recheck" => serde_json::to_value(runtime.recheck_interval(
            &args.next().context("missing asset")?,
            &args.next().context("missing version")?,
            args.next().context("missing start ms")?.parse()?,
            args.next().context("missing end ms")?.parse()?,
        )?)?,
        "adopt-candidate" => serde_json::to_value(
            runtime.adopt_candidate(&args.next().context("missing candidate")?)?,
        )?,
        "discard-candidate" => {
            runtime.discard_candidate(&args.next().context("missing candidate")?)?;
            json!({"discarded":true})
        }
        "check" => serde_json::to_value(runtime.check_integrity(
            &args.next().context("missing asset")?,
            &args.next().context("missing version")?,
        )?)?,
        "vault-init" => json!({"path":runtime.initialize_vault()?}),
        "vault-sync" => serde_json::to_value(runtime.sync_vault(
            &args.next().context("missing asset")?,
            &args.next().context("missing version")?,
        )?)?,
        "diagnose" => serde_json::to_value(runtime.probe_resources()?)?,
        "preview" => {
            serde_json::to_value(runtime.probe_source(&args.next().context("missing source")?)?)?
        }
        "import" => {
            let source = args.next().context("missing source")?;
            let mode = args.next().unwrap_or_else(|| "auto".into());
            let pages = args
                .next()
                .unwrap_or_else(|| "1".into())
                .split(',')
                .map(str::parse)
                .collect::<std::result::Result<Vec<u32>, _>>()?;
            let jobs = runtime.create_jobs(CreateJobsRequest {
                source,
                pages,
                mode,
            })?;
            let started = Instant::now();
            loop {
                let current: Vec<_> = jobs
                    .iter()
                    .map(|job| runtime.db().get_job(&job.id))
                    .collect::<Result<_>>()?;
                if current
                    .iter()
                    .all(|job| !["running", "queued"].contains(&job.status.as_str()))
                {
                    break json!({"jobs":current,"assets":runtime.db().list_assets()?});
                }
                anyhow::ensure!(
                    started.elapsed() < Duration::from_secs(7200),
                    "diagnostic timeout"
                );
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        "detail" => {
            serde_json::to_value(runtime.asset_detail(&args.next().context("missing asset id")?)?)?
        }
        "jobs" => serde_json::to_value(runtime.db().list_jobs()?)?,
        "search" => {
            serde_json::to_value(runtime.search(&args.next().context("missing query")?, None)?)?
        }
        "export" => {
            json!({"path":runtime.export(&args.next().context("missing asset id")?,&args.next().context("missing format")?,&args.next().context("missing destination")?,None)?})
        }
        "benchmark" => serde_json::to_value(runtime.benchmark_profile(
            &args.next().context("missing asset id")?,
            &args.next().unwrap_or_else(|| "small".into()),
            &args.next().unwrap_or_else(|| "cuda".into()),
        )?)?,
        _ => anyhow::bail!("unknown command"),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
