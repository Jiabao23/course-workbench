# v0.3.0 完整性核对与 Obsidian 验证

日期：2026-09-10。Windows 10，本机 RTX 3050 Laptop 4 GB，small / CUDA，8 线程、GPU 并发 1。测试数据由 SQLite online backup 复制到隔离资料库；没有在原始课程上制造缺块或修改原文。

## 自动化验证

- Rust workspace：89 项通过，另 1 个仅供父测试启动的子进程辅助入口标为 ignored。
- Python worker：`python -m unittest discover -s workers/asr/tests -v`，8 项通过。
- 前端 Vitest：8 项通过。合计 **105 项**，较 v0.2.0 新增 14 项。
- `cargo fmt --all -- --check`、Clippy `--workspace --all-targets --locked -- -D warnings`、TypeScript/Vite production build 通过。

新增规则/集成测试覆盖中间缺段、尾部截短、时长未知、重叠区间并集、重复文字、下载时长差异、缺块拒绝、300 秒边界的亚毫秒尾块、同长音频改变检查依据、校对版本不借用旧任务证据。服务测试覆盖核对记录重启持久化、旧报告拒绝、新版本隔离和带引用知识库同步。

知识库文件测试覆盖幂等同步、内容变化新增快照、保留个人补充、已修改快照冲突、外来笔记/非法 ID 拒绝及 Windows junction 路径隔离。四项独立审查发现均已修复；相关新增回归先失败再通过，复核无阻塞性问题。

## 真实工作台验证

| 材料 | 观察结果 |
|---|---|
| 《课程目标》356.608 秒旧文字稿 | 158 段，时间轴覆盖 97.4%；缺原始关联任务证据，明确显示待核对，未冒充任务证据齐全 |
| 中文路径、空格、大写扩展名的 20 秒 WAV | 真实桌面导入 → small/CUDA → 1/1 块 → 8 段，提交完成结果；报告显示 89.4% 时间轴覆盖及 1/1 块 |
| WAV 校对新版本 | 保存为 v2；不继承 v1 人工结论与完成标记，切回 v1 可读原记录 |

真实 Tauri WebView2 操作还覆盖：展开报告、疑点时间播放原音轨、保存人工结论、重复同步、设置页初始化、未保存结论时禁用校对和同步。上述覆盖比例来自时间区间并集，不是中文识别准确率。

## 真实 Obsidian 桥接

使用已安装的 Obsidian **1.13.7**，独立库 **`D:\CourseWorkbenchKnowledge`**。原有另一个 Obsidian 库未被改写。

- Windows 原生文件夹选择器无法可靠激活，因此通过 Obsidian 自带 CLI 与其现有“打开库”接口注册独立目录；不把它算作原生选择器验收通过。
- 工作台“在 Obsidian 打开课程”确实通过 URI 打开对应课程索引；CLI 查询活动文件与 vault 路径确认一致。
- Obsidian 自身索引识别长课 158 个块和新音频 8 个块；课程关键词搜索命中，`unresolved` 返回无断链。
- 通过 Obsidian 的 `openLinkText` 打开导出笔记中的实际 `#^s-…` 引用，定位到目标原文块并高亮；不是只检查生成字符串。
- 在 Obsidian 内向新课程的 `个人笔记.md` 追加验收补充，再从工作台同步原版及修订版，追加内容保留；v1/v2 快照均存在。
- 本地截图：`output/playwright/integrity-vault-v030.png`、`output/playwright/obsidian-v030.png`（不进入 Git）。

## 打包与升级

Windows NSIS v0.3.0 构建通过，安装到 `D:\CourseWorkbench`，安装程序退出码 0，EXE 产品版本为 0.3.0。可交付包：`output/release/Course-Workbench-0.3.0-windows-x64-setup.exe`，SHA256：`14a16e7e770eeba808994497b785096e34ea971798f5c6ae0509388b47940639`。

原始 schema 2 资料库已在升级前在线备份，备份 `integrity_check=ok`，包含 2 份资料、2 份文字版本和 1 条笔记；备份目录为 `.local-data/backups/before-0.3.0-20260910`，不进入 Git。包含旧设置、任务快照及 v0.1.0 安装目录，原始媒体仍留在原位置。

安装版在 Vite 已停止时从 `http://tauri.localhost/` 加载内置页面，使用正式资料库完成 schema 3 升级。升级后数据库 `integrity_check=ok`，资料/版本/笔记数量不变。真实界面读回旧“课程目标：学习重心”笔记、158 段原文与核对报告，再次同步并通过按钮打开 Obsidian 中的对应课程索引。安装验证截图：`output/playwright/installed-integrity-v030.png`。

正式设置仅补充独立 vault 路径和稳定目录的 yt-dlp 可执行文件，保留已有识别与 API 配置。发布程序无需开发终端启动；本轮未在第二台干净电脑重测依赖安装。

## 实际边界

- 规则核对只能发现疑点和处理证据异常，不能证明逐字无遗漏。没有人工参考全文，未测 CER、漏词率或术语准确率；不把静音当作确定漏转。
- 目前是工作台到 vault 的单向快照发布；Obsidian 的个人编辑不反向覆盖 SQLite。不要直接编辑生成快照来进行双向校对。
- 采用媒体时长和已有任务记录，不额外运行第二套 ASR 或 VAD，不为读报告下载音轨。
- 本次未验证第二台 Windows、其他 GPU 或真实云端知识 API 的质量。既有原生文件选择/跨窗口拖入限制仍见 v0.2.0 验证记录。

设计依据：[Obsidian URI](https://obsidian.md/help/uri)、[内部链接和块引用](https://obsidian.md/help/links)、[官方 CLI](https://obsidian.md/help/cli)。
