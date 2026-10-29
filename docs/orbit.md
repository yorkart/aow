# AoW Orbit

Orbit 是同仓库内独立构建和运行的实验应用。`frontend-orbit` 使用 React / TypeScript，`server-orbit` 使用 Node.js / TypeScript 和内置 SQLite。需要 Node.js 22.16+（22.x）或 24+、npm 和 Bash。

Orbit 包含独立登录、组织（org）的创建和切换、Skills 技能库、Connector 管理，以及工作窗口。Tasks、Projects、Automation、Agents 目前仅提供图标导航，尚未接入业务功能或 ACP / terminald。没有预置组织或演示数据。

## 启动

从仓库根目录运行：

```bash
./scripts/orbit/start.sh
```

缺少构建产物时，脚本先安装 Orbit 自己的依赖并构建。首次启动在终端提示配置用户名、密码并确认密码；已有账户会保留。然后访问 <http://127.0.0.1:8382>，登录并创建第一个组织。没有组织时始终显示引导；创建后进入工作窗口。

左侧 org 下拉菜单支持创建其他组织、切换组织和退出登录。组织、当前选择和登录会话保存在 SQLite，刷新或服务重启后仍可恢复；会话受有效期和空闲超时限制。

源码更新后重新构建，再启动：

```bash
./scripts/orbit/build.sh
./scripts/orbit/start.sh
```

修改登录账户：

```bash
./scripts/orbit/account.sh
```

修改账户会使已有登录会话失效，组织数据保留。自动化部署可通过 `ORBIT_USERNAME` 和 `ORBIT_PASSWORD` 环境变量提供账户信息；不要把密码作为命令行参数。启动脚本只在没有账户时使用这些变量。

## 数据与配置

默认数据文件是 `~/.orbit/orbit.sqlite`。数据库包含账户、登录会话和组织，密码使用与 AoW 一致的 PBKDF2-HMAC-SHA256（600,000 次迭代）加盐哈希。会话令牌通过 Orbit 专用的 HttpOnly / SameSite=Strict Cookie 传输，数据库仅保存令牌哈希。

首次打开数据库时创建必要的表；已有数据库直接复用。数据库初始化不导入旧版 AoW 数据，也未引入额外的版本迁移框架。

## Skills 技能库

进入 Skills，左侧面板为 Installed，工作区为 Installed skills。所有新增、导入和编辑操作都在工作区整页完成。

- **Create a skill**：填写名称、说明和指令，生成带 YAML frontmatter 的 SKILL.md。
- **Import skills from project**：选择当前组织已登记的本地项目，扫描并勾选技能。Projects 模块尚未开放时，可以直接在导入页登记项目名称和本地工作目录。路径属于运行 server-orbit 的机器。
- **详情和编辑**：默认打开 SKILL.md，支持 Markdown 编辑、预览和包内相对引用；文本文件可直接编辑，图片可预览，二进制文件可下载。文件树支持新建文件或目录、上传、重命名和删除，SKILL.md 入口不可删除或重命名。
- **保存**：所有文件改动统一生成一个新版本；name、description 从当前 SKILL.md 解析到索引。过期草稿会返回版本冲突，防止覆盖其他窗口的修改。组织内同名技能需要改名，不覆盖已有技能。
- **删除**：仅隐藏技能索引，保留历史版本。存在 Agent 引用时需先在 Agent 配置中解除引用。

索引、版本记录、项目目录登记及 Agent 引用关系保存在 `~/.orbit/orbit.sqlite`，完整技能包保存在 `~/.orbit/skills/<org-id>/<skill-id>/versions/<version-id>/`。文件写入成功后才切换当前版本。每个文件最多 2 MiB，每包最多 20 MiB、1000 个文件或目录；一次批量导入最多 20 MiB。扫描跳过依赖、构建和 Git 数据目录，最多扫描 10000 个目录、16 层和 100 个技能；达到限制会显示提示。

导入复制完整技能包，保留目录、相对引用、二进制文件及脚本的可执行属性。源项目变化不会修改导入副本；导入和保存均不执行脚本，不导入符号链接或特殊文件。

`server-orbit/src/skills/references.ts` 为后续 Agent 配置和执行器提供技能多选引用及运行快照接口。运行开始时固定版本，并在 `~/.orbit/skill-runs/<org-id>/<agent-id>/<run-id>/` 下准备独立的完整副本；已开始的运行不随技能更新而变化。当前 Agent 和运行器界面尚未开放，此接口不会改写用户 home 下的全局技能目录。

## Connectors

进入 Connectors，左侧只有“本地 Connector”，右侧提供连接列表、名称 / Provider / 账号搜索、新增、编辑、认证检测、启停和删除。新增和编辑使用工作区整页。每个组织独立保存连接，引用信息来自真实的 Agent 关联记录。

| Provider | 执行器 | 认证方式 |
| --- | --- | --- |
| OpenAI | Codex | 本地登录、独立设备码登录、API Key |
| Anthropic | Claude Code | 本地登录、独立浏览器授权和授权码回填、API Key |
| DeepSeek | Claude Code | API Key |
| Google Gemini | Gemini CLI | 本地 OAuth 身份（安装执行器后显示） |

本期 DeepSeek 仅支持 Claude Code，不提供 DeepSeek / Codex 组合或协议适配。其他尚未接入适配器的 Provider 不显示为可选项。

### 本地身份

“本地”是执行器所在机器的用户环境。Codex 从 `CODEX_HOME` 或 `~/.codex` 读取认证；Claude Code 从 `CLAUDE_CONFIG_DIR` 或 `~/.claude` 读取认证文件，macOS 也支持对应账号的钥匙串；Codex 的系统凭据存储通过本机 security / secret-tool 读取。Gemini 支持本地 OAuth 凭据文件及必要的模型配置。

