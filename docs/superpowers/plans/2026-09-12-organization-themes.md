# Organization and Themes Implementation Plan

**Goal:** Organize personal courses into local knowledge bases/folders and favorites, with three persistent themes.
**Architecture:** Add isolated SQLite organization metadata and typed commands. Extract the library UI and centralize colors. Preserve assets, versions and Obsidian paths.
**Tech Stack:** Rust, SQLite schema 4, Tauri 2, React/TypeScript, CSS custom properties.

- [x] Add failing core organization tests: nested collections, duplicate/invalid names, atomic batch move, favorite independence, nonempty deletion, migration and reopen. Run `cargo test -p course-core --test organization`.
- [x] Implement `crates/core/src/organization.rs`, schema migration and records; add Runtime commands and Bootstrap organization snapshot. Run targeted tests and Clippy.
- [x] Build `LibraryView.tsx` with tree navigation, title filter, favorites and batch move. Preserve filter on return and reset on data directory change; test filtering helpers.
- [x] Add theme setting validation and persistence tests; implement three palettes, preview/save/discard and startup cache. Check production TypeScript build.
- [x] Exercise real Tauri UI in an isolated library and record screenshots, classification/restart/empty deletion/old content behavior and all themes.
- [x] Run full workspace tests, Python tests, frontend tests, fmt, Clippy and NSIS build. Back up old library and validate installed upgrade when no user window has unsaved edits.
- [ ] Document exact evidence/limits, independently review, commit and push existing feature branch; verify remote SHA and CI status.

- [x] Add “收纳到…” popup with inline branches, independent main/classification collapse and keyboard focus.
- [x] Add short layout/menu/favorite feedback and reduced-motion support; independent incremental review found no outstanding issue.
- [x] Verify final motion and minimum window layout in packaged WebView2.
