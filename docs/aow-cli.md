# AoW CLI 使用

`aow-cli` 随 AoW 一起安装，用于查询项目、启动 Agent 和查看自动化执行记录。安装方式见 [README](../README.md)，请将 `~/.local/bin` 加入 `PATH`。

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
