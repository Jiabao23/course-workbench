# 课程工作台 · Course Workbench

面向个人的本地课程资料库。把 B 站单视频、选定分 P、其他视频网站的单视频链接、音视频直链或本地文件，整理成可校对、可检索、带出处的学习材料。

**Windows 优先 · Rust + Tauri 2 + React · SQLite · 独立 Whisper worker**

有可读取字幕时直接提取，**不需要下载视频或音轨**。没有可用字幕时优先获取独立音轨并转写；来源只有混合音视频时，会提示下载媒体后提取音频。首次读取 B 站课程后默认只选第一 P；整套课程必须由使用者明确选择。

## 可以做什么

- 三个导入入口：B 站链接、其他视频网站、本地文件。本地支持选择文件、拖入一个文件和粘贴绝对路径，文件保存在本机。
- 识别“可提取字幕 / 没有字幕 / 需要登录 / 请求失败”，支持字幕优先、仅字幕、重新转写。
- 按时间戳回听、搜索和校对；每次保存建立新版本，历史文字和旧引用保留。
- 中文全文检索、手写笔记和摘录；可选 OpenAI 兼容文本 API 生成摘要、提纲、关键概念、学习笔记和单课程问答。
- 只发送明确选中的原文；校验返回引用，材料不足时明确提示。API 密钥保存在 Windows 凭据存储。
- 导出 TXT、Markdown、SRT、VTT。Markdown 包含当前所选版本的笔记、时间戳和片段链接。
- 检测 CPU、内存、磁盘、NVIDIA GPU 和实际 Python/CUDA 能力；提供省资源、均衡、高质量和手动配置。
- 单任务队列、取消、按原配置或新配置重试、分块检查点、缓存分类查看和清理。
- 转录完整性核对：检查时间轴和分块证据，定位疑似遗漏，保存版本绑定的人工结论。时间轴覆盖比例不代表准确率。
- 本地 Obsidian 知识库：同步文字、引用笔记和核对报告，保留历史快照与个人补充，无需社区插件。

- 知识库与嵌套分组、独立收藏；“收纳到…”面板支持批量归类和顺手新建分支。主导航与分类栏可分别收起。
- 森林浅色、暖纸米色、深海夜色；轻量动态反馈，适配系统减少动态效果。

## 开始使用

从本仓库提供的 Windows 构建产物安装，或按下方说明构建。安装后的应用通过开始菜单或快捷方式启动，不需要 Node、Rust 或开发终端。

1. 在“设置 → 资料与工具”选择资料目录与模型目录。开发数据建议放 D 盘；目录可以随时切换，切换不会自动搬迁文件。
2. 先导入一个 `.srt` 或 `.vtt` 即可体验校对、搜索和手写笔记，这条路径无需 Python、FFmpeg 或 API。
3. 需要音频转写时配置下方识别环境，再选择模型、下载并运行小样本验证。
4. 选择导入来源，核对标题、处理范围、时长和字幕状态后点击“开始处理”。仅提取字幕模式不会下载媒体。其他视频网站使用单视频链接，播放列表与直播暂不支持。
5. 在文字稿旁选择片段并写笔记。需要 AI 整理时，填写 API 地址、模型和密钥，再主动提交选中文字。
6. 展开完整性核对，回听疑点并记录结论。在设置中连接独立 Obsidian 库后，可把所查看的已保存版本同步成可搜索、可定位原文的知识快照。

v0.4.0 使用 SQLite schema 4。升级前退出应用并备份资料目录；旧版程序不能直接打开升级后的资料库。回退时需恢复升级前备份，不要手工修改 schema 版本号。

完整操作见 [使用指南](docs/user-guide.md)、[安装与识别环境](docs/setup.md)。

## 识别环境

