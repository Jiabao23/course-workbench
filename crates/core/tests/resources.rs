use course_core::{
    resources::recommend,
    types::{GpuInfo, SystemResources},
};

fn resources(cuda: bool, total_mb: u64, free_mb: u64) -> SystemResources {
    SystemResources {
        cpu_name: "测试 CPU".into(),
        logical_cores: 8,
        ram_total_mb: 16_384,
        ram_available_mb: 8192,
        disk_free_mb: 100_000,
        gpu: Some(GpuInfo {
            name: "测试 GPU".into(),
            total_mb,
            free_mb,
            driver: "test".into(),
        }),
        cuda_available: cuda,
        python_available: true,
        torch_version: Some("test".into()),
        warnings: vec![],
    }
}

#[test]
fn gpu_presence_without_actual_cuda_support_falls_back_to_cpu_base() {
    let rec = recommend(&resources(false, 24_576, 24_000), "quality");
    assert_eq!(rec.device, "cpu");
    assert_eq!(rec.model, "base");
    assert_eq!(rec.gpu_concurrency, 1);
    assert_eq!(rec.max_gpu_concurrency, 1);
    assert!(!rec.reason.is_empty());
}

#[test]
fn four_gigabyte_card_can_offer_small_only_with_sufficient_free_headroom() {
    let candidate = recommend(&resources(true, 4096, 3500), "balanced");
    assert_eq!(candidate.device, "cuda");
    assert_eq!(candidate.model, "small");
    let busy = recommend(&resources(true, 4096, 500), "balanced");
    assert_eq!(busy.device, "cpu");
    assert_eq!(busy.model, "base");
    assert!(
        !candidate.warnings.is_empty(),
        "candidate must not imply a measured benchmark"
    );
}

#[test]
fn total_vram_does_not_increase_concurrency_or_hide_busy_memory() {
    let large = recommend(&resources(true, 98_304, 90_000), "quality");
    assert_eq!(large.gpu_concurrency, 1);
    assert_eq!(large.max_gpu_concurrency, 1);
    let busy_large = recommend(&resources(true, 98_304, 100), "quality");
    assert_eq!(busy_large.device, "cpu");
}

#[test]
fn missing_probes_and_small_cpu_counts_stay_conservative() {
    let mut machine = resources(true, 4096, 4096);
    machine.gpu = None;
    machine.logical_cores = 0;
    machine.python_available = false;
    machine.warnings.push("探测提示".into());
    let rec = recommend(&machine, "eco");
    assert_eq!(rec.device, "cpu");
    assert_eq!(rec.threads, 1);
    assert!(rec.warnings.iter().any(|warning| warning == "探测提示"));
    assert!(rec
        .warnings
        .iter()
        .any(|warning| warning.contains("Python")));
}

#[test]
fn simulated_eight_gb_gpu_changes_candidates_with_actual_free_memory() {
    // Synthetic policy inputs, not a measured 8 GB hardware benchmark.
    let free = recommend(&resources(true, 8192, 7900), "quality");
    let shared = recommend(&resources(true, 8192, 3000), "quality");
    let busy = recommend(&resources(true, 8192, 600), "quality");
    assert_eq!(
        (free.device.as_str(), free.model.as_str()),
        ("cuda", "medium")
    );
    assert_eq!(shared.model, "small");
    assert_eq!(busy.device, "cpu");
    for profile in [free, shared, busy] {
        assert_eq!(
            (profile.gpu_concurrency, profile.max_gpu_concurrency),
            (1, 1)
        );
    }
}

#[test]
fn simulated_twenty_four_gb_gpu_never_approves_concurrency_from_capacity() {
    // Synthetic policy inputs, not a measured 24 GB hardware benchmark.
    let free = recommend(&resources(true, 24576, 22000), "quality");
    let shared = recommend(&resources(true, 24576, 4000), "quality");
    let busy = recommend(&resources(true, 24576, 500), "quality");
    assert_eq!(free.model, "large-v3");
    assert_eq!(shared.model, "small");
    assert_eq!(busy.device, "cpu");
    for profile in [free, shared, busy] {
        assert_eq!(
            (profile.gpu_concurrency, profile.max_gpu_concurrency),
            (1, 1)
        );
    }
}
