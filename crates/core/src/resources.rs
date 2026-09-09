use crate::types::{ResourceRecommendation, SystemResources};

/// A conservative candidate policy, not a benchmark or allocation guarantee.
pub fn recommend(resources: &SystemResources, preset: &str) -> ResourceRecommendation {
    let cores = resources.logical_cores.max(1);
    let threads = match preset {
        "eco" => (cores / 2).clamp(1, 4),
        "quality" => cores.saturating_sub(2).clamp(1, 12),
        _ => (cores / 2).clamp(1, 8),
    };
    let mut warnings = resources.warnings.clone();
    warnings.push("模型建议尚未经本机基准测试；首次转写时请观察速度和内存占用。".into());
    if !resources.python_available {
        warnings.push("尚未检测到可用的 Python 环境，请先完成转写环境设置。".into());
    }
    if resources.ram_available_mb < 2048 {
        warnings.push("当前可用内存少于 2 GB，建议关闭其他程序后转写。".into());
    }
    if resources.disk_free_mb < 2048 {
        warnings.push("当前磁盘可用空间少于 2 GB，请为模型和音频缓存释放空间。".into());
    }
    let mut recommendation = ResourceRecommendation {
        model: "base".into(),
        device: "cpu".into(),
        threads,
        gpu_concurrency: 1,
        max_gpu_concurrency: 1,
        reason: "使用 CPU 与 base 模型作为保守起点。".into(),
        warnings,
    };
    if !resources.cuda_available || !resources.python_available {
        recommendation.reason =
            "尚未确认当前 Python 环境可使用 CUDA，建议使用 CPU 与 base 模型。".into();
        return recommendation;
    }
    let Some(gpu) = &resources.gpu else {
        recommendation.reason = "缺少 GPU 可用显存探测结果，建议使用 CPU 与 base 模型。".into();
        return recommendation;
    };
    if gpu.total_mb == 0 || gpu.free_mb > gpu.total_mb {
        recommendation.reason = "显存探测结果不完整或不一致，建议使用 CPU 与 base 模型。".into();
        return recommendation;
    }
    // Reserve at least 512 MB and 20% of currently free memory for runtime
    // overhead. Total VRAM alone never authorizes a model or concurrency level.
    let headroom = (gpu.free_mb / 5).max(512);
    let usable = gpu.free_mb.saturating_sub(headroom);
    let model = match preset {
        "quality" if usable >= 11_264 => Some("large-v3"),
        "quality" if usable >= 6144 => Some("medium"),
        "eco" if usable >= 1024 => Some("base"),
        _ if usable >= 2304 => Some("small"),
        _ if usable >= 1024 => Some("base"),
        _ => None,
    };
    if let Some(model) = model {
        recommendation.model = model.into();
        recommendation.device = "cuda".into();
        recommendation.reason = format!(
            "已确认 CUDA 可用；当前空闲显存 {} MB，预留 {} MB 后可尝试 {}，GPU 同时只运行一个任务。",
            gpu.free_mb, headroom, model
        );
    } else {
        recommendation.reason = format!(
            "当前空闲显存仅 {} MB，预留运行余量后不足以推荐 GPU 模型，建议使用 CPU 与 base。",
            gpu.free_mb
        );
    }
    recommendation
}
