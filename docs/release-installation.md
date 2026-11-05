# 安装与维护

## 环境要求

支持 Linux、macOS 的 x86_64 和 ARM64，需要 Bash、Node.js 20+、Git、tar 和 curl。

- **Linux**：当前用户可使用 systemd 用户服务。
- **macOS**：首次安装检查安装账户当前是否有图形会话。有则使用 LaunchAgent，随该账户桌面登录启动；没有则使用 LaunchDaemon，无需桌面登录。已有安装和后续更新沿用记录的模式，不随登录状态切换。

## 安装与升级

在运行 AoW 的机器上，以实际运行 AoW 的系统用户执行：

```bash
curl -fsSL https://github.com/yorkart/aow/releases/latest/download/aow-install.sh | bash
```

首次安装必须按提示输入登录账号、密码并再次确认密码。密码输入不回显；未完成账户设置或取消输入时不会启动服务。安装和升级默认更新并启动或重启 host-daemon，无需额外确认；重启会结束它管理的现有终端会话。

需要保留 host-daemon 当前状态和终端会话时，添加 `--skip-host`：

```bash
just install --skip-host
aow update --skip-host
curl -fsSL https://github.com/yorkart/aow/releases/latest/download/aow-install.sh | bash -s -- --skip-host
```

该参数仅影响本次安装或升级，不保存为后续默认设置。Web 服务、CLI 和 Runner 仍正常更新；host-daemon 的服务、当前版本链接和托管 Node 运行时保持不变。首次安装使用该参数时不会启动 host-daemon，需事先配置可用节点的 URL 和本机令牌文件，Server 启动时必须能读取令牌文件。

host-daemon 统一管理 ACP adapter 和 PTY。重启会中断活动 ACP 连接并结束终端进程。
ACP 会话历史由 Server 保存；适配器原生会话按其 `session/load` 能力恢复。


## 安装后的目录

以下为默认位置：

| 路径 | 内容 |
| --- | --- |
| `~/.local/bin/` | `aow`、`aow-cli`、`aow-automation-runner` 命令入口 |
| `~/.local/lib/aow/releases/` | 各版本的程序和网页文件 |
| `~/.local/lib/aow/active/` | Web 服务与 host-daemon 当前使用的版本链接 |
| `~/.local/lib/aow/update.json` | 更新来源、命令目录，以及 macOS 服务模式、账户和 UID |
| `~/.local/lib/aow/pending-launchdaemon.json` | 等待管理员登记的 LaunchDaemon 请求；完成安装验证后清除 |
| `~/.aow/server.env` | 服务配置 |
| `~/.aow/` | 应用配置、登录凭据和技能 |
| `~/.aow/data/` | ACP 会话和应用管理的用户数据 |
| `~/.aow/state/` | 终端状态、自动化记录和操作日志 |
| `~/.aow/cache/` | 下载、安装缓存和剪贴板图片 |
| `~/.aow/runtime/` | 本机通信 socket |
| `~/.aow/data/notes/` | 默认 Notes 目录 |
| `~/.config/systemd/user/aow-*.service` | Linux 服务配置 |
| `~/Library/LaunchAgents/org.aow.*.plist` | macOS LaunchAgent 配置 |
| `/Library/LaunchDaemons/org.aow.service.*.plist` | macOS LaunchDaemon 配置，由 root 管理，服务以指定普通账户运行 |

备份时保留配置目录、数据目录和 Notes；使用自定义路径时备份实际位置。命令行用法见 [CLI 使用说明](aow-cli.md)。

`update.json` 中的 macOS 记录示例：

```json
{ "service": { "mode": "launchdaemon", "user": "aow-service", "uid": 502 } }
```

该记录用于选择安装方式，不是权限凭据。安装器会同时检查实际账户与服务配置；冲突时停止安装。切换模式需要显式迁移，不能仅修改这个字段。已有 LaunchAgent 配置会被识别并保留；没有图形会话时会提示恢复会话，不会自动改装成 LaunchDaemon。

