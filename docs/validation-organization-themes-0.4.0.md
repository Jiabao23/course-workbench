# v0.4.0 分类、主题与动态交互验证

验证日期：2026-09-12。本机 Windows 10，真实 Tauri/WebView2。基线为 v0.3.0（acdd907）；本轮未重新运行语音识别质量或硬件性能基准。

## 自动化

共 117 项通过：Rust 97、Python 8、前端 12。另有一个进程测试 helper 按设计 ignored，由生命周期测试作为子进程执行，不计入 117 项。TypeScript/Vite 构建、cargo fmt、Clippy `-D warnings` 通过。

新增覆盖：schema 3→4 升级与旧内容保留；分类嵌套、名称、八层限制、重名与非空删除；无效目标/课程的整批回滚；收藏独立与重开持久化；资料目录切换隔离；老设置主题默认值、三主题保存、未知主题拒绝；前端后代范围、未分类、收藏、标题过滤及折叠树。

命令：

```powershell
. .\scripts\dev-env.ps1
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
npm --prefix apps/desktop test
D:\bili2text\.venv\Scripts\python.exe -m unittest discover -s workers/asr/tests -v
npm --prefix apps/desktop run package
```

## 真实桌面流程（隔离副本）

使用 SQLite backup API 复制原两门课程到 `.local-data/organization-theme-tests/library`，专用配置和验证标识；不把测试分类、笔记写入原资料库。

- 创建“计算机学习 / 计算机网络”，两门课批量归类；P1 收藏后移入“重点复习”，收藏仍保留，上级范围包含下级。
- 在“收纳到…”面板新建子分组并自动选中；新建名称尚未完成时不能确认移动，最终确认后课程位置更新。
- 原生桌面路径入口导入一个 VTT，得到两个片段并进入未分类；在收纳面板中新建“日常阅读”，确认后课程归入其中。
- 左侧省略号菜单重命名为“网络课程”，所有课程跟随稳定 ID；分支折叠/展开有效。非空删除提示拒绝；“可删除空分类”创建、确认删除并刷新后消失。
- 分类栏收起前课程内容宽度 929px，收起后 1083px；收藏筛选和一条可见课程保持不变。展开保留收藏，刷新后布局收起偏好仍在。主导航可独立缩为 68px。
- P1 原 158 段与“课程目标：学习重心”旧笔记可读；隔离库新增“主题与收纳流程验收”笔记保存成功。没有修改生产原文或旧引用。
- 三种主题分别实际保存、显示并截图；纸色预览后放弃设置恢复森林色，夜色保存写入 JSON。重开数据库后分类和收藏保留。
- 三主题当前资料库可见文字及夜色阅读器的对比度抽查，未发现小于 4.5:1 的可见文本；这是当前页面抽查，不是完整无障碍认证。修复阅读器处理中提示的浅色背景，实际计算值为白底、深色文字。

截图在 `output/playwright/`（本地验收资料，不进入 Git），包括 `library-forest-final.png`、`library-paper-final.png`、`library-night-final.png`、`organize-popup-night.png`、`reader-night.png`、`classification-collapsed.png`。

## 发布记录

原 schema 3 数据库、设置、任务和安装目录已备份到 `.local-data/backups/before-0.4.0-20260912`；备份时 assets=2、transcripts=2、notes=1，SQLite integrity_check=ok。

v0.4.0 NSIS 构建及静默安装均成功，`D:\CourseWorkbench\course-workbench.exe` 文件/产品版本均为 0.4.0。安装版从 `http://tauri.localhost/` 加载内置资源，不依赖 Vite。

安装版在隔离库完成：

- 实际读取布局动画时长 220ms；分类栏收起后焦点为“展开分类栏”，按 Enter 可展开，连续三轮操作仍保留收藏筛选与一条课程。
- 模拟系统 `prefers-reduced-motion: reduce` 后 transitionDuration=0s、活动布局动画=0，弹窗 animation=none；恢复普通偏好后菜单为 140ms 淡入，Escape 正常关闭。
- 设置 WebView 视口 1000×700：documentWidth=1000，课程内容 clientWidth/scrollWidth 均为 599；收纳弹窗宽 540，无横向溢出，截图已检查。这是最小视口模拟，不是另一显示器实测。
- 主侧栏/分类栏均收起后刷新，两个展开入口仍存在；森林主题和三条测试课程保留。

安装版随后以正式配置打开原资料库，schema 升为 4，integrity_check=ok。逐行比较 assets、transcripts、notes 与升级前备份完全一致（2/2/1），没有写入测试分类；界面显示两条未分类课程、默认森林主题，旧“课程目标：学习重心”笔记与五处出处可读。

安装包：`output/release/Course-Workbench-0.4.0-windows-x64-setup.exe`。
SHA256：`31d2c81945f4a34e0bc893339a81de08628f159432a69cfba917ccd374c50cac`。
安装版截图另见 `installed-minimum-forest.png`、`installed-minimum-organize.png`、`installed-collapsed-final.png`。

## 边界

分类是同一资料库内的逻辑管理，不移动音视频、文字版本或 Obsidian 快照；暂不支持文件夹重排/跨父级移动。收藏与归属存 SQLite，布局偏好存当前 WebView 配置；临时搜索/筛选不跨重启。Obsidian 仍为主动单向本地快照。

本轮未重测云端模型质量、CER、其他显卡和第二台干净 Windows。此前原生文件选择/跨窗口拖入的自动化限制仍然存在。

代码提交 `6276f85` 已推送 `feat/course-workbench-v1` 并核对远端 SHA；GitHub 仓库现转到 `Jiabao23/course-workbench`。交付时 Windows checks #5 显示 In progress，不能记为已通过。
