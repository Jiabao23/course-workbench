# Course Workbench implementation

This is a new Windows-first personal desktop app. Approved stack: Rust core,
Tauri 2, React/TypeScript, independent Python Whisper worker. Keep all media,
transcripts and search local. Cloud knowledge requests are user-initiated and
only send selected transcript text. Never store API keys in JSON or SQLite.

Use the contract in contracts/implementation-contract.md. Serialized Rust
records use camelCase. Python worker protocol uses snake_case and version 1.
Keep service orchestration separate from pure parsing, storage and export.

The root agent owns integration, tool installation, desktop Rust bridge and
Python worker. Assigned agents must edit only their named files. Do not change
shared contracts without notifying the root agent. Do not reset or clean other
work. Network installs and global git operations are owned by the root agent.

Use meaningful tests for behavior, including failure paths. Mark simulated,
mocked and actual hardware/provider tests distinctly. Never claim a cloud API
or hardware configuration passed without an actual corresponding run.

UI direction: quiet Chinese desktop workspace; warm white canvas, forest-green
accent, readable list and three-pane transcript reader, minimal card decoration.
Product copy must explain actual status and action, never implementation slogans.
