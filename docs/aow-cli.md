# AoW CLI 使用

`aow-cli` 随 AoW 一起安装，用于查询项目、启动 Agent，以及创建自动化任务、查看执行记录。安装方式见 [README](../README.md)，请将 `~/.local/bin` 加入 `PATH`。

在运行 AoW 的机器上，以同一系统用户执行。`project` 需要 Web 服务运行；`agent` 还需要 host-daemon，并提前安装、登录对应 Agent CLI；`automation` 查询可在 Web 服务停止时使用。

## 查询项目

```bash
aow-cli project list
aow-cli project get PROJECT-ID
```

从结果中获取项目的 `id`、`name` 和 `repo_path`。下文的 `PROJECT-ID` 替换为项目 `id`，路径替换为该项目已有的仓库或 worktree 根目录。

## Orchestrator、角色 mention 与任务协作

在任务视图中，点击项目旁的“打开项目 Orchestrator”，首次选择 ACP Agent 或 ACP Role；之后会复用同一任务和会话。Orchestrator 负责接收、澄清和分发需求，开发、设计、排查请求即使没有 mention 也应交给执行任务。每个 Task 和 Orchestrator 空间都在创建时绑定唯一主会话，主会话持续接收用户对话、工作和后续消息；辅助会话用于人工开发、调查和讨论，默认不参与自动协作。

项目入口使用 `POST /api/projects/{id}/orchestrator`，任务数据通过 `is_orchestrator` 标识。升级会一次性迁移字段和默认名称，保留已有任务及会话 ID、任务关系和 Inbox 引用。旧 `/router` 接口和 `is_router` 字段不提供兼容别名；CLI 的 `task`、`role` 命令保持原样。

在 ACP 输入框输入 `@` 可选择角色，角色以头像标签显示。消息使用 `[@名称](mention://role/ROLE_ID)` 保存稳定引用；纯文本 `@名称` 和代码块中的示例不会生成角色引用。主会话输入由后端校验角色 ID，并提供各自职责、CLI 用法、当前项目、任务、父任务及本轮引用的角色 ID；角色详情按需查询。当前通过带元数据的 ACP 用户消息前缀注入规则，界面不把该前缀作为用户正文展示；它不是协议级 system message。只有 Orchestrator 主会话获得项目编排指令，普通任务主会话获得持续协作和显式报告指引。辅助会话、原生子会话和 fork 不继承主会话身份，也不追加协作指令或隐藏派发前缀。角色 Home、身份和工作区仍正常使用，任务之外的工作区角色会话保留自身的角色指令。已有对话历史中的旧提示词不会被清除。

创建任务和续办任务都应忠实传递用户原意，只移除纯路由措辞，保留明确的要求、约束、交付要求和关键引用。不要自行增加分析维度、实现步骤、验收标准、受众、报告格式或限制。拆分任务时只提取相关部分，不扩大范围；复杂请求也不能被压缩成遗漏细节的摘要。新任务不继承完整会话，确需补充定位信息时才追加最小的 `Context` 段，区分已知事实与假设，尤其避免换工作区后误解“当前分支”。角色、来源、工作区和执行策略由平台提供，不需要重复写入任务描述，也不默认添加“约束”“预期交付”等章节。这些是模型委派时的提示词规则，API 不会自动改写或截断传入的任务文本。

例如，`@Developer 你看下当前分支提交的内容做了什么` 的任务正文应保留为“你看下当前分支提交的内容做了什么”，通过角色和 `analyze` 参数表达分配与工作区策略；需要区分分支时再补充相应引用，不扩写成审查清单或报告模板。

