# 实现架构

```mermaid
flowchart LR
  UI[React 桌面界面] -->|类型化命令| Tauri[Tauri Runtime]
  Tauri --> DB[(SQLite WAL / FTS5)]
  Tauri --> Source[SourceProvider]
  Source --> Bili[B 站接口]
  Source --> Web[yt-dlp 网页 / 直链]
  Source --> Local[本地文件]
  Tauri --> Media[yt-dlp 媒体 / FFmpeg]
  Tauri -->|JSON Lines v1| ASR[Python Whisper worker]
  Tauri -->|仅选中的文字| API[KnowledgeProvider]
  Tauri --> Profile[ResourceProfiler]
  Tauri -->|任务事件| UI
```

## 模块边界

`crates/core` 不依赖 Tauri。负责不可变文字版本、事务和 FTS 索引、字幕解析/导出、资源候选策略、引用校验。桌面端 `service.rs` 组合 `SourceProvider`、`AsrEngine`、`KnowledgeProvider` 和 `ResourceProfiler`；替换引擎无需改变阅读界面。

桌面命令在阻塞线程池中工作；事件只是刷新提示，数据库是最终状态依据。React 不执行 shell、不直接写数据库，子进程通过参数数组启动。Windows JobObject 管理进程树，主进程退出后其子进程也终止。

`source.rs` 负责 B 站接口、本地文件与来源分流；`web_source.rs` 通过 yt-dlp 的单视频 JSON 构建 `webMedia` 预览，独立获取指定语言 SRT/VTT。元数据与字幕请求均使用 `--skip-download`；媒体路径独立调用 `bestaudio/best`，需要混合媒体时预览先提示。通用预览和字幕进程限时 120 秒并登记退出控制。前端通过 Tauri 文件对话框与 webview 拖入事件获取路径，同一时刻仅接受一个文件，并忽略已失效的预览响应。

## 存储与一致性

- `assets` 是课程/分 P；`transcripts`、`segments` 保存各版文字。校对保留片段 ID，新增版本，绝不改写旧版。
- `notes` 绑定 transcript ID，引用时间和文字必须与该版片段匹配。`stale` 由当前活动版本计算。
- `segment_fts` 使用 jieba 分词建立中文 FTS5 索引；只索引各资料的当前文字版本。
- `jobs` 记录状态/进度，配置快照保存在 `jobs/<id>.json`。批量入队通过一个事务提交，后项冲突不会留下部分可运行任务。
- `job_transcripts` 将任务 ID 绑定一次完成结果；文字、活动版本、索引与完成状态共同提交。相同任务再次提交返回原结果，不重复建立版本。
- SQLite schema v3 兼容 v1/v2 升级，新增按 transcript ID + 证据摘要绑定的 `integrity_reviews`；拒绝用旧应用打开更新的 schema。升级前备份，降级使用旧备份。
- 取消、终态提交、校对和激活版本共用 mutation gate。设置变更有代数，耗时探测后若设置已变，拒绝将旧配置写入新库。
- 打开或切换资料库时把遗留 `running/queued` 恢复为 `paused`，由使用者选择重试。
- 每个资料库持有独占 OS 文件锁；桌面应用和诊断进程不能同时恢复同一资料库，异常退出后 OS 自动释放锁。

## 识别协议与恢复

标准输出仅含 JSONL v1；日志走 stderr。输入是 FFmpeg 生成的单声道 16 kHz PCM16 WAV。默认每块 300 秒，块边界使用全局毫秒时间戳。检查点身份包含音频 SHA256、模型、设备、语言、提示词、线程、分块尺寸、引擎版本和模型 SHA256。

每块先原子写检查点，再发进度；未完成块重新处理，已完成块直接读取。换模型或配置产生新检查点空间。完成的任务结果进入 SQLite，而中间片段仍在检查点中。性能记录写入失败不会把已提交的文字任务标为失败。

## API 与文件边界

