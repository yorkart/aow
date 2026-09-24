# 安装与维护

## 环境要求

支持 Linux、macOS 的 x86_64 和 ARM64，需要 Bash、Node.js 20+、Git、tar 和 curl。

- **Linux**：当前用户可使用 systemd 用户服务。
- **macOS**：当前用户已登录图形会话；服务随用户登录启动，退出登录后停止。

## 安装与升级

在运行 AoW 的机器上，以日常使用的系统用户执行：

```bash
curl -fsSL https://github.com/yorkart/aow/releases/latest/download/aow-install.sh | bash
```

首次安装按提示设置 6 位 PIN，并在启动 terminald 的提示中输入小写 `y`，启用网页 Terminal。升级时可跳过 terminald 重启，保留现有终端会话；重启 terminald 会结束这些会话。

安装后在本机打开 `http://127.0.0.1:8282/` 并输入 PIN。远程访问方式见 [README](../README.md#如何访问)。

将 `~/.local/bin` 加入 shell 的 `PATH` 后，可使用：

```bash
aow update                         # 升级到最新版本
aow update --version RELEASE-TAG    # 切换到指定版本
aow pin                            # 修改访问 PIN
```

`RELEASE-TAG` 替换为 [GitHub Releases](https://github.com/yorkart/aow/releases) 中的完整版本标签。升级保留配置、PIN 和用户数据；切换旧版本不会恢复旧数据。修改 PIN 后需重新登录网页。

## 安装后的目录

以下为默认位置：

| 路径 | 内容 |
| --- | --- |
| `~/.local/bin/` | `aow`、`aow-cli`、`aow-automation-runner` 命令入口 |
| `~/.local/lib/aow/releases/` | 各版本的程序和网页文件 |
| `~/.local/lib/aow/active/` | Web 服务与 terminald 当前使用的版本链接 |
| `~/.config/aow/server.env` | 服务配置 |
| `~/.local/state/aow/` | 项目配置、终端状态、自动化记录、PIN 等用户数据 |
| `~/aow/` | 默认 Notes 目录 |
| `~/.config/systemd/user/aow-*.service` | Linux 服务配置 |
| `~/Library/LaunchAgents/org.aow.*.plist` | macOS 服务配置 |

备份时保留配置目录、数据目录和 Notes；使用自定义路径时备份实际位置。命令行用法见 [CLI 使用说明](aow-cli.md)。

## 修改配置

编辑 `~/.config/aow/server.env`，按需设置监听地址、端口、数据目录或访问路径，具体选项见 [配置说明](configuration.md)。默认仅监听 `127.0.0.1:8282`。

- **Linux**：保存后重启 Web 服务。
- **macOS**：保存后重新执行安装命令，使新配置生效；只改 Web 配置时可跳过 terminald 重启。

## 服务管理与系统日志

以安装 AoW 的同一系统用户执行以下命令。重启 Web 服务不会结束后台终端；重启 terminald 会结束其管理的所有终端会话。

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

### macOS

```bash
# 查看状态
launchctl print "gui/$(id -u)/org.aow.server"
launchctl print "gui/$(id -u)/org.aow.terminald"

# 使用现有配置重启 Web 服务
launchctl kickstart -k "gui/$(id -u)/org.aow.server"

# 需要重启终端服务时执行
launchctl kickstart -k "gui/$(id -u)/org.aow.terminald"
```

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
