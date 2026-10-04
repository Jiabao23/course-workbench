# Performance and quality implementation plan

**Goal:** Ship measurable local processing improvements with recoverable versions and explicit quality limits.
**Architecture:** Preserve JSONL v1 and immutable transcripts; add deterministic interval utilities, audio-identity evidence reuse, per-stage measurements, optional engine adapters and an experimental chunk manifest. Production defaults change only with evidence. Reference text requires human verification.
**Stack:** Rust/Tauri/SQLite, Python Whisper plus isolated CTranslate2, React/TypeScript.

## 1. Baseline and measurement

- [x] Save approved proposal and this plan; keep work on existing feature branch, reuse installed build tools, isolate all test data.
- [x] Add `benchmarks/quality/evaluate.py`, tests and reference JSON schema. Compute deterministic CER, term errors and speech interval precision/recall only for explicitly reviewed references. Reject unreviewed/missing references; record performance independently.
- [x] Record fixture/audio hashes, engine/runtime/options, cold/warm/checkpoint state, end-to-end and inference time, RAM/GPU metric definitions. Add reproducible commands and raw-result locations.
- [x] Run `python -m unittest discover -s benchmarks/quality/tests -v`; use constructed truth only for metric correctness, not claims about actual ASR.

## 2. Existing pipeline optimization

- [x] Add `crates/core/src/quality_intervals.rs` and oracle/random boundary tests; integrate into desktop `integrity.rs`. Benchmark old/new results on long-course-sized generated timelines, require identical uncovered intervals.
- [x] Replace diagnostic scans with validated ID lookup. Cache audio speech evidence by full content/detector identity and attach to each transcript without copying old review conclusions. Add schema migration, failure/history/invalidation tests.
- [x] Add grouped doubt presentation, separate evidence-insufficiency count, candidate text diff and bounded interval loop playback. Keep individual evidence/review records and unsaved guards; test helpers and actual desktop controls.
- [x] Run `cargo test --workspace --locked`, frontend `npm test` and production build.

## 3. Engine and timing adapter

- [x] Extend worker optional `engine`, `compute_type`, beam/context configuration and timings; default remains existing OpenAI Whisper. Bind all options into checkpoint identity; preserve old protocol compatibility and reject unsupported combinations.
- [x] Add lazy-import faster-whisper adapter with local-only model loading, capability probe, standardized segments/diagnostics and correct non-Torch GPU metric disclosure. Add Rust settings/probe/routing and UI advanced configuration. Add missing environment/model and engine-specific retry tests.
- [x] Install independent runtime after current driver detection; never modify bili2text. Test same multilingual small model with matching beam/thread/language and actually consume generator results. Record CUDA compatibility failure separately from CPU success.
- [x] Compare baseline/Faster FP16/INT8 only when supported; use actual total processing time and resources. Do not infer CER from output agreement.

## 4. Experimental chunking and targeted recheck

- [x] Add deterministic manifest planner with stop-boundary preference and bounded fallback; full timeline remains covered. Context expansion never changes ownership. Bind manifest/planner version to checkpoints; validate completion against manifest.
- [x] Test continuous speech, overlap, last sub-second, repeated words, timestamp ownership, missing chunk, cancellation/recovery. Keep production fixed chunking until real boundary/quality evidence supports promotion.
- [x] Add bounded grouped recheck selection with explicit range/cost preview and persisted candidates; changed context/decoding policy is recorded, never silently adopted. Enforce total budget and resource exclusion.
- [x] Profile repeated local requests. Implement model session retention only if measured benefit justifies extra protocol/lifetime risk; otherwise record decision and keep process isolation.

## 5. Verification and delivery

- [x] Spec review then independent correctness review; fix findings with regressions.
- [x] Full Rust/Python/frontend checks, fmt/Clippy/build. Real CPU/GPU tests and desktop loop in isolated library/vault; missing/manual-reference limits stated explicitly.
- [x] Update contracts, user guide, benchmark report and acceptance. Package, back up original library/settings, install and verify original rows/citations unchanged.
- [ ] Commit/push existing feature branch and verify remote SHA/CI state without claiming queued CI passed.

Useful commands (PowerShell at repo root): `. .\scripts\dev-env.ps1`; `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `D:\bili2text\.venv\Scripts\python.exe -m unittest discover -s workers/asr/tests -v`; in apps/desktop: `npm test`, `npm run package`.

Reference/CER gating is a real data dependency: prepare review artifacts and metrics, mark unreviewed audio honestly, continue all independent implementation and hardware measurements. No existing test result establishes human word accuracy.

Measured decisions: faster-whisper is optional; fixed chunking remains default. Both real experimental boundary samples failed alignment validation safely, so quality promotion is not accepted. Model retention is deferred based on recorded load/inference times and 4GB lifecycle costs. See the validation and benchmark reports for exact test scope.
