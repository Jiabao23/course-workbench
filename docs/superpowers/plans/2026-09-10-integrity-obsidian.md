# Integrity and Obsidian Implementation Plan

**Goal:** Detect likely incomplete transcripts, support version-bound manual verification, and publish traceable local knowledge snapshots to Obsidian.

**Architecture:** Separate integrity rules and vault file operations from Runtime. Reuse SQLite job-to-transcript evidence, immutable versions and source-aware playback. Add a review table without changing existing media or model data.

**Tech Stack:** Rust/SQLite, Tauri commands, React/TypeScript, local Markdown and Obsidian URI.

- [x] Add meaningful failing tests in desktop tests/integrity.rs and tests/vault.rs; run targeted cargo tests.
- [x] Implement integrity.rs rules, db.rs review persistence and originating-job lookup, Runtime commands and ASR completion gate. Test unknown durations, gaps, overlaps, repetition, chunk loss, changed report evidence and version isolation.
- [x] Implement vault.rs initialization, immutable snapshots, block links, personal notes preservation, index append and URI construction. Test repeated sync, changed notes, modified-file conflicts, invalid paths and reparse points.
- [x] Connect typed commands and API; add Reader integrity/sync panel and Settings vault controls. Check stale responses and unsaved-edit protection.
- [x] Add service-level integration tests covering import, check, review, edit, vault sync, reopen and citations. Run real local course checks and Obsidian bridge with a dedicated D-drive vault.
- [x] Run cargo test --workspace --locked, cargo fmt --all -- --check, cargo clippy --workspace --all-targets --locked -- -D warnings, Python unittest, npm test and npm run package. 105 tests passed; NSIS installed and real production-library upgrade verified. See docs/validation-integrity-obsidian-0.3.0.md.
- [x] Document exact results and remaining limits; independently review implementation and exclude local data from version control.

Publication: commit and push the existing feature branch after final staging review; verify the remote SHA and report CI separately from local checks. Git history records publication status.
