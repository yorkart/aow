# AoW

## 项目介绍

**AoW — Agent-oriented Workbench**，面向 AI Agent 的浏览器开发工作台，运行在你自己的电脑或服务器上。

在一个界面中管理 Git 仓库和 Worktree、浏览和编辑文件、使用 Terminal、查看 Agent 会话与执行自动化任务。支持 Codex、Claude Code 等主流Agent，提供桌面和手机界面。

详细功能见 [使用说明](docs/usage.md)。

产品门户：[yorkart.github.io/aow](https://yorkart.github.io/aow/)。门户独立通过 GitHub Pages 发布，源码、预览与部署说明见 [website/README.md](website/README.md)。

## 安装 / 升级

支持 **Linux、macOS 的 x86_64 和 ARM64**。运行环境需要 Bash、Node.js 20+、Git、tar 和 curl；Linux 需要可用的 systemd 用户服务，macOS 需要当前用户已登录图形会话。

在需要运行 AoW 的机器上安装：

```bash
curl -fsSL https://github.com/yorkart/aow/releases/latest/download/aow-install.sh | bash
```

安装器从 GitHub Release 获取对应平台的包，校验后安装并启动 Web 服务。首次安装需要在终端设置 6 位访问 PIN；提示启动 terminald 时输入小写 `y`，才能使用 Terminal。升级时重启 terminald 会结束它管理的现有终端会话。

安装后升级：

```bash
aow update
```

请将 `~/.local/bin` 加入 `PATH`；修改访问 PIN 使用 `aow pin`。

版本与产物见 [GitHub Releases](https://github.com/yorkart/aow/releases)，指定版本安装与排错见 [安装说明](docs/release-installation.md)。

## 如何访问

Web 服务默认仅监听 `127.0.0.1:8282`，在本机打开 `http://127.0.0.1:8282/` 并输入安装时设置的 PIN。远程访问可使用 SSH 隧道。

### 通过 IP 直接访问（需显式开启）

在 `~/.config/aow/server.env` 中选择一种绑定方式（仅保留一项，指定地址请替换为网卡的实际 IP；其他示例使用时取消注释）：

```dotenv
# 指定 IPv4 地址
AOW_SERVER_HOST=192.168.1.10

# 全部 IPv4 接口
# AOW_SERVER_HOST=0.0.0.0

# 指定 IPv6 地址
# AOW_SERVER_HOST=2001:db8::10

# 全部 IPv6 接口，部分系统也接受 IPv4 连接
# AOW_SERVER_HOST=::
```

按 [配置说明](docs/configuration.md#监听地址与服务配置) 使设置生效，并在防火墙或安全组中仅允许可信来源访问 TCP 8282，然后打开 `http://<服务器IP>:8282/`（IPv6 使用 `http://[IPv6地址]:8282/`）。手机访问时自动进入移动界面。

IP 直连适用于可信内网或 VPN，访问来源应受到限制。AoW 能操作运行账号的文件和终端，6 位 PIN 只是轻量访问门槛；公网访问请配置 HTTPS 和额外的访问控制。

### 通过 SSH 隧道访问

访问自己远程机器上的 AoW 时，可以通过 SSH 隧道安全连接。在本机执行以下命令，将 `user@host` 替换为远程机器的 SSH 登录地址：

```bash
ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:18282:127.0.0.1:8282 user@host
```

保持 SSH 连接，在本机浏览器打开 `http://127.0.0.1:18282/`。本地端口 `18282` 可以替换为其他空闲端口。

默认监听配置即可使用隧道，服务器只需开放 SSH 端口。

## 本地开发与环境要求

在运行依赖之外，准备 Rust stable / Cargo、npm 和 just。macOS 需要 Xcode Command Line Tools；Linux 打包需要 C 编译器、`musl-gcc`、`readelf` 和本机架构的 Rust musl target。macOS 安装脚本测试还需要 Python 3。

在源码目录运行 `just build` 编译、`just test` 测试；本地部署先 `just package`，再 `just install`。本地包与 GitHub 包使用相同安装流程，会更新当前用户实际运行的服务。开发服务也默认仅监听本机。