```bash
aow-cli role list
aow-cli role get ROLE-ID
aow-cli task list --source-thread THREAD-ID

# 每项新工作使用一个稳定请求 ID；重试复用相同 ID 和内容
aow-cli task create --source-thread THREAD-ID --request-id login-implementation \
  --role DEVELOPER-ROLE-ID --intent change --task-file requirement.md

# 审查是独立子任务，使用目标任务的已提交工作区
aow-cli task create --source-thread THREAD-ID --request-id login-review \
  --role REVIEWER-ROLE-ID --intent review --target-task login-implementation \
  --task '审查登录功能'

# 在已有任务的主会话继续沟通；--thread 可指定该任务内另一角色会话
aow-cli task send login-implementation --source-thread THREAD-ID \
  --request-id login-feedback-1 --task-file feedback.md

# 执行会话主动报告阶段结论，不会自动结束任务
aow-cli task report login-implementation --source-thread DEVELOPER-THREAD-ID \
  --request-id login-result-1 --kind result --task-file conclusion.md

aow-cli task get login-implementation --source-thread THREAD-ID
aow-cli task status login-implementation done --source-thread THREAD-ID
```

`THREAD-ID` 是 AoW 本地会话 ID，不是模型供应商的原生 session ID。主会话从注入的上下文获取自身标识；辅助会话的标识可通过 CLI 或会话信息查询。这些命令使用运行中的 AoW 本机 socket。上下文同时提供 CLI 可执行文件和 state directory，角色 Home 改变时仍应使用该实例的 `--state-dir`。

Task 表示持续推进的工作目标，拥有一个主会话和多个角色会话。调研、POC、设计、开发可以在会话内多轮沟通；独立目标通过子任务推进。ACP 自己维护对话、轮次与结果，任务层不再保存 Run 或执行历史。任务页的新建入口提供 ACP 角色，可以增加辅助会话，创建或切换辅助会话不会替换主会话。主会话通过 `initial_thread_id` 固定绑定，自动流程按 Task 定位该会话。项目左侧显示 Orchestrator 和任务父子关系；右侧 Properties 维护任务信息和状态，Conversation 分组显示当前任务的会话关系。

主会话 tab 始终位于所属空间最左侧，不能关闭或移出；刷新后恢复原会话。完成、取消、阻塞和空闲均不解除保护。归档只保留历史并允许后续清理；删除任务组织后也可清理会话。停止 agent、释放连接或离开空间不会删除会话记录。归档后若已清理主会话，不能恢复该任务或自动生成替代主会话。

`change` 从项目默认基线创建独立 worktree；`analyze` 在项目工作区进行无修改的分析；`review` 要求目标任务工作区干净，记录其当前提交，按该提交审查。共享工作区并非文件系统只读沙箱；协作指令不提供文件系统级隔离。已有任务继续使用 `send`，保留工作区和会话上下文。`create` 省略 `--role` 时使用发起会话的 ACP adapter，不继承其角色身份。复杂开发与审查循环、远程角色不在当前范围内。

`create` / `send` 将消息持久化后立即返回。发起会话简要告知用户后结束当前轮次，不轮询、不等待。接收会话忙碌时，消息按会话顺序排队；服务恢复后继续投递。系统只记录消息是否已被 ACP 接收，不重复保存其执行结果。相同请求重试必须复用请求 ID 和内容。

Inbox 由会话主动调用 `task report` 产生，支持 `result`、`progress`、`blocked`、`question`。默认投递到父任务的主会话，即使任务最初由辅助会话创建，也不会把该辅助会话变成自动回报入口。需要人工指定时，可用 `--target-thread` 指定同一项目内的会话；直接创建且没有来源的任务会生成面向用户的 Inbox 通知。父任务已删除时，新报告仍保存在项目 Inbox，不再尝试唤醒其他会话；已有报告重试保留原来的目标和事件 ID，不会重复生成通知。主会话在交付结论、遇到阻塞或需要父任务决策时显式报告，不要求每轮反馈。普通轮次结束、状态变更、处理一条 Inbox 都不会自动生成报告。

报告与待投递消息在同一事务内保存。接收会话空闲后，以 `[WAKEUP]` 触发新一轮，携带事件 ID、报告任务及会话、来源请求和报告正文。Orchestrator 或父任务据此识别工作并处理新消息；需要向上汇总时可再次显式报告。消息来源通过持久化投递记录与 ACP 接收凭据关联，主会话收到独立的来源上下文，用户任务正文保持不变。重复投递通过 ACP 消息接收凭据去重，已经接收但中断的轮次不会自动重跑。Inbox 的已读、归档不影响投递；接收会话无法恢复时保留报告和错误，支持重试。