识别进程与桌面应用分离，可复用已有 Python 环境。需要 `openai-whisper`、`torch`、`numpy`；通用网站解析及网络媒体下载需要 [yt-dlp](https://github.com/yt-dlp/yt-dlp)。可单独配置官方 `yt-dlp.exe`，无须改动识别环境。FFmpeg 与 ffprobe 通过独立可执行文件配置；需要 JavaScript 的网站解析器会使用检测到的 Node.js。

```powershell
# 在仓库根目录执行；安装到独立目录，不修改其他工具的环境。
powershell -File .\scripts\setup-cpu.ps1 -Directory D:\CourseWorkbenchRuntime -Python C:\Path\To\python.exe
```

脚本安装 CPU 依赖，适合没有可用 CUDA 的机器；有 NVIDIA 显卡时按 [PyTorch 官方安装选择器](https://pytorch.org/get-started/locally/)为独立环境选择与驱动兼容的组合。不要把本机的 CUDA 11.8 环境复制为所有用户的固定依赖。

本机验证：Ryzen 7 5800H、RTX 3050 Laptop 4 GB，Python 3.12.7、PyTorch 2.4.1+cu118、Whisper 20250625。约 357 秒中文课程使用 small/CUDA 的一次应用内识别耗时 60.484 秒。此数值仅为一次本机样本；**中文错误率与专业术语准确率尚未建立人工标注基准**。详见 [实测记录](benchmarks/README.md)。

## 开发与验证

需要 Windows 10/11 x64、WebView2、Node.js 22.12+、Rust stable MSVC、Visual Studio C++ Build Tools 和 Windows SDK。

```powershell
cd apps\desktop
npm ci
npm run desktop
```

本机的隔离工具链位于 `.tools`，如果使用该工具链，在仓库根目录先执行 `. .\scripts\dev-env.ps1`。普通开发者使用自行安装的工具链即可，无需此目录。

```powershell
# 仓库根目录
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python -m unittest discover -s workers/asr/tests -v
cd apps\desktop
npm test
npm run build
npm run package
```

安装包输出至 `target/release/bundle/nsis/`。发布包包含 Python worker，模型和 Python/FFmpeg 环境独立配置。开发数据库、媒体、模型、密钥、工具链和编译缓存不进入 Git。

## 文件与实现说明

| 目录 | 职责 |
| --- | --- |
| `apps/desktop` | Tauri 桌面桥接与 React 界面 |
| `crates/core` | 数据库、版本、字幕、导出、中文搜索和资源策略 |
| `workers/asr` | 版本化 JSON Lines Whisper 进程 |
| `contracts` | 桌面命令及 worker 协议 |
| `docs` | 产品、架构、资源策略、验收及使用说明 |
| `benchmarks` | 不含课程原文和媒体的实测摘要 |
| `.local-data` | 本机开发资料，Git 忽略 |

[产品范围](docs/product.md) · [架构](docs/architecture.md) · [资源策略](docs/resource-policy.md) · [验收状态与局限](docs/acceptance.md)

v0.3.0 增加完整性核对与独立 Obsidian 知识库桥接，105 项自动化测试及本机真实桌面/Obsidian 验证见[核对与知识库验收](docs/validation-integrity-obsidian-0.3.0.md)。

v0.2.0 增加通用网站和本地文件入口。网站覆盖参考 [yt-dlp 支持列表](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md)，实际可用性仍受当前版本、登录和地区/网络条件影响。本机已完成 YouTube 字幕直提；Vimeo 测试来源要求登录，TED 测试来源解析失败，未记为导入通过。应用不处理 DRM、直播和通用播放列表。

资源路径支持 CPU 和 NVIDIA CUDA；其他 GPU 后端、跨课程语义检索和实时转写仍在后续范围。没有提供 API 密钥时，云端模型的实际回答质量未被验证。详见 [验收记录](docs/acceptance.md)。

## 许可与来源

延续仓库原有 [Apache-2.0 许可](LICENSE)。[bili2text](https://github.com/lanbinleo/bili2text) 用作功能和性能对照，没有复制其源码。各独立依赖遵循各自许可，见 [第三方说明](THIRD_PARTY_NOTICES.md)。
