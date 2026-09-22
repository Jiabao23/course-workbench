# Transcript quality and focused reader implementation plan

**Goal:** Find and resolve individual transcript doubts with audio evidence, preserve versions, and simplify reading.
**Architecture:** Extend immutable quality evidence and version-bound reviews in SQLite schema 5. A CPU VAD worker supplies independent speech intervals; optional Whisper diagnostics flag suspects. Local rechecks create persisted candidates; explicit adoption creates a new transcript version. Reuse the auxiliary process gate for resource exclusion/cancellation.
**Tech Stack:** Rust/Tauri, SQLite, React/TypeScript, Python/Whisper, Silero VAD CPU.

- [x] Add failing tests for individual review persistence, invalid/stale reviews, evidence and partial candidate lifecycle; implement schema 5 storage and service contracts.
- [x] Extend Python diagnostics without breaking old checkpoints; separate bounded CPU VAD command and deterministic tests. Install isolated optional VAD dependency, record actual sample execution.
- [x] Integrate audio detection with evidence hashing/cache, unavailable states and cancellation; add missing-speech/diagnostic rules and tests.
- [x] Implement local interval recheck with 120-second limit, context, shared GPU exclusion, persisted candidate, explicit adoption and stale-base rejection. Preserve provenance and old citations.
- [x] Simplify Reader with collapsible auxiliary pane, quality summary, More menu and explicit excerpt mode; implement per-issue review and candidate comparison. Frontend helper tests first.
- [x] Run Rust/Python/frontend tests, fmt/Clippy/build; independently review spec compliance and quality.
- [x] Real isolated desktop validation: VAD, doubt review, local candidate/reject/adopt, version/citations, cancel, no-audio, narrow viewport and keyboard. Distinguish diagnostics from actual CER; no invented human reference.
- [ ] Package, back up production library, verify upgrade, document precise evidence/limits, commit and push feature branch.

Ownership: root owns database, Rust service/commands/contracts and integration. Worker implementation agent owns workers/asr only; reader implementation agent owns Reader/IntegrityPanel/new reader CSS/helpers and frontend api/types. Independent reviewers are read-only. Never modify D:\bili2text's existing environment; optional dependencies use a project-owned directory.

Validation evidence: docs/validation-quality-ux-0.5.0.md. Human reference/CER, noise/mixed-language expansion and other-device measurements remain explicitly unverified; no accuracy-improvement claim.
