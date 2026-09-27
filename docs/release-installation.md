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

首次安装必须按提示输入登录账号、密码并再次确认密码，并在启动 terminald 的提示中输入小写 `y`，启用网页 Terminal。密码输入不回显；未完成账户设置或取消输入时不会启动服务。升级时可跳过 terminald 重启，保留现有终端会话；重启 terminald 会结束这些会话。

### macOS 本地安装与目标账户

macOS 本地安装需要 Python 3.9+。`just install` 默认安装到当前账户；`--user` 指定实际运行 AoW 的账户。安装器先显示目标账户，再检查该账户的图形会话和已有服务登记：

```bash
just install                           # 当前账户，使用 target/packages/latest
just install --user aow-service         # 指定运行账户
just install /path/to/package.tar.gz --user aow-service
```

有图形会话时使用 LaunchAgent，当前账户可直接安装。无图形会话、尚未登记 LaunchDaemon 且发起安装的账户不是管理员时，安装器会在设置登录账户和切换版本前停止，并提示到管理员账户执行 `just install --user aow-service`。只有安装目录或待登记请求不代表登记已经完成。

管理员须在这台 Mac 的本地交互终端中，使用自己拥有、服务账户不可写的可信 AoW 源码和安装包运行该命令（需要 Python 3）。可先执行 `just package` 生成本地包。安装器按需请求管理员密码，然后以目标用户身份安装文件和配置登录账号和密码，以管理员权限登记系统服务，最后回到目标用户身份验证启动；整个流程无需手动切换账户。terminald 的启动或重启只确认一次。

目标账户需要 Node.js 20+。安装器会以目标用户身份读取其登录 shell 的 PATH，支持该账户通过 nvm 安装的 Node.js；管理员的环境变量和凭据不会传给目标账户，服务配置读取目标账户的 `~/.config/aow/server.env`。

首次从下载脚本安装无图形服务账户时，也会提示使用上述管理员入口。登记完成后，回到服务账户使用 `just install` 或 `aow update` 更新，无需 sudo；已有安装保持原服务模式。配置变化需要重新登记时，仍会以退出码 **78** 提示管理员处理。

已有待登记请求也可由本地管理员在自己的可信源码目录中手动登记：

```bash
sudo -k /usr/bin/python3 -I scripts/register-launchdaemon.py --user aow-service
```

将 `aow-service` 替换为实际安装账户；随后回到该账户重新执行安装命令，完成启动验证。不要在服务账户可写的源码或发布目录中以 sudo 执行脚本，也不要给服务账户 sudo 权限。

安装后在本机打开 `http://127.0.0.1:8282/` 并使用安装时设置的账号和密码登录。远程访问方式见 [README](../README.zh-CN.md#如何访问)。

将 `~/.local/bin` 加入 shell 的 `PATH` 后，可使用：

```bash
aow update                         # 升级到最新版本
aow update --version RELEASE-TAG    # 切换到指定版本
aow account                        # 修改登录账号和密码
```

`RELEASE-TAG` 替换为 [GitHub Releases](https://github.com/yorkart/aow/releases) 中的完整版本标签。升级保留配置、登录账户和用户数据；切换旧版本不会恢复旧数据。修改账号或密码后需重新登录网页。旧版仅有 PIN 的安装，升级时必须在交互终端中设置账号和密码；旧 PIN 不再用于登录。

## 安装后的目录

以下为默认位置：

| 路径 | 内容 |
| --- | --- |
| `~/.local/bin/` | `aow`、`aow-cli`、`aow-automation-runner` 命令入口 |
| `~/.local/lib/aow/releases/` | 各版本的程序和网页文件 |
| `~/.local/lib/aow/active/` | Web 服务与 terminald 当前使用的版本链接 |
| `~/.local/lib/aow/update.json` | 更新来源、命令目录，以及 macOS 服务模式、账户和 UID |
| `~/.local/lib/aow/pending-launchdaemon.json` | 等待管理员登记的 LaunchDaemon 请求；完成安装验证后清除 |
| `~/.config/aow/server.env` | 服务配置 |
| `~/.local/state/aow/` | 项目配置、终端状态、自动化记录、登录凭据等用户数据 |
| `~/aow/` | 默认 Notes 目录 |
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

编辑 `~/.config/aow/server.env`，按需设置监听地址、端口、数据目录或访问路径，具体选项见 [配置说明](configuration.md)。默认仅监听 `127.0.0.1:8282`。

- **Linux**：保存后重启 Web 服务。
- **macOS**：保存后重新执行安装命令；LaunchDaemon 配置发生变化时需按提示由管理员重新登记，再重跑安装验证。只改 Web 配置时可跳过 terminald 重启。

## 服务管理与系统日志

以安装 AoW 的同一系统用户执行以下命令。重启 Web 服务不会结束后台终端；重启 terminald 会结束其管理的所有终端会话。

LaunchDaemon 更新会先发送 SIGTERM，等待最多 5 秒正常退出；若旧进程仍未退出，安装器会重新核对 PID 与账户归属，再用 SIGKILL 结束该进程，并检查新进程的健康状态。这样可避免长连接让 Web 服务停在「端口已关闭但进程未退出」的状态。选择 `n` 跳过 terminald 时，只重启 Web 服务，terminald 及其终端会话保持运行。

### Linux

```bash
# 查看状态
systemctl --user status aow-server.service aow-terminald.service

# 重启 Web 服务
systemctl --user restart aow-server.service

# 需要重启终端服务时执行
systemctl --user restart aow-terminald.service
```

系统日志由 journald 管理：

```bash
journalctl --user -u aow-server.service -u aow-terminald.service -f
# 最近一小时的日志
journalctl --user -u aow-server.service -u aow-terminald.service --since '1 hour ago'
```

### macOS LaunchAgent

```bash
# 查看状态
launchctl print "gui/$(id -u)/org.aow.server"
launchctl print "gui/$(id -u)/org.aow.terminald"

# 使用现有配置重启 Web 服务
launchctl kickstart -k "gui/$(id -u)/org.aow.server"

# 需要重启终端服务时执行
launchctl kickstart -k "gui/$(id -u)/org.aow.terminald"
```

### macOS LaunchDaemon

```bash
launchctl print system/org.aow.service.server
launchctl print system/org.aow.service.terminald
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
- **网页可用但 Terminal 不可用**：检查 terminald 状态；首次安装跳过其启动时，重新执行安装命令并输入小写 `y`。
