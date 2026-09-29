# AoW CLI 使用

`aow-cli` 随 AoW 一起安装，用于查询项目、启动 Agent、管理 Inbox 与任务状态，以及查看自动化执行记录。安装方式见 [README](../README.md)，请将 `~/.local/bin` 加入 `PATH`。

在运行 AoW 的机器上，以同一系统用户执行。`project` 需要 Web 服务运行；`agent` 还需要 terminald，并提前安装、登录对应 Agent CLI；`automation` 查询可在 Web 服务停止时使用。

## 查询项目

```bash
aow-cli project list
aow-cli project get PROJECT-ID
```

从结果中获取项目的 `id`、`name` 和 `repo_path`。下文的 `PROJECT-ID` 替换为项目 `id`，路径替换为该项目已有的仓库或 worktree 根目录。

## 启动 Agent 与提交任务

```bash
# 启动 Agent，并提交首个任务
aow-cli agent create --project-id PROJECT-ID --cwd /repo \
  --task '检查项目并修复测试失败'

# 查看已有 Agent 终端
aow-cli agent list
aow-cli agent get --pane-id PANE-ID

# 向同一个终端提交后续任务
aow-cli agent submit --pane-id PANE-ID --task '继续处理剩余问题'
```

`PANE-ID` 使用创建结果中的 `pane_id`。当前支持 `--agent codex`（默认）和 `--agent traecli`，启动配置沿用网页 **Settings → Agents**。项目须已注册，`--cwd` 须是该项目已存在的仓库或 worktree 根目录；需要新 worktree 时请先用 Git 创建。

任务也可以从文件或标准输入读取：

```bash
aow-cli agent create --project-id PROJECT-ID --cwd /repo --task-file task.md
aow-cli agent submit --pane-id PANE-ID --task-file - < task.md
```

`--task` 与 `--task-file` 二选一，任务文本上限为 128 KiB。`create` 可省略任务，仅启动 Agent；`submit` 必须提供任务。

`create` 等待输入就绪后返回，默认超时 120 秒，可用 `--timeout 300` 调整（范围 1–600 秒）。命令成功表示终端就绪或任务已提交，**不表示 Agent 已完成任务**。

在网页对应项目的 **Terminal → CLI Terminals** 中打开终端查看进度，初始化完成后可点击“接管”。人工接管期间 `submit` 会返回冲突，收起终端释放控制权后可继续提交。启动失败或通信超时时，先查看终端和 `agent get` 的状态，避免重复提交；任务结束后终端继续保留，可在页面中销毁。

## 查看自动化任务与执行记录

```bash
# 查询任务
aow-cli automation list
aow-cli automation list --project-id PROJECT-ID
aow-cli automation get TASK-ID

# 查询执行历史与单次执行详情
aow-cli automation runs list TASK-ID --limit 20
aow-cli automation runs get TASK-ID RUN-ID

# 翻到下一页：RUN-ID 使用上一页返回的 next_cursor
aow-cli automation runs list TASK-ID --limit 20 --before RUN-ID
```

`TASK-ID` 和 `RUN-ID` 从列表结果获取。任务列表默认隐藏已删除任务，添加 `--include-deleted` 可一并查看。执行列表的 `--limit` 默认 50、范围 1–500；`next_cursor` 为 `null` 表示没有下一页。

CLI 当前只查询自动化任务，创建、修改和执行请使用网页。执行详情中的 `stdout_path`、`stderr_path` 是本机日志路径，可自行打开查看。

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

数据目录默认是 `~/.local/state/aow`，也可由 `AOW_STATE_DIR` 或 `XDG_STATE_HOME` 指定；`--state-dir` 优先。服务使用自定义目录时，CLI 必须指定同一目录。`--state-dir` 和 `--compact` 均可放在子命令前后。

