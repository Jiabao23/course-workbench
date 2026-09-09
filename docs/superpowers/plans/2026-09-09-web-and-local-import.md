# Web and Local Import Implementation Plan

> **For agentic workers:** Use executing-plans to implement these tasks in the current personal project. Track completed work below.

**Goal:** Add generic video URLs and a visible native local-file import flow, with real desktop and additional failure-path validation.

**Architecture:** Keep the Bilibili provider; add a yt-dlp-backed SourceProvider and a shared command builder. Reuse the existing queue, ASR, immutable transcripts and export, with a new webMedia source kind.

**Tech Stack:** Rust, Tauri 2, React/TypeScript, yt-dlp, FFmpeg and the existing Python Whisper environment.

- [x] Add failing routing and source-link tests. Run `cargo test -p course-workbench --test service_flow web_sources_report_missing_downloader` and `npm --prefix apps/desktop test`.
- [x] Add `apps/desktop/src-tauri/src/web_source.rs`: validate HTTP(S), normalize single-video JSON, rank readable captions, build bounded metadata/subtitle commands and map extractor errors. Add `tests/web_sources.rs` in the desktop crate.
- [x] Integrate in `source.rs`, `service.rs`, `media.rs` and `lib.rs`: automatic provider selection, registered preview process controls, caption-first jobs, mixed-media fallback and source-aware export.
- [x] Refine `ImportDialog.tsx`, add `ImportDialog.css` and `import-utils.ts`: three entry tabs, native dialog and webview drag events, one-file validation, stale-result protection and source-aware copy. Extend Reader/App labels and original-source links.
- [x] Verify generic web captions and audio fallback with a loopback fixture and real YouTube. Verify local audio/video/subtitle path import, playback, notes, four exports and failure reporting in WebView2. Save outputs only under `.local-data` and `output`.
- [x] Address review findings with regressions: shared website Cookie isolation, selected automatic caption kind, and legacy Windows local-source identity preserving versions/notes.
- [x] Run 75 Rust, 8 frontend, 8 Python tests; fmt/clippy and production frontend build. Record actual websites and hardware versus fixture tests in docs.
- [ ] Complete native file-picker selection and cross-window drag/drop verification. Windows automation capture/activation is unavailable on this instance; path entry is verified, native input completion remains a manual check.
- [x] Build the normal version 0.2.0 NSIS package, check executable version and record package SHA256.
- [ ] Install version 0.2.0 and repeat the relevant UI path after the user saves and closes the running older application.
- [ ] Review staged files for private data, commit, and push the existing feature branch. Keep the existing user library and its historical notes.

Do not copy whole third-party extractors into the app. Use the upstream CLI contract and record the actual tool version in validation notes. No cloud-provider or other-GPU test is implied by this work.