Task 状态独立维护：`backlog`、`todo`、`in_progress`、`in_review`、`done`、`cancelled`、`blocked`。首次分配进入进行中，轮次结束或发送报告不会自动完成 Task。通过 `task status` 或 Properties 更新状态；已完成或取消的 Task 要先显式重新打开才能发送新工作。取消阻止排队工作并中断任务会话，不删除会话、工作区或 Inbox。归档与业务状态互不替代。Properties 按 Work、Relationships、Workspace、About 展示状态、项目、父子任务、分支、目录和时间。`created_at`、`updated_at`、`completed_at` 为毫秒时间戳；标记 Done 记录完成时间，重新打开清空完成时间。未记录过的历史时间为空；旧 Waiting 状态一次性迁移为 Blocked。

升级会保留任务、ACP 会话、已有 Inbox、待投递消息和旧角色会话的工作区策略，移除旧执行记录表。独立的自动化任务及其执行记录不受此调整影响。

## 启动 Agent 与提交任务

```bash
# 启动 Agent，并提交首个任务
aow-cli agent create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo \
  --agent-id work-codex --task '检查项目并修复测试失败'

# 查看已有 Agent 终端
aow-cli agent list
aow-cli agent get --pane-id PANE-ID

# 向同一个终端提交后续任务
aow-cli agent submit --pane-id PANE-ID --task '继续处理剩余问题'
```

`PANE-ID` 使用创建结果中的 `pane_id`。使用 `--agent-id <配置ID>` 选择网页 **Settings → Runtime → Agent CLI** 中的配置，省略时使用 ID 为 `codex` 的配置。

配置 ID 必填、区分大小写且唯一，限 1–80 个字母、数字、`-` 或 `_`；新建和复制配置时需自行填写，创建后不可修改。显示名称及其他配置仍可编辑；如需调整 ID，请删除旧配置后重新创建，并在使用该配置的地方重新选择新 ID。已有终端和自动化的配置引用不会自动替换。CLI 不再接受 `--agent`。

交互式启动支持 Codex、TraeCode CLI、Hermes 和 Pi 类型的配置。项目须已注册，`--cwd` 须是该项目已存在的仓库或 worktree 根目录；需要新 worktree 时请先用 Git 创建。

通过 `agent create` 启动 Codex（包括自定义 Codex 配置）时，AoW 会为本次启动追加 `-c check_for_update_on_startup=false`，关闭启动更新检查，避免升级选择阻塞输入就绪和任务提交。Agent 注册配置和 Codex 全局配置不变，网页手动创建的普通 Agent 终端仍沿用原配置。Agent 版本更新可在任务执行之外单独进行。

任务也可以从文件或标准输入读取：

```bash
aow-cli agent create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo --task-file task.md
aow-cli agent submit --pane-id PANE-ID --task-file - < task.md
```

`--task` 与 `--task-file` 二选一，任务文本上限为 128 KiB。`create` 可省略任务，仅启动 Agent；`submit` 必须提供任务。

`create` 等待输入就绪后返回，默认超时 120 秒，可用 `--timeout 300` 调整（范围 1–600 秒）。命令成功表示终端就绪或任务已提交，**不表示 Agent 已完成任务**。

在网页对应项目的 **Terminal → CLI Terminals** 中打开终端查看进度，初始化完成后可点击“接管”。人工接管期间 `submit` 会返回冲突，收起终端释放控制权后可继续提交。启动失败或通信超时时，先查看终端和 `agent get` 的状态，避免重复提交；任务结束后终端继续保留，可在页面中销毁。

## 创建 ACP 实例与追溯父子关系

`agent` 管理交互式 Terminal，`acp` 管理 ACP 会话。ACP 命令通过同一个本机 CLI socket 访问 AoW 服务，无需浏览器登录，执行需要 host-daemon；`--agent-id` 使用 **Settings → Runtime → ACP** 的配置 ID，与 **Settings → Runtime → Agent CLI** 的配置独立。

