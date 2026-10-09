# 配置说明

## 监听地址与服务配置

在 **Settings → Environment → AoW 服务环境变量** 中直接编辑并保存 `~/.config/aow/server.env`，也可以在服务器上编辑该文件。文件不存在时，页面保存会创建它；注释、引号和空行按原文保留。每行写 `KEY=value`，不写 `export`；路径使用绝对路径，修改时保留其他配置。

页面保存只更新本机文件，不写入配置 Git 仓库，也不会自动重启服务。按下方“使配置生效”的步骤加载配置。文件被其他窗口或外部程序修改时，保存会提示冲突并保留草稿，请重新读取后再编辑。Environment 中的执行 PATH 仍单独保存，对后续启动的 Agent 和自动化任务生效。

```dotenv
AOW_SERVER_HOST=127.0.0.1
AOW_SERVER_PORT=8282

# 自定义数据目录（默认 ~/.local/state/aow）
# AOW_SERVER_STATE_DIR=/path/to/state

# terminald 的 socket 路径，须与其监听路径一致
# AOW_TERMINALD_SOCKET=/path/to/terminald.sock
```

`AOW_SERVER_HOST` 选择一项：

| 值 | 监听范围 |
| --- | --- |
| `127.0.0.1` | 仅 IPv4 本机，默认值 |
| `::1` | 仅 IPv6 本机 |
| 指定网卡的 IPv4 / IPv6 地址 | 仅该地址 |
| `0.0.0.0` | 全部 IPv4 接口，含公网接口（如有） |
| `::` | 全部 IPv6 接口，含公网接口（如有）；部分系统也接受 IPv4 |

IPv6 浏览器地址使用方括号，例如 `http://[::1]:8282/`。若改为 IPv6 监听，同步修改 SSH 隧道目标或反向代理上游地址。

### 使配置生效

- **Linux**：执行 `systemctl --user restart aow-server.service`。
- **macOS**：重新执行对应的安装命令（GitHub 安装命令或 `just install`），重新加载配置；只改 Web 服务配置时可跳过 terminald 重启。

macOS **LaunchDaemon** 模式会将配置写入由 root 管理的系统 plist。配置变化时，安装器先准备请求并以退出码 78 提示本地管理员登记；登记后回到服务账户重跑安装验证。仅程序版本变化不需要重新登记，步骤见 [安装说明](release-installation.md)。

LaunchDaemon 的 `server.env` 仅接受服务配置和基本运行环境：`AOW_SERVER_HOST`、`AOW_SERVER_PORT`、`AOW_SERVER_STATE_DIR`、`AOW_STATE_DIR`、`AOW_TERMINALD_SOCKET`、`AOW_BASE_PATH`、`AOW_AUTH_SECURE_COOKIE`、`XDG_STATE_HOME`、`XDG_RUNTIME_DIR`、`PATH`、`SHELL`、`LANG`、`LC_ALL`、`LC_CTYPE`。账户身份、HOME、运行目录和日志模式由安装器固定。系统 plist 可被其他本机账户读取，不要在其中填写 API 密钥或口令；其他环境变量会被拒绝。

LaunchDaemon 默认 PATH 包含 `~/.local/bin`、AoW 管理的 Node 入口、`~/.cargo/bin` 和常用系统/Homebrew 目录，不继承安装终端的完整 PATH；开发 shell 可在服务账户自己的 `.zprofile` / `.zshrc` 中初始化 Node 和其他工具链。

修改 `AOW_TERMINALD_SOCKET` 时，terminald 也需配置同一路径：

- **Linux**：执行 `systemctl --user edit aow-terminald.service`，添加以下配置，再执行 `systemctl --user daemon-reload` 和 `systemctl --user restart aow-terminald.service aow-server.service`。
- **macOS**：重新执行安装，在 terminald 重启提示中输入小写 `y`。

```ini
[Service]
Environment="AOW_TERMINALD_SOCKET=/path/to/terminald.sock"
```

重启 terminald 会结束现有终端会话。

直接运行二进制时不会读取 `server.env`，需使用 `--host`、`--port`、`--state-dir`、`--terminald-socket`、`--base-path` 参数设置。

