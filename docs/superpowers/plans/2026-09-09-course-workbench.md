# Course Workbench implementation plan

Goal: Windows-first local course workspace with Rust/Tauri/React, replaceable
Python ASR, subtitle-first ingest, adaptive resources, cited knowledge and export.

Approved scope and interfaces: see contracts/implementation-contract.md.

- [x] M0: bootstrap isolated project, Rust/MSVC and frontend dependencies;
  bind JSON Lines worker; store approved product/architecture/resource docs.
- [x] M1: SQLite versions + FTS, subtitle parsing/export, import previews and
  selected parts, local input and audio-only download, editor and reader.
- [x] M2: checkpoint worker, persistent queue, cancellation/retry, resource
  recommendations and benchmarks, model download and bounded cache cleanup.
- [x] M3: local Chinese search, manual notes, configurable text-only provider,
  validated citations, immutable note/version relationships, failure handling.
- [x] M4 local: core/worker/frontend tests; actual P01 run; actual Tauri build/package;
  UI inspection, independent reviews and honest acceptance report.
- [ ] M4 external: repeat installation on a separate clean Windows machine;
  verify the user's actual API provider when configured. See docs/acceptance.md.

Verification: cargo test --workspace; Python unittest worker suite; npm test;
npm run build; Windows bundle. Separate mock API tests from actual provider
tests. Without a configured user API key, record live AI integration unverified.

Design: warm white, forest-green action color, compact navigation, library list,
three-pane transcript and notes reader, restrained drawer/selection transitions.
