# 配置说明

## 监听地址与服务配置

编辑 `~/.config/aow/server.env`，文件不存在时先创建。每行写 `KEY=value`，不写 `export`；路径使用绝对路径，修改时保留其他配置。

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

## 配置仓库与版本

在 **Settings → Configuration** 中输入服务所在机器上的 Git 仓库根目录，点击“读取版本”，再单选一个 UUID 版本目录并保存。输入 UUID 版本目录时，会自动定位到父仓库并选中该版本；非 Git 仓库或普通子目录会提示错误。

选择保存在数据目录下的 `config.toml`（默认 `~/.local/state/aow/config.toml`）：

```toml
config-repo = "/absolute/path/to/config-repo"
config-id = "550e8400-e29b-41d4-a716-446655440000"
```

实际配置目录是 `<config-repo>/<config-id>/`。保存有变化时会提示重启 AoW 服务；重启前，运行中的服务继续使用原版本。再次打开 Settings 可以查看已保存的选择和当前运行版本。保存相同选择不会重复写入文件。

服务在启动时将 `config.toml` 载入内存，运行期间不再读取该文件。Settings 查询从内存返回；保存使用内存中的配置文档写入文件，并更新待重启的选择，当前生效版本不变。外部编辑文件也只有重启服务后才会加载。

配置版本只通过 `config.toml` 选择，旧 `__current__` 文件会被忽略，不读取或迁移。缺少 `config.toml` 时，服务按首次初始化创建默认配置；用户可在 **Settings → Configuration** 中重新选择已有仓库和版本，保存后重启服务。

`config.toml` 当前只使用这两个顶层字段。保存版本选择会保留其他字段和注释，未来可扩展分层配置；当前尚未实现全局、用户和项目配置的覆盖规则。

## Notes 目录与访问 PIN

- **Notes 目录**：在 **Settings → Notes** 中修改，默认 `~/aow`。保存后迁移使用默认路径的 Notes，自定义绑定的 Notes 路径保持不变。
- **访问 PIN**：在服务器终端执行 `aow pin`。手动使用自定义数据目录时，执行 `AOW_SERVER_STATE_DIR=/path/to/state aow pin`；修改后重新登录网页。

服务日志和故障排查见 [安装说明](release-installation.md)。