### 终端语言与中文显示

新建终端继承 terminald 的语言环境；`LANG` 未设置或为空时，macOS 默认使用 `en_US.UTF-8`，Linux 默认使用 `C.UTF-8`。已有的非空 `LANG` 和所有 `LC_*` 设置会保留，`LC_ALL`、`LC_CTYPE` 仍按系统规则决定字符编码。

旧会话中如果 Vim 将 UTF-8 中文显示成 `~X` 等字符，可退出 Vim 后用 `LC_ALL=en_US.UTF-8 vim 文件路径` 临时打开（Linux 使用 `LC_ALL=C.UTF-8`）。运行 `locale` 可检查语言环境，Vim 中执行 `:set encoding? fileencoding?` 可查看当前编码。更新 terminald 后，新建会话才会获得新的默认值；重启 terminald 会结束现有终端会话。

## 部署到域名子目录（Base Path）

在 `server.env` 中设置：

```dotenv
AOW_BASE_PATH=/tools/aow
```

按上面的步骤使配置生效后，通过 `https://example.com/tools/aow/` 访问。使用反向代理时保留请求前缀；恢复根目录部署时将值改为 `/`。

默认监听地址下检查服务（修改过地址或端口时同步替换）：

```bash
curl --fail --show-error http://127.0.0.1:8282/tools/aow/api/health
```

更换路径后更新书签，以及 **Settings → 通知 → AoW 访问地址**。

## 登录与 HTTPS

本机 HTTP 和 SSH 隧道访问默认保持兼容。通过 HTTPS 反向代理访问时，在 `server.env` 中设置并按上面的步骤重启服务：

```dotenv
AOW_AUTH_SECURE_COOKIE=true
```

直接运行二进制也可使用 `--secure-cookies`。该设置为登录和清除会话的 Cookie 添加 `Secure`；它不为后端启用 TLS，反向代理仍需负责 HTTPS、HTTP 跳转和 HSTS。代理应保留浏览器请求的 `Host`，以便登录/退出请求检查 `Origin`。服务端不根据客户端可伪造的 `X-Forwarded-Proto` 降低 Cookie 安全设置。只接受 `true` 或 `false`，本机 HTTP 默认为 `false`。

登录会话最长保留 12 小时，30 分钟没有工作台 API 请求或终端输入时过期；登录状态轮询和终端输出不延长空闲期限。桌面在 Settings 中、手机在项目首页右上角可退出登录。退出、会话过期、修改账号/密码后，旧会话的新请求和终端输入会被拒绝，已建立的终端和事件流最迟在下一次会话检查时断开（约 1 秒）；这不会结束 terminald 中的进程。

登录校验全局最多并发 2 个，最多突发 10 次，此后每 6 秒恢复一次额度。连续 5 次凭据错误后开始退避，最长 60 秒；被限流时返回 `429` 和 `Retry-After`。这些限制不使用客户端提交的账号名或转发 IP 来创建独立额度。重启服务会清空内存会话和限流状态。

文件原始接口对 HTML、SVG、XML、脚本及其他未允许内联的类型强制下载，并设置沙箱和禁止类型嗅探的响应头。普通图片、PDF、纯文本和受支持音视频仍可预览；下载后在本机打开文件应自行判断其来源。

### 登录通知与审计日志

每次登录成功或失败（包括错误密码、无效请求、来源检查失败和限流），都会记录日志并向当前已配置的全部 IM 渠道（飞书、微信）逐条发送通知。它独立于“Agent 完成通知”开关；未配置 IM 时仍记录日志。未登录访问受保护接口、文件页、帮助页或直接打开工作台/其他非公开页面地址，只记录日志，不推送通知。健康检查、登录状态轮询、前端静态资源、公开分享页面及其只读接口不记录这类访问审计。工作台入口仍显示登录表单，受保护的数据接口仍拒绝匿名请求。