成功时向标准输出写入 JSON：列表通常为 `{"items":[...]}`，详情为单个对象。失败时以非零状态退出，并向标准错误输出 `{"error":{"code":"错误类型","message":"原因"}}`。查询到失败的自动化执行记录仍算查询成功，需查看记录中的状态判断任务结果。

## Inbox 与任务看板

右侧 **Tasks** 导航显示当前项目的 **Inbox**，同时在工作区打开该项目的 **Task Board**。录入需求时自动归属当前项目，不要求选择 Agent 或工作目录；录入弹框支持「创建」和「连续创建」。右击需求选择「转为任务」，直接沿用需求所属项目，只需配置 Agent、该项目的已有工作目录或新 Worktree、初始状态。「立即执行」可选；未勾选时仍会创建交互 Agent，稍后通过卡片上的执行按钮或 CLI 提交初始任务。

所有任务共用一套状态，可通过看板的「配置任务状态」调整名称、颜色和排列顺序。默认的 Todo / In progress / In review / Done 只是初始模板。拖动卡片或更新状态不会启动、停止 Agent，也不代表用户验收。卡片始终链接原来的 Terminal Agent，可以随时打开继续对话。原需求保留在 Inbox 中，可通过标题栏「… → 显示已转化」查看；修改原需求不会改写已有任务的内容。

录入和编辑需求使用同一个 Markdown 编辑区：第一条非空白行自动作为标题，其余内容作为正文；标题行突出显示，回车继续写正文。连续创建后清空内容并保持编辑焦点，保存失败时保留输入。

需求和任务按项目展示，状态定义在当前配置中共用。配置目录由 `STATE_DIR/config.toml` 的 `config-repo` 和 `config-id` 解析：

- 状态定义：`CONFIG_DIR/tasks/statuses.json`，包含有序状态列表和配置 revision。
- 需求：`CONFIG_DIR/tasks/inbox/PROJECT-ID/REQUIREMENT-ID.json`，每条需求一个文件，保存标题、Markdown 正文、项目归属、revision 和时间；修改标题不改变文件名。通过现有配置仓库保存和提交，删除需求也记录 Git 变更。
- 执行任务：`STATE_DIR/tasks/CONFIG-ID/tasks.json`，保存任务内容快照、来源需求 ID、当前状态、Agent / 终端关联和执行历史；不会提交到配置仓库。

新结构不读取或迁移原来的 `STATE_DIR/tasks.json`。服务运行期间固定使用启动时选定的配置；切换配置后重启生效，执行记录按配置 ID 隔离。需求文件不包含本地任务 ID；列表中的 `task_ids` 和“已转换”由当前配置下的本地执行记录计算，因此配置复制到其他机器不会携带原机器的执行关联。

服务启动时扫描需求文件建立内存摘要，正文按需从独立文件读取。Inbox 按创建时间倒序分页，每页默认 50 条，上限 200；同一时间以需求 ID 排序。列表不携带正文，返回 `items`、`total`、`next_cursor`。默认隐藏已转换需求；`include_converted=true` 可包含它们。历史需求持续保留，不按时间自动清理。直接修改磁盘文件后需重启服务以重建摘要。

需求从创建时就必须包含项目归属。Web 和 CLI 通过同一服务写入，在线页面通过工作区事件实时刷新。

