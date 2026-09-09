# Third-party software

Course Workbench follows the existing repository's Apache-2.0 license. It uses
independently distributed dependencies with their own licenses; see Cargo.lock
and apps/desktop/package-lock.json for resolved versions.

- Tauri and the Rust/Tauri plugins: MIT / Apache-2.0, as specified upstream.
- React and Vite: MIT. TypeScript: Apache-2.0, as specified upstream.
- SQLite: public domain; rusqlite and jieba-rs retain their upstream licenses.
- OpenAI Whisper: MIT; its model weights and Python package are installed
  separately. The bundled worker is original integration code.
- yt-dlp and FFmpeg are invoked as external programs; their distribution terms
  depend on the selected build. They are not bundled into the desktop EXE.
- PyTorch and NumPy retain their upstream licenses and are installed separately.
- Lucide icons: ISC.

bili2text (https://github.com/lanbinleo/bili2text, MIT) was used as a functional
and performance reference. No bili2text source code is included in this repo.
If code is reused later, include the required original copyright and license.