```bash
# 查询可用的 ACP 配置，并创建会话、提交首个任务
aow-cli acp agents
aow-cli acp create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo \
  --agent-id codex-acp --task '分析这个项目'

# thread_id 是 AoW 的本地会话身份，与 adapter 返回的 session_id 不同
aow-cli acp list
aow-cli acp get --thread-id THREAD-ID
aow-cli acp submit --thread-id THREAD-ID --task-file follow-up.md

# Terminal 创建 ACP，ACP 再创建 Terminal
aow-cli acp create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo \
  --agent-id codex-acp --parent terminal:PANE-ID --task '检查设计'
aow-cli agent create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo \
  --agent-id codex --parent acp:THREAD-ID --task '实现已确认的设计'

# 同类型父子关系也使用相同参数
aow-cli acp create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo \
  --agent-id codex-acp --parent acp:PARENT-THREAD-ID
aow-cli agent create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo \
  --parent terminal:PARENT-PANE-ID

# 查询跨类型创建关系：祖先按从近到远排列，children 是直接子实例
aow-cli instance get acp:THREAD-ID
aow-cli instance get terminal:PANE-ID
aow-cli instance list
```

两种 `create` 都必须提供调用者当前的原生 session ID：显式 `--session-id` 优先，否则读取命令执行环境中的 `AOW_SESSION_ID`。这不是新建实例的 session ID，也不是 AoW 的 `thread_id` 或 Terminal pane ID。session ID 作为独立的 `caller` 请求字段传递，不会拼入任务文本。CLI 创建接口缺少该上下文时会拒绝请求；网页等非 Agent 发起的创建不需要虚构调用者 session。

父关系的来源按以下规则处理：

- `--no-parent` 明确创建根实例，忽略继承身份，但仍须提供调用者 session ID。
- 显式 `--parent terminal:ID|acp:ID` 覆盖自动父关系。
- Terminal 内部沿用 `AOW_INSTANCE=terminal:PANE-ID`，不通过 session ID 映射父实例。
- ACP 内部使用 `AOW_ACP_CONNECTION_ID` 与当前 session ID，由服务端在该连接的活跃会话绑定中找到 AoW 实例。可用 `--acp-connection-id` 显式提供连接 ID。不同连接可以有相同的原生 session ID，不会混淆。
- 未提供父身份、连接身份或 `--no-parent` 时拒绝创建，不静默创建根实例。会话已关闭、连接已断开、session 不属于该连接时也会拒绝自动关联。

ACP adapter 进程获得当前服务的 `AOW_STATE_DIR` 和自己的 `AOW_ACP_CONNECTION_ID`；共享进程不设置会话级 `AOW_SESSION_ID`，并清除继承的 `AOW_INSTANCE` 和旧 session 身份。adapter 的命令执行层必须在每次启动 CLI 时，通过 `--session-id` 或仅针对该次命令的 `AOW_SESSION_ID` 传入原生 session ID。AoW 不会向 prompt、steering、system prompt 或用户消息中注入身份说明。第三方后端如果不提供会话级命令上下文，需要在其执行层适配；AoW 不会猜测当前会话，也不会为此退回提示词注入。已有运行的 adapter 需重新连接以获得连接环境。

例如，adapter 已提供连接环境时，在会话的命令执行层调用：

```bash
aow-cli acp create --session-id CURRENT-ACP-SESSION-ID \
  --project-id PROJECT-ID --cwd /repo --agent-id codex-acp
aow-cli agent create --session-id CURRENT-ACP-SESSION-ID \
  --project-id PROJECT-ID --cwd /repo --agent-id codex
```

AoW 服务端在新建、分屏、重建或恢复 Terminal 时，通过 host-daemon 的通用 `environment` 和 `environment_remove` 启动参数设置自己的 `AOW_INSTANCE`，并清除继承的 `AOW_SESSION_ID` 和 `AOW_ACP_CONNECTION_ID`，避免子 Agent 把调用者的会话身份继续当成自身身份。macOS 下的 `AOW_LOG_MODE` 清理也由服务端指定。host-daemon 只执行环境变量的设置与清理，不理解这些业务身份；它继续管理 cwd、PTY、TERM、COLORTERM 等终端运行上下文。`environment_remove` 默认是空列表，清理优先于传入的同名设置；这些启动参数参与幂等创建比较，但不会在 HTTP 响应中返回。父关系仍统一保存为 `parent: {kind, id}`，创建时父实例必须存在。移除注入逻辑不会自动改写第三方后端已保存的历史消息；新的消息保持用户原文。

