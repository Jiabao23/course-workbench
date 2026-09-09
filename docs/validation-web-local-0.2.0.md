# v0.2.0 网页与本地导入验证

日期：2026-09-09。开发机为 Windows 10 x64 / RTX 3050 Laptop 4 GB。以下区分真实来源、回环夹具与自动化边界；不将上游支持列表等同于本机全部验证通过。

## 实际公开视频

| 来源 | 本机结果 |
| --- | --- |
| [YouTube：Me at the zoo](https://www.youtube.com/watch?v=jNQXAC9IVRw) | Rust 预览和桌面导入均完成，19 秒，英文人工字幕 6 个片段。没有媒体下载或 ASR 缓存。桌面保存一条片段引用笔记并导出 Markdown，出处与锚点对应实际片段 |
| [Vimeo：76979871](https://vimeo.com/76979871) | 解析器明确要求登录，工作台显示网站 Cookie 配置提示；没有可用账号，未记为导入通过 |
| [TED：Do schools kill creativity?](https://www.ted.com/talks/ken_robinson_do_schools_kill_creativity) | 当前 yt-dlp 解析响应时发生 NoneType 错误，未记为导入通过 |

使用官方独立 `yt-dlp.exe` 2026.08.19 与 Node.js 24。下载工具已与该版本官方 `SHA2-256SUMS` 校验；SHA256 为 `66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a`。原 Whisper Python 环境未修改。

## 回环网页与失败路径

使用仓库 `tests/fixtures/media_site.py`，只监听 `127.0.0.1`。这是真实 HTTP → yt-dlp → Rust → SQLite 的集成验证，网站内容是本机夹具。

- `/with-subtitles`：直接生成 2 个 `webSubtitle` 片段；请求日志只含页面和 `lesson.vtt`，没有 `sample.mp4` 请求。资产没有音频路径、模型和 ASR 检查点。
- `/without-subtitles`：预览提示下载混合媒体，实际获取 MP4、转换为 16 kHz WAV，再以 small/CUDA 完成 7 个片段，回听文件存在。
- 同一无字幕网页的 `subtitlesOnly` 任务明确失败，不自动转写。
- `/unavailable`：503 在 Rust 和桌面界面都显示为请求失败，没有被误报为“无字幕”。
- 自动化测试另覆盖播放列表、直播/预约、DRM、登录要求、不可读字幕格式与无下载工具。人工字幕不可读而同语言自动字幕可读时，下载命令只请求选中的自动字幕，避免上游优先选择人工轨道。

复现服务命令（`site` 目录需自行放入 `sample.mp4` 与 `lesson.vtt`，不提交个人媒体）：

```powershell
python tests/fixtures/media_site.py --directory .local-data/web-import-tests/site --log .local-data/web-import-tests/site-requests.jsonl --port 18444
```

使用隔离配置和资料库运行 `cw-diagnostic --config <配置> preview/import/detail/export`；不要同时打开占用同一资料库的桌面程序。记录和导出保存在 `.local-data/web-import-tests/`，界面截图保存在 `output/playwright/`，均不进入 Git。

## 桌面本地文件流程

通过真实 Tauri WebView2 和“本地文件”路径输入操作，完成以下验证：

| 文件 | 结果 |
| --- | --- |
| `本地 音频.WAV`，含空格、中文与大写扩展名 | 20 秒，small/CUDA，8 个片段；点击时间戳后播放器持续前进，readyState=4、无播放错误；保存带 1 个引用的笔记；四种格式导出成功 |
| `sample.mp4` | 20.032 秒，small/CUDA，7 个片段，回听 WAV 可加载且时长正确 |
| `lesson.vtt` | 2 个片段，没有音频播放器，没有 ASR |

Markdown 导出包含真实引用锚点，SRT 保留时间轴，VTT 有正确头部。不存在的文件与不支持扩展名均显示错误。网页读取尚未返回时切换到本地字幕，后来的网页响应没有覆盖本地预览。多文件、相对路径、URL 冒充文件及空输入通过纯函数测试拒绝。

原生文件对话框已接入 Tauri 插件，拖入已接入 webview 原生事件。本轮自动化未完成系统文件选择和跨窗口拖入的操作闭环：本机 Windows 自动化工具截图报 `SetIsBorderRequired ... 0x80004002`，对话框期间又无法激活目标窗口。已确认点击调用进入原生选择请求，但没有把路径输入测试或合成事件当作原生选择/拖入实测。该项仍需人工操作复验。

短样本处理时间与资源峰值见 [实测记录](../benchmarks/2026-09-09-web-local.md)。这些结果不代表中文准确率、其他网站账号条件、其他 GPU 或干净系统安装已经验证。

## Windows 构建产物

正式配置下的 `npm run package` 已完成，内置前端通过 TypeScript/Vite 构建，EXE 文件版本与产品版本均为 `0.2.0`。NSIS 安装包为 `Course Workbench_0.2.0_x64-setup.exe`，本地交付副本为 `output/release/Course-Workbench-0.2.0-windows-x64-setup.exe`，大小 12,186,271 字节。

安装包 SHA256：`74b64cdb4aeaaf02644d3b4d4fb4c7157578a5128f0ee517f204884c6bde5918`。同目录提供 `.sha256` 文件。构建结果不等同于安装验证；旧版窗口仍在运行，待保存关闭后再升级及复验，避免丢失未保存的编辑。
