# 前端代码组织

业务代码按功能聚合。一个功能的面板、详情、菜单、移动端页面、hooks、接口、类型和专用样式放在同一个 `features/<功能>/` 目录。

| 目录 | 职责 |
| --- | --- |
| `features/automations/` | 定时与手动任务、编辑器、运行参数和历史 |
| `features/terminals/` | 终端面板、分屏、连接、输入和终端状态 |
| `features/sessions/` | Agent 会话列表、消息、执行过程、快照和分享 |
| `features/pr/` | PR 列表、详情、Diff 和 Review Provider 配置 |
| `features/files/` | 文件树、系统文件浏览器、上传、收藏和文件接口 |
| `features/git/` | Source Control、提交记录、Git 操作和轮询 |
| `features/editor/` | Monaco、文档预览、编辑设置和共享文档状态 |
| `features/agents/` | Agent 定义、配置接口和图标 |
| `features/notifications/` | 任务通知、未读状态和通知设置 |
| `features/operations/` | 后台操作状态和日志 |
| `aow/` | 桌面工作区组装、项目与 Worktree、Tab 路由和浮动窗口 |
| `mobile/` | 移动端工作区外壳、导航、通用页面组件和路由状态 |
| `components/` | 不依赖业务功能的通用 UI：面板、列表行、按钮、确认框、Markdown |
| `lib/` | 不依赖 UI 和业务功能的基础能力：部署路径、存储命名空间、HTTP 请求 |

## 归属与依赖

- `AutomationPanel`、`TerminalPanel` 等属于各自功能；共同使用 `components/AowPanel`、`AowListRow` 和 `AowIconButton`。
- 同一功能被桌面和移动端复用时，仍归该功能所有。例如运行参数组件属于 `automations`，移动端终端页面属于 `terminals`。
- 功能专属类型在该功能的 `types.ts`；项目与 Worktree 等工作区契约在 `aow/types.ts`。不要重新建立汇总全部业务类型的根目录文件。
- 功能接口归各功能的 API 模块。`aow/aowApi.ts` 只负责项目、Worktree 和工作区设置。请求超时、重试与错误处理由原有请求实现保留，避免目录迁移改变服务行为。
- 跨功能复用应导入明确的模块和类型。工作区负责组合功能；`components/` 和 `lib/` 不反向依赖 `features/`、`aow/` 或 `mobile/`。
- 公共列表行通过 `ListRowMenuContext` 接收菜单能力；浮动打开的实现由工作区提供。
- 功能专用 CSS 与组件放在一起。`styles.css` 保留现有全局主题、布局和历史共享样式的级联顺序；新功能样式优先在所属目录维护。
- 使用具体模块路径导入，保留现有动态加载边界，避免汇总导出文件意外提前加载 Monaco 或移动端页面。
- Monaco 编辑区域按需挂载；隐藏的空编辑器仍会触发下载。表单可以动态导入 `features/editor/MonacoEditor`，让弹窗和普通字段先显示，代码编辑区域通过 `Suspense` 加载；不要在面板或工作区入口静态导入 Monaco 初始化模块。

## 验证

在 `frontend/` 下运行 `npm run typecheck`、`npm run build` 和受影响功能的测试。`npm run test:architecture` 检查本地导入是否有效、路径大小写是否冲突，以及公共组件和基础库的依赖边界。