ACP 的 `--cwd` 同样必须是指定项目已有的仓库或 worktree 根目录。任务支持 `--task`、`--task-file` 或 `--task-file -`，上限 128 KiB；创建可以不提交任务。`--timeout` 默认 120 秒，范围 1–600 秒。成功表示会话就绪或任务已接受，**不表示任务完成**。`submit` 会恢复已断开的会话；正在处理任务或被其他客户端控制时返回冲突。需要登录或权限确认时，可在网页打开会话处理。

创建失败或超时会返回非零退出码和已经分配的 `thread_id`。超时后的初始化可能仍会继续，应先通过 `acp get` 检查状态；提交超时时也不要盲目重试。CLI 只在启动和提交期间占有控制权，网页可以接管；接管会阻止尚未提交的初始任务，不会取消已经开始的任务。

创建关系在重启和 ACP 恢复后保留，Terminal 和 ACP 列表都可以展开混合子实例。`instance list/get` 会包含保留的创建记录，`deleted: true` 表示实例已移除；删除中间实例不会删除后代的创建关系。追溯只保留身份、parent、创建时的标题及工作目录等元数据，不保留已删除会话的正文。祖先必须有实际的实例或留存记录，不为缺失数据生成占位祖先。

## 查看自动化任务与执行记录

```bash
# 查询任务
aow-cli automation list
aow-cli automation list --project-id PROJECT-ID
aow-cli automation get TASK-ID

# 查询执行历史与单次执行详情
aow-cli automation runs list TASK-ID --limit 20
aow-cli automation runs get TASK-ID RUN-ID

# 翻到下一页：直接使用上一页返回的 next_cursor
aow-cli automation runs list TASK-ID --limit 20 --before NEXT-CURSOR
```

`TASK-ID` 和 `RUN-ID` 从列表结果获取。任务列表默认隐藏已删除任务，添加 `--include-deleted` 可一并查看。执行列表按记录的 `started_at` 倒序排列，`--limit` 默认 50、范围 1–500；`next_cursor` 为 `null` 表示没有下一页。游标与资源 ID 独立，调用方原样传回即可，游标对应的记录被清理后仍可继续翻页。

CLI 支持从 JSON 文件创建自动化任务；修改、启用和执行请使用网页。执行详情中的 `stdout_path`、`stderr_path` 是本机日志路径，可自行打开查看。

## 从文件创建或迁移自动化任务

在目标机器上运行 AoW 服务，以同一系统用户执行；服务使用自定义数据目录时，传入相同的 `--state-dir`。创建通过本机私有 Unix socket 完成，无需浏览器登录，执行需要 host-daemon；对应 Agent 需要已在目标机器安装并配置。

```bash
# 查询目标项目 ID
aow-cli project list

# 从任务文件创建；也可以直接使用 automation get 输出的 JSON
aow-cli automation create --file task.json --project-id TARGET-PROJECT-ID

# 从标准输入读取，便于脚本处理
aow-cli automation create --file - --project-id TARGET-PROJECT-ID < task.json
```

`--file` 和 `--project-id` 均必填。文件须为一个 JSON 对象，上限 1 MiB；支持已有保存的任务文件、`automation get` 输出及只包含任务配置的 JSON。列表输出中的 `items` 数组不能作为单个任务直接导入。

每次成功调用都会创建一个新任务：

- 服务端重新生成任务 ID、时间和 revision，不复用原任务身份或执行历史。
- 项目归属和 `workspace_path` 使用目标项目及其仓库根目录，忽略文件中的原值；`workspace_mode` 保留，因此选择新 Worktree 或临时工作区的任务仍按原模式执行；`dynamic` 任务的 `workspace_path` 清空，由执行入口提供。
- 强制 `enabled: false`，定时任务以暂停状态保存，随后在网页确认配置并启用；手动任务保留手动类型，不产生定时计划。
- 保留名称、提示词、Agent 类型、运行计划、并发数、提示词变量绑定及通知等任务配置。Agent 启动路径、启动参数和配置环境由目标机器重新解析；忽略源文件中的 `launch`、删除标记及运行状态。