## 修改配置

编辑 `~/.aow/server.env`，按需设置监听地址、端口、数据目录或访问路径，具体选项见 [配置说明](configuration.md)。默认仅监听 `127.0.0.1:8282`。

- **Linux**：保存后重启 Web 服务。
- **macOS**：保存后重新执行安装命令；`HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY`（含小写形式）由 Web 启动脚本读取，已有服务只改代理无需管理员登记。其他 LaunchDaemon 配置发生变化时需按提示由管理员重新登记，再重跑安装验证。只改 Web 配置时可加上 `--skip-host` 跳过 host-daemon 重启。本地源码有改动时先执行 `just package`，因为 `just install` 只安装已有包。

## 服务管理与系统日志

以安装 AoW 的同一系统用户执行以下命令。重启 Web 服务不会结束后台终端；ACP 连接关闭会结束其适配器进程；重启 host-daemon 会结束其管理的所有终端会话。

LaunchDaemon 更新会先发送 SIGTERM，等待最多 5 秒正常退出；若旧进程仍未退出，安装器会重新核对 PID 与账户归属，再用 SIGKILL 结束该进程，并检查新进程的健康状态。这样可避免长连接让 Web 服务停在「端口已关闭但进程未退出」的状态。指定 `--skip-host` 时，只重启 Web 服务，host-daemon 及其终端会话保持运行。

### Linux

```bash
# 查看状态
systemctl --user status aow-server.service aow-host-daemon.service

# 重启 Web 服务
systemctl --user restart aow-server.service

# 需要重启终端服务时执行
systemctl --user restart aow-host-daemon.service
```

系统日志由 journald 管理：

```bash
journalctl --user -u aow-server.service -u aow-host-daemon.service -f
# 最近一小时的日志
journalctl --user -u aow-server.service -u aow-host-daemon.service --since '1 hour ago'
```

### macOS LaunchAgent

```bash
# 查看状态
launchctl print "gui/$(id -u)/org.aow.server"
launchctl print "gui/$(id -u)/org.aow.host-daemon"

# 使用现有配置重启 Web 服务
launchctl kickstart -k "gui/$(id -u)/org.aow.server"

# 需要重启终端服务时执行
launchctl kickstart -k "gui/$(id -u)/org.aow.host-daemon"
```

### macOS LaunchDaemon

```bash
launchctl print system/org.aow.service.server
launchctl print system/org.aow.service.host-daemon
```

日常更新在服务账户下执行 `just install` 或 `aow update`。配置未变时，安装器切换发布版本，并仅向该账户自己的服务进程发送退出信号；launchd 自动拉起新版本，健康检查失败则尝试恢复旧版本。安装器不执行 sudo，不修改系统 plist。

需要手动启停系统服务时，由管理员在本机终端使用 launchctl；不要给服务账户添加管理系统服务的提权规则。

### macOS 日志

系统日志存储在 macOS Unified Logging 中，可在“控制台”应用按 `org.aow` 搜索，或执行：

```bash
/usr/bin/log stream --style compact --predicate 'subsystem == "org.aow"'
# 最近一小时的日志
/usr/bin/log show --last 1h --style compact --predicate 'subsystem == "org.aow"'
```

## 常见问题

- **找不到 `aow` 命令**：在 shell 配置中加入 `export PATH="$HOME/.local/bin:$PATH"`，重新打开终端；也可直接使用 `~/.local/bin/aow`。
- **网页无法打开**：先检查服务状态和日志，再执行 `curl -fsS http://127.0.0.1:8282/api/health`。自定义地址、端口或 Base Path 时替换为对应 URL；从其他设备访问请按 README 配置 SSH 隧道或 IP 直连。
- **网页可用但 Terminal 不可用**：检查 host-daemon 状态；首次安装跳过其启动时，重新执行安装命令，不带 `--skip-host`。
