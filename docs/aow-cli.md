# AoW CLI 使用

`aow-cli` 随 AoW 一起安装，用于管理 Inbox 需求和评论、查询项目、启动 Agent，以及创建自动化任务、查看执行记录。安装方式见 [README](../README.md)，请将 `~/.local/bin` 加入 `PATH`。

在运行 AoW 的机器上，以同一系统用户执行。`project` 和 `inbox` 需要 Web 服务运行；`agent` 还需要 terminald，并提前安装、登录对应 Agent CLI；`automation` 查询可在 Web 服务停止时使用。

## Inbox 需求与评论

```bash
# 查询需求和评论
aow-cli inbox list
aow-cli inbox get REQUIREMENT-ID
aow-cli inbox comments list REQUIREMENT-ID

# 从 Markdown 文件创建需求；--file - 可以读取标准输入
aow-cli inbox create --file requirement.md --request-key capture-one

# 修正需求正文，版本从 inbox get 的 revision 字段读取
aow-cli inbox update REQUIREMENT-ID --expected-revision 1 --file corrected.md

# 只调整项目、标签，保留正文；重复 --label-id 可指定多个标签
aow-cli inbox update REQUIREMENT-ID --expected-revision 2 \
  --project-id PROJECT-ID --label-id todo --label-id review

# 解除项目绑定并清空标签
aow-cli inbox update REQUIREMENT-ID --expected-revision 3 --unbound --clear-labels

# 追加人工或 AI 评论
aow-cli inbox comments add REQUIREMENT-ID --author-type human --author 用户 \
  --content '补充一个验收条件' --request-key supplement-one
aow-cli inbox comments add REQUIREMENT-ID --author-type ai --author Codex \
  --file conclusion.md --request-key conclusion-one

# 删除时将需求及评论整体移入 deleted/，保留文件
aow-cli inbox delete REQUIREMENT-ID --expected-revision 4
```

创建需求和追加评论可用 `--content` 传入文本，或用 `--file` 读取 UTF-8 Markdown，二者不能同时使用；正文上限为 128 KiB。创建的需求默认不绑定项目、没有标签，随后可通过 `update` 调整。更新仅改变显式指定的字段；`--label-id` 替换整个标签集合，`--clear-labels` 清空标签。

更新和删除必须指定 `--expected-revision`，与最新需求版本不同会返回冲突（退出码 7），保留已有内容。评论追加不改变需求版本，只支持 `list` 和 `add`，没有编辑或删除命令。评论作者类型默认 `human`，`--author` 必填。

`--request-key` 用于创建需求和评论的重试去重。每个新操作使用新标识，同一操作重试时复用原标识和原始内容；省略时每次调用生成新的标识。请求失败时错误信息会包含本次使用的标识。不要用同一标识提交修改后的内容。

`inbox list` 返回网页使用的完整快照，包含 `items`、`labels`、`revision`、`executions` 和 `comment_counts`；`get` 返回单个需求；`comments list` 返回 `{"items":[...]}`，顺序与追加顺序一致。删除成功返回 `{"id":"需求 ID","deleted":true}`。

Inbox 执行时会在需求正文和追加 prompt 前注入严格 XML：

```xml
<aow-inbox>
  <requirement id="req-123" />
  <execution id="run-456" />
  <description>This task comes from AoW Inbox. You can use aow-cli inbox commands to retrieve requirements and comments, update requirements, and append comments.</description>
</aow-inbox>
```

XML 固定只有三个子节点，动态内容会转义；描述使用英文说明任务来源及 CLI 支持的操作。需求 Markdown 与追加 prompt 保持原文，放在 XML 块外。整个实际提交内容（包含 XML）仍限制为 128 KiB。Agent 继承指向发起实例的 `AOW_STATE_DIR`，无需在 XML 中重复实例配置。

## 查询项目

```bash
aow-cli project list
aow-cli project get PROJECT-ID
```

从结果中获取项目的 `id`、`name` 和 `repo_path`。下文的 `PROJECT-ID` 替换为项目 `id`，路径替换为该项目已有的仓库或 worktree 根目录。

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

`PANE-ID` 使用创建结果中的 `pane_id`。使用 `--agent-id <配置ID>` 选择网页 **Settings → Agents** 中的配置，省略时使用 ID 为 `codex` 的配置。