从零创建时可以使用下面的最小定时任务配置，无需填写项目、路径或启用状态：

```json
{
  "name": "Daily review",
  "prompt": "检查项目最近的变更",
  "agent": "codex",
  "workspace_mode": "existing",
  "cron": "0 9 * * 1-5",
  "max_concurrent_runs": 1
}
```

`agent` 表示 Agent 类型；可选的 `agent_id` 指定 **Settings → Runtime → Agent CLI** 中的配置 ID，配置类型须与 `agent` 一致。省略时使用与类型同名的配置。

其他配置沿用网页创建接口：`kind`（默认 `scheduled`）、`prompt_bindings`、`base_branch`、`interval_seconds`、`cleanup_worktree`、`yolo` 和 `failure_notification`。工作区方式支持 `new_worktree`（新建 Worktree）、`existing`（已有 Worktree）、`temporary`（动态工作区 → 临时工作区）和 `dynamic`（动态工作区 → 动态指定）。`new_worktree` 使用 `base_branch` 作为基准，新 Worktree 创建在主仓库的同级目录，任务结束后清理；`temporary` 创建空目录，Automation 在任务结束后删除。`interval_seconds` 有值时按间隔运行，否则使用 `cron`；cron 按目标机器本地时区解释。手动任务使用 `kind: "manual"`，不填写 cron 或间隔。`dynamic` 仅供手动任务使用，模板的 `workspace_path` 留空；Web 手动执行时必须填写工作区目录，Autopilot 绑定时自动提供当前 Agent 的实际工作目录。

批量迁移时逐文件调用，失败后继续处理其他文件。下面的脚本适用于 Bash，结束时输出成功／失败数量；有文件失败则返回非零状态：

```bash
project_id=TARGET-PROJECT-ID
succeeded=0
failed=0
for file in ./automation-files/*.json; do
  [ -f "$file" ] || continue
  if aow-cli automation create --file "$file" --project-id "$project_id"; then
    succeeded=$((succeeded + 1))
  else
    printf '导入失败：%s\n' "$file" >&2
    failed=$((failed + 1))
  fi
done
printf '成功：%s，失败：%s\n' "$succeeded" "$failed"
[ "$failed" -eq 0 ]
```

成功时输出新任务 JSON，失败时沿用 CLI 的错误 JSON 和非零退出码。重复调用会创建多个任务；若请求超时或响应丢失，先通过 `automation list --project-id TARGET-PROJECT-ID` 确认结果，再决定是否重试。

## 通用选项与帮助

```bash
# 使用与服务一致的自定义数据目录
aow-cli --state-dir /path/to/state project list

# 输出紧凑 JSON，便于脚本处理
aow-cli --compact automation list

# 查看命令和参数
aow-cli --help
aow-cli agent create --help
aow-cli automation runs list --help
```

数据目录默认是 `~/.aow`，也可由 `AOW_STATE_DIR` 指定；`--state-dir` 优先。服务使用自定义目录时，CLI 必须指定同一目录。`--state-dir` 和 `--compact` 均可放在子命令前后。

AoW 启动或重建终端 Agent 时，会在进程环境中设置指向当前服务实例的 `AOW_STATE_DIR`，覆盖 Agent 配置中的同名值；Agent 及其继承环境的子进程可以直接使用 `aow-cli`。这不会修改保存的 Agent 配置。已有运行中的 Agent 不会自动获得新环境，需要重新创建或重建后生效。

成功时向标准输出写入 JSON：列表通常为 `{"items":[...]}`，详情为单个对象。失败时以非零状态退出，并向标准错误输出 `{"error":{"code":"错误类型","message":"原因"}}`。查询到失败的自动化执行记录仍算查询成功，需查看记录中的状态判断任务结果。