检测在临时隔离目录中检查执行器，Codex / Claude Code 使用原生登录状态命令。Gemini 检测 OAuth 凭据和执行器安装状态，不发送推理请求。

本地 Connector 只保存身份来源和检测摘要，不在数据库中保存本地凭据副本。每次启动执行进程前，运行接口重新读取最新身份并复制到该次运行的隔离环境，保留原始 home 不变。全局指令、Skills、MCP 配置不会随认证文件导入。隔离环境中的身份变化不回写源目录，Orbit 不承担身份刷新或同步，执行器沿用自己的认证机制。

### 独立登录和 API Key

- Codex 通过原生 `codex login --device-auth` 发起设备码流程；页面展示官方地址和设备码。
- Claude Code 通过原生 `claude setup-token` 的 PTY 流程展示官方授权地址，接收授权码，并保存执行器生成的独立 OAuth Token。需要本机安装 Claude Code 和 `script`（Linux 为 util-linux 工具，macOS 系统提供）。
- 授权进度可轮询、取消和过期，退出编辑页会取消未保存的授权。授权码和原始终端输出不写入日志。授权成功结果有有效期，保存后由 Connector 独立持有。
- API Key 通过对应 Provider 的模型列表接口验证，不发送推理请求。密钥输入留空可保留已保存的密钥；切换 Provider、执行器或认证方式会清除旧认证绑定。

“保存配置”可以保存尚未认证的连接；提交新的 API Key 时，密钥必须先通过验证。“保存并连接”同时检查执行器和认证，成功后状态为可用。可用、待认证、异常和已停用由后端维护。重新启用后需再次检查连接。被 Agent 引用的连接需解除引用后才能删除。

配置和加密后的认证存入 `~/.orbit/orbit.sqlite`。实例加密密钥为 `~/.orbit/credentials.key`（0600），备份数据库时应同时备份这个文件。检测临时目录位于 `~/.orbit/connector-work/`，独立授权临时目录位于 `~/.orbit/connector-auth/`，流程结束后清理。

`Connectors.prepareExecution` 提供后续 Agent 运行接入接口，在 `~/.orbit/connector-runs/<org-id>/<connector-id>/<execution-id>/home/` 下生成独立环境，并设置原生执行器配置目录。凭据文件权限为 0600；运行环境通过后端内部接口交付，不通过浏览器 API 返回。Connector 停用后禁止创建新执行环境，已准备的环境保持独立。

这些目录提供配置隔离；进程的操作系统文件访问权限由后续运行器和沙箱负责。当前尚未接入 Agent 任务执行。

## 服务配置

| 环境变量 | 默认值 | 用途 |
| --- | --- | --- |
| `ORBIT_HOST` | `127.0.0.1` | 服务监听地址 |
| `ORBIT_PORT` | `8382` | 服务端口 |
| `ORBIT_STATE_DIR` | `~/.orbit` | 独立数据目录 |
| `ORBIT_FRONTEND_DIR` | `frontend-orbit/dist` | 静态前端构建目录，默认相对于项目位置 |
| `ORBIT_FRONTEND_ORIGIN` | 未设置 | 独立前端部署时允许的精确来源，例如 `https://orbit.example.com` |
| `ORBIT_SECURE_COOKIES` | 未设置 | HTTPS 部署时设为 `1` |
| `ORBIT_SESSION_MAX_AGE_SECONDS` | `43200` | 登录会话最长有效期 |
| `ORBIT_SESSION_IDLE_TIMEOUT_SECONDS` | `1800` | 登录会话空闲超时 |

## 独立前后端开发与部署

两个目录分别维护 package.json、锁文件和构建命令；没有跨前端目录导入。登录界面和主题色板从现有 AoW 复制到 Orbit 后独立维护，主题偏好键为 `orbit.theme`。色板来源见 [AoW 主题说明](themes.md)。

后端可以单独构建和启动：

```bash
npm --prefix server-orbit ci --ignore-scripts --no-audit --no-fund
npm --prefix server-orbit run build
npm --prefix server-orbit run account
npm --prefix server-orbit start
```

另一个终端启动前端开发服务：

```bash
npm --prefix frontend-orbit ci --ignore-scripts --no-audit --no-fund
npm --prefix frontend-orbit run dev
```

访问 <http://127.0.0.1:5174>。Vite 默认将 `/api` 代理到 `http://127.0.0.1:8382`；可用 `ORBIT_API_TARGET` 指定其他后端地址。

生产环境可由启动脚本中的 Node 服务同时提供静态前端，也可以将 `frontend-orbit/dist` 单独部署，通过同源网关转发 `/api`。若前端与 API 使用不同来源，在前端构建时设置 `VITE_ORBIT_API_URL` 为后端来源，同时为后端设置 `ORBIT_FRONTEND_ORIGIN`。使用同一站点下的域名或相同主机的不同端口，以符合 SameSite Cookie 策略。

## 验证

```bash
npm --prefix server-orbit run lint
npm --prefix server-orbit test
npm --prefix frontend-orbit run lint
npm --prefix frontend-orbit run build
npm --prefix frontend-orbit test
```

浏览器测试依赖 Playwright Chromium。可设置 `ORBIT_TEST_BROWSER` 为系统 Chromium 的绝对路径，或运行 `npm --prefix frontend-orbit exec -- playwright install chromium` 安装测试浏览器。测试使用临时 SQLite 数据库及独立服务端口。