配置 ID 必填、区分大小写且唯一，限 1–80 个字母、数字、`-` 或 `_`；新建和复制配置时需自行填写，创建后不可修改。显示名称及其他配置仍可编辑；如需调整 ID，请删除旧配置后重新创建，并在使用该配置的地方重新选择新 ID。已有终端和自动化的配置引用不会自动替换。CLI 不再接受 `--agent`。

交互式启动支持 Codex、TraeCode CLI、Hermes 和 Pi 类型的配置。项目须已注册，`--cwd` 须是该项目已存在的仓库或 worktree 根目录；需要新 worktree 时请先用 Git 创建。

通过 `agent create` 启动 Codex（包括自定义 Codex 配置）时，AoW 会为本次启动追加 `-c check_for_update_on_startup=false`，关闭启动更新检查，避免升级选择阻塞输入就绪和任务提交。Inbox 执行共用此流程，也会生效；Agent 注册配置和 Codex 全局配置不变，网页手动创建的普通 Agent 终端仍沿用原配置。Agent 版本更新可在任务执行之外单独进行。

任务也可以从文件或标准输入读取：

```bash
aow-cli agent create --session-id CURRENT-SESSION-ID --project-id PROJECT-ID --cwd /repo --task-file task.md
aow-cli agent submit --pane-id PANE-ID --task-file - < task.md
```

`--task` 与 `--task-file` 二选一，任务文本上限为 128 KiB。`create` 可省略任务，仅启动 Agent；`submit` 必须提供任务。

`create` 等待输入就绪后返回，默认超时 120 秒，可用 `--timeout 300` 调整（范围 1–600 秒）。命令成功表示终端就绪或任务已提交，**不表示 Agent 已完成任务**。

在网页对应项目的 **Terminal → CLI Terminals** 中打开终端查看进度，初始化完成后可点击“接管”。人工接管期间 `submit` 会返回冲突，收起终端释放控制权后可继续提交。启动失败或通信超时时，先查看终端和 `agent get` 的状态，避免重复提交；任务结束后终端继续保留，可在页面中销毁。

## 创建 ACP 实例与追溯父子关系

`agent` 管理交互式 Terminal，`acp` 管理 ACP 会话。ACP 命令通过同一个本机 CLI socket 访问 AoW 服务，无需 terminald 或浏览器登录；`--agent-id` 使用 **Settings → ACP** 的配置 ID，与 **Settings → Agents** 的配置独立。

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

两种 `create` 都必须提供调用者当前的原生 session ID：显式 `--session-id` 优先，否则读取命令执行环境中的 `AOW_SESSION_ID`。这不是新建实例的 session ID，也不是 AoW 的 `thread_id` 或 Terminal pane ID。session ID 作为独立的 `caller` 请求字段传递，不会拼入任务文本。CLI 创建接口缺少该上下文时会拒绝请求；网页和 Inbox 等非 Agent 发起的创建不需要虚构调用者 session。

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

AoW 服务端在新建、分屏、重建或恢复 Terminal 时，通过 terminald 的通用 `environment` 和 `environment_remove` 启动参数设置自己的 `AOW_INSTANCE`，并清除继承的 `AOW_SESSION_ID` 和 `AOW_ACP_CONNECTION_ID`，避免子 Agent 把调用者的会话身份继续当成自身身份。macOS 下的 `AOW_LOG_MODE` 清理也由服务端指定。terminald 只执行环境变量的设置与清理，不理解这些业务身份；它继续管理 cwd、PTY、TERM、COLORTERM 等终端运行上下文。`environment_remove` 默认是空列表，清理优先于传入的同名设置；这些启动参数参与幂等创建比较，但不会在 HTTP 响应中返回。父关系仍统一保存为 `parent: {kind, id}`，创建时父实例必须存在。移除注入逻辑不会自动改写第三方后端已保存的历史消息；新的消息保持用户原文。

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

在目标机器上运行 AoW 服务，以同一系统用户执行；服务使用自定义数据目录时，传入相同的 `--state-dir`。创建通过本机私有 Unix socket 完成，无需浏览器登录或 terminald；对应 Agent 需要已在目标机器安装并配置。

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

`agent` 表示 Agent 类型；可选的 `agent_id` 指定 **Settings → Agents** 中的配置 ID，配置类型须与 `agent` 一致。省略时使用与类型同名的配置。

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