```bash
# 随手录入；ID 由调用方生成，同一次录入重试时复用它
# 不会创建 Agent 或 Worktree
aow-cli task capture --id idea-20260928-01 --project-id PROJECT-ID --title '支持导出报告' \
  --description '先讨论报告结构和验收标准'
aow-cli task inbox --project-id PROJECT-ID
# 继续读取返回的 next_cursor；需要已转换需求时增加 --include-converted
aow-cli task inbox --project-id PROJECT-ID --limit 50 --cursor NEXT-CURSOR
# 按需读取完整正文和最新 revision
aow-cli task inbox-get idea-20260928-01

# 查看统一状态及实际 status ID；状态名称和 ID 不必相同
aow-cli task statuses
aow-cli task list --project-id PROJECT-ID

# 把需求转为任务并创建交互 Agent，暂不提交需求
# 使用 Inbox 当前 revision，以及 statuses 返回的状态 ID
aow-cli task create --id task-20260928-01 --inbox idea-20260928-01 \
  --expected-revision 1 --title '支持导出报告' \
  --description '先讨论报告结构和验收标准' \
  --project-id PROJECT-ID --cwd /repo --agent codex \
  --status todo

# 在新的 Worktree 中立即执行：创建参数再加
# --cwd /worktrees/report --new-branch task/report --base-ref main --start-now

# 读取任务的执行情况和当前 revision
aow-cli task get task-20260928-01

# Agent 已 ready 后才可提交初始需求；以刚读取的 revision 替换 4
aow-cli task start task-20260928-01 --expected-revision 4

# 报告状态，状态 ID 须在看板的统一状态配置中
# 自动化检查、外部工具脚本或 Agent 均可使用同一接口
aow-cli task set-status task-20260928-01 --status in-review \
  --expected-revision 6 --reason '已创建 PR，等待 Review'
```

`task create` 和 `task start` 返回当前记录，执行准备在后台继续。`execution` 为 `preparing`、`ready`、`submitting`、`submitted` 或 `failed`，与看板 `status_id` 独立。`submitted` 只表示初始内容已发送，后续执行结果由对话和外部检查确认。任务启动会附带任务 ID、可选状态和上报命令，Agent 可用 `aow-cli` 更新状态。

状态更新和初始提交都要求 `expected_revision`。HTTP 409 / CLI 退出码 7 表示记录已改变或操作冲突，需要重新读取并判断，不能盲目覆盖。转换需复用同一个 `--id` 来重试不确定的响应；再次转换同一个需求须使用新任务 ID 和最新 Inbox revision。Agent 启动或提交中断时会保留任务和已创建资源，显示错误；先打开关联终端检查，避免重复发送。归档任务不停止或删除 Agent / Worktree。

任务 API 使用 Web 登录认证；CLI 使用当前用户的私有 Unix socket，与 `agent` 一样要求运行中的 AoW 服务。转换和执行还需要 terminald 及已安装、登录的 Agent。交互启动目前支持 Codex、Trae CLI、Hermes 及这几种类型的自定义配置。自定义状态不会触发内置流水线、PR 轮询或隐式的 Agent 操作；已有定时任务或外部工具可显式调用状态接口。

Web 路径以 `/api/tasks` 为前缀，私有 CLI 路径以 `/v1/tasks` 为前缀，共享以下 JSON 接口：

| 方法与相对路径 | 用途 |
| --- | --- |
| `GET /` | 统一状态和任务快照，包含状态配置版本 status_revision；可用 `?project_id=PROJECT-ID` 筛选任务 |
| `GET /inbox` | 需求摘要分页；支持 `project_id`、`limit`、`cursor` 和 `include_converted`，返回 `items`、`total`、`next_cursor` |
| `GET /inbox/{id}` | 需求详情，包含完整 Markdown 正文和 revision |
| `POST /inbox` | 创建或编辑需求，携带 `project_id`；创建使用固定 `id`，编辑携带 `expected_revision`，不可更改项目归属 |
| `POST /inbox/{id}/delete` | 删除尚未转换的需求 |
| `POST /inbox/{id}/convert` | 转换需求并准备 Agent，支持 `start_now` 和 `worktree` |
| `POST /statuses` | 更新统一的有序状态列表；expected_revision 使用 status_revision，使用中的状态不能删除 |
| `GET /items/{id}` | 任务详情、执行上下文及状态记录 |
| `POST /items/{id}/status` | 设置 `status_id`，可附 `reason`，要求 `expected_revision` |
| `POST /items/{id}/start` | 一次性提交初始需求，要求 `expected_revision` |
| `POST /items/{id}/archive` | 切换归档状态，要求 `expected_revision` |
