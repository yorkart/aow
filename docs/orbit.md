# AoW Orbit

Orbit 是同仓库内独立构建和运行的实验应用。`frontend-orbit` 使用 React / TypeScript，`server-orbit` 使用 Node.js / TypeScript 和内置 SQLite。需要 Node.js 22.16+（22.x）或 24+、npm 和 Bash。

本期包含独立登录、组织（org）的创建和切换，以及空白工作窗口。Tasks、Projects、Automation、Agents、Skills、Connectors 仅提供图标导航，尚未接入业务功能或 ACP / terminald。没有预置组织或演示数据。

## 启动

从仓库根目录运行：

```bash
./scripts/orbit/start.sh
```

缺少构建产物时，脚本先安装 Orbit 自己的依赖并构建。首次启动在终端提示配置用户名、密码并确认密码；已有账户会保留。然后访问 <http://127.0.0.1:8382>，登录并创建第一个组织。没有组织时始终显示引导；创建后进入工作窗口。

左侧 org 下拉菜单支持创建其他组织、切换组织和退出登录。组织、当前选择和登录会话保存在 SQLite，刷新或服务重启后仍可恢复；会话受有效期和空闲超时限制。

源码更新后重新构建，再启动：

```bash
./scripts/orbit/build.sh
./scripts/orbit/start.sh
```

修改登录账户：

```bash
./scripts/orbit/account.sh
```

修改账户会使已有登录会话失效，组织数据保留。自动化部署可通过 `ORBIT_USERNAME` 和 `ORBIT_PASSWORD` 环境变量提供账户信息；不要把密码作为命令行参数。启动脚本只在没有账户时使用这些变量。

## 数据与配置

默认数据文件是 `~/.aow-orbit/orbit.sqlite`。数据库包含账户、登录会话和组织，密码使用与 AoW 一致的 PBKDF2-HMAC-SHA256（600,000 次迭代）加盐哈希。会话令牌通过 Orbit 专用的 HttpOnly / SameSite=Strict Cookie 传输，数据库仅保存令牌哈希。

首次打开数据库时创建必要的表；已有数据库直接复用。本期没有数据导入或额外的版本迁移框架。

| 环境变量 | 默认值 | 用途 |
| --- | --- | --- |
| `ORBIT_HOST` | `127.0.0.1` | 服务监听地址 |
| `ORBIT_PORT` | `8382` | 服务端口 |
| `ORBIT_STATE_DIR` | `~/.aow-orbit` | 独立数据目录 |
| `ORBIT_FRONTEND_DIR` | `frontend-orbit/dist` | 静态前端构建目录，默认相对于项目位置 |
| `ORBIT_FRONTEND_ORIGIN` | 未设置 | 独立前端部署时允许的精确来源，例如 `https://orbit.example.com` |
| `ORBIT_SECURE_COOKIES` | 未设置 | HTTPS 部署时设为 `1` |
| `ORBIT_SESSION_MAX_AGE_SECONDS` | `43200` | 登录会话最长有效期 |
| `ORBIT_SESSION_IDLE_TIMEOUT_SECONDS` | `1800` | 登录会话空闲超时 |

## 独立前后端开发与部署

两个目录分别维护 package.json、锁文件和构建命令；没有跨前端目录导入。登录界面和主题色板从现有 AoW 复制到 Orbit 后独立维护，主题偏好键为 `orbit.theme`。色板来源见 [AoW 主题说明](themes.md)。

后端可以单独构建和启动：

```bash
npm --prefix server-orbit ci --ignore-scripts --no-audit --no-fund
npm --prefix server-orbit run build
npm --prefix server-orbit run account
npm --prefix server-orbit start
```

另一个终端启动前端开发服务：

```bash
npm --prefix frontend-orbit ci --ignore-scripts --no-audit --no-fund
npm --prefix frontend-orbit run dev
```

访问 <http://127.0.0.1:5174>。Vite 默认将 `/api` 代理到 `http://127.0.0.1:8382`；可用 `ORBIT_API_TARGET` 指定其他后端地址。

生产环境可由启动脚本中的 Node 服务同时提供静态前端，也可以将 `frontend-orbit/dist` 单独部署，通过同源网关转发 `/api`。若前端与 API 使用不同来源，在前端构建时设置 `VITE_ORBIT_API_URL` 为后端来源，同时为后端设置 `ORBIT_FRONTEND_ORIGIN`。使用同一站点下的域名或相同主机的不同端口，以符合 SameSite Cookie 策略。

## 验证

```bash
npm --prefix server-orbit run lint
npm --prefix server-orbit test
npm --prefix frontend-orbit run lint
npm --prefix frontend-orbit run build
npm --prefix frontend-orbit test
```

浏览器测试依赖 Playwright Chromium。可设置 `ORBIT_TEST_BROWSER` 为系统 Chromium 的绝对路径，或运行 `npm --prefix frontend-orbit exec -- playwright install chromium` 安装测试浏览器。测试使用临时 SQLite 数据库及独立服务端口。