接口默认不配置模型。云端必须 HTTPS；明确的回环本机服务可 HTTP。密钥只存 Windows 凭据存储。网站 Cookie 使用 Netscape 格式：yt-dlp 按域处理，B 站 API 只接收适用于 `api.bilibili.com` 的未过期 Cookie；只有其他网站 Cookie 时 B 站仍可匿名访问。字幕 CDN 与 AI 请求均不携带 B 站 Cookie。AI 请求明确携带所查看的 transcript ID 和选中片段；不会默默扩大或截断范围。

拒绝未知/未声明的引用；输出 Markdown 不渲染原始 HTML 和远程图片。响应读取实际限制 8 MiB，包括 chunked 响应。HTTP 超时、限流、认证失败都返回操作级错误，不改写原文。

缓存清理只接受类别，检查类别根及子目录的 junction/reparse，保护数据库、原始文件和共享模型。发布包从安装目录解析 worker，调试版从源码目录解析。

## 完整性核对和本地知识桥接

`integrity.rs` 是纯规则模块：区间并集覆盖、异常时间轴、重复、来源时长差异与原始任务分块证据。`service.rs` 在发布 ASR 结果前检查块数，并在人工结论提交时重算证据摘要，防止旧报告结论套用新媒体或文字。摘要包括文字、来源时长、任务证据与音频路径；不对音频内容做语义一致性证明。新校对版本不会借用原始版本的完成记录。

`vault.rs` 在本地 vault 的 `CourseWorkbench` 目录生成内容摘要命名的不可覆盖 Markdown 快照；笔记只取所选文字版本，引用转换为稳定块链接。快照先完整落盘，再追加课程与全库索引，OS 文件锁串行化同步。重试可补齐中断的索引写入。个人笔记只创建一次，不进行双向同步。写入路径拒绝上级跳转、符号链接和 Windows reparse points。

Obsidian 通过标准 URI 打开已注册的 vault 文件。没有安装或注册时，用户仍可使用本地 Markdown。工作台不安装插件，不复制媒体，不向 Obsidian 云服务传输数据。

## 分类与主题（v0.4.0）

SQLite schema 4 增加 `collections` 与 `asset_organization`。前者为有父级的稳定 ID 树，最多 8 层；后者记录每份课程的唯一分类与独立收藏。缺少记录代表未分类、未收藏。所有分类操作经过 Runtime mutation gate；批量移动在同一事务中校验目标和全部课程。非空分类不可删除，Asset、转写、笔记、FTS 与 Obsidian 文件路径均保持原有身份。

Bootstrap 提供当前数据库的 organization 快照；LibraryView 计算含后代的范围，CollectionNavigation 管理分支菜单，OrganizeDialog 固定待收纳课程集合、显式确认移动。主题存入 AppSettings，CSS 语义色覆盖各页面；缓存只保存主题/布局标识。motion.css 集中管理短过渡并遵循 prefers-reduced-motion，动画不参与写入完成判定。

## 音频证据与逐项复核（v0.5.0）

schema 5 的 quality 模块保存逐项复核、不可变诊断/来源证据、语音证据
历史和局部识别候选。语音缓存绑定完整音频 SHA256、模型与 worker 文件摘要、
运行时及检测参数；变化后降回基础检查，旧结论不套用。

quality_service 编排独立 CPU VAD、音频区间提取和 Whisper 复核，共用现有
资源排他门及进程取消。资源档位解析和 GPU/Python 探测沿用同一取消控制器。
局部范围扩展到完整原片段且最多 120 秒，结果先持久化为候选。显式采纳在
事务中写新版本、FTS、父版本/模型/区间来源和相关旧疑点修订记录；手动
校对仅关联实际文本改动完整覆盖的识别/重复疑点。

前端理由草稿绑定 issue ID 和证据指纹。音频/时长后台刷新不换绑草稿，
过期时阻止提交并保留可复制/放弃的内容；后台新 active version 不切走
当前阅读版本。笔记和核对面板保持挂载，收起不清空未保存状态。