操作日志面板可按“认证”来源，或“登录 / 未登录访问 / 认证通知”类型筛选，并按 IP、账号、事件 ID 搜索。记录包括 UTC 时间、事件 ID、服务启动 ID、实际连接 IP 和端口、请求方法/路径（含 Base Path）、HTTP 版本/状态、处理耗时、提交的账号、验证后的身份、User-Agent、来源页面、结果和原因。密码、Cookie、Authorization、请求正文、查询参数及分享令牌不会写入审计记录；来自请求的字符串会限制长度并清除控制字符。

`peer_ip` 是服务实际收到的 TCP 连接来源。经过反向代理或 SSH 隧道时，它可能是代理/隧道端点；`X-Forwarded-For`、`X-Real-IP`、`Forwarded` 会另外记录为 `*_unverified`，不能直接当作已验证的客户端 IP。回溯时应结合可信反向代理的访问日志；代理应覆盖来自客户端的伪造转发头。

日志位于服务数据目录的 `operation-logs/operations.YYYY-MM-DD-HH.log`（默认 `~/.local/state/aow/operation-logs/`），按小时轮转，最多保留 720 个文件。登录结果先写入日志，再交给独立的有界通知队列；满队列会等待，未登录访问的日志不受通知队列影响。各渠道的发送成功、失败或 30 秒超时都会使用同一事件 ID 追加记录；投递失败不改变登录结果，也不会阻止其他渠道尝试。队列不跨服务重启恢复，已落盘的审计日志保留；通知未送达时可据此排查。底层 IM 渠道可能有自身的速率限制。

## 配置仓库与版本

在 **Settings → Configuration** 中输入服务所在机器上的 Git 仓库根目录，点击“读取版本”，再单选一个配置版本目录并保存。输入配置版本目录时，会自动定位到父仓库并选中该版本；非 Git 仓库或普通子目录会提示错误。

选择保存在数据目录下的 `config.toml`（默认 `~/.local/state/aow/config.toml`）：

```toml
config-repo = "/absolute/path/to/config-repo"
config-id = "g123456789ab"
```

实际配置目录是 `<config-repo>/<config-id>/`。保存有变化时会提示重启 AoW 服务；重启前，运行中的服务继续使用原版本。再次打开 Settings 可以查看已保存的选择和当前运行版本。保存相同选择不会重复写入文件。

服务在启动时将 `config.toml` 载入内存，运行期间不再读取该文件。Settings 查询从内存返回；保存使用内存中的配置文档写入文件，并更新待重启的选择，当前生效版本不变。外部编辑文件也只有重启服务后才会加载。

配置版本只通过 `config.toml` 选择，旧 `__current__` 文件会被忽略，不读取或迁移。缺少 `config.toml` 时，服务按首次初始化创建默认配置；用户可在 **Settings → Configuration** 中重新选择已有仓库和版本，保存后重启服务。

`config.toml` 当前只使用这两个顶层字段。保存版本选择会保留其他字段和注释，未来可扩展分层配置；当前尚未实现全局、用户和项目配置的覆盖规则。

## Notes 目录与登录账户

- **Notes 目录**：在 **Settings → Notes** 中修改，默认 `~/aow`。保存后仅更新新注册项目使用的默认根目录，已有项目仍使用原 Notes 路径。如需切换，请在项目菜单中使用「绑定 Notes 目录」：目录已存在时直接使用，不存在时自动创建；两处操作均不迁移笔记。
- **登录账户**：在服务器终端执行 `aow account`。手动使用自定义数据目录时，执行 `AOW_SERVER_STATE_DIR=/path/to/state aow account`；按提示输入账号、密码并确认密码，修改后重新登录网页。

账户仅保存在服务所在机器的数据目录中（默认 `~/.local/state/aow/credentials.json`），权限为 `0600`。当前使用一个本地账户，保存账号、随机盐和 PBKDF2-HMAC-SHA256 密码哈希，不保存明文密码。账户不进入配置 Git 仓库。首次安装必须设置，后续升级保留；忘记密码可在服务器终端重新运行 `aow account`，无需重启服务。旧 `pin.md5` 不再读取，也不会自动转换成账户。

服务日志和故障排查见 [安装说明](release-installation.md)。
