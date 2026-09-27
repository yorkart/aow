# 统一认证模块

业务路由只接入 `AuthService` 和 `require_auth`，不依赖具体的登录方式。

认证子路由由本模块定义，在 [routes.rs](../routes.rs) 的 `build_router` 中统一挂载；公共中间件和 Base Path 挂载也由该入口装配。

| 文件 | 职责 |
| --- | --- |
| `mod.rs` | 注册登录方式、分发认证请求、查询登录状态、统一校验会话 |
| `provider.rs` | `LoginProvider` 接口和认证后的身份契约 |
| `password.rs` | 本地账号文件读取、密码哈希校验、凭据变更后的会话失效 |
| `sessions.rs` | 生成会话 token、绝对/空闲过期、最多 64 个会话、撤销和连接取消信号 |
| `limits.rs` | 进入密码校验线程前的全局速率、并发限制及失败退避 |
| `http.rs` | 登录、状态和退出 API、来源检查、禁止缓存、Cookie 属性与 Base Path 作用域 |
| `authorization.rs` | 受保护路由和公开访问规则 |
| `audit.rs` | 登录及匿名访问审计，采集连接来源和请求证据，脱敏后持久化日志，仅登录事件发送通知 |

`POST /api/auth/login` 的 `method` 指定登录方式，其余字段交给对应 provider。当前方式为 `password`，字段为 `username`、`password`；省略 `method` 时兼容原有账号密码请求。未知方式直接拒绝，不会回退到其他方式。`GET /api/auth/status` 返回统一登录状态和 `methods` 列表。

新增登录方式时，实现 `LoginProvider` 并在 `AuthService::persistent` 中注册。认证成功只返回 `VerifiedIdentity`，由统一会话服务签发 token。身份包含 provider 内的用户标识和可用于撤销会话的版本信息；provider 必须校验自己的会话，不能接受其他方式签发的身份。凭据验证在 HTTP 层的 blocking 任务中执行，避免阻塞异步请求线程。

前端对应 `frontend/src/features/auth/`：新增登录表单并在 `loginMethods.ts` 注册相同的方式 ID，复用 `authApi.ts`、`LoginGate` 和 `AuthScreen`。页面只展示服务端已配置且客户端支持的方式。

当前授权策略要求登录后访问工作台 API、文件页和帮助页；健康检查、登录入口和精确匹配的只读分享接口由统一规则放行。新增公开认证入口时在本模块注册路由并明确更新公开规则，不能将整个 `/api/auth/` 前缀放行。

账户文件格式和 `aow account` 初始化命令保持兼容，已有安装无需重新设置密码。

登录的成功、凭据错误、JSON 拒绝、Origin 拒绝和限流均逐次审计并通知；受保护地址的拒绝访问，以及匿名打开非公开页面（包括未知地址）只记录审计日志，不推送通知。公开分享、健康检查、认证状态轮询和前端静态资源不记录这类访问审计。已进入凭据验证的请求即使客户端断开，也会完成结果审计，后台任务受登录并发限制约束。

审计通过 `OperationService::record` 等待有界日志队列和文件写入，不创建进度操作。登录事件的 IM 投递使用独立有界队列，向事件发生时配置的所有渠道发送，不受 Agent 通知开关影响；满队列施加背压而非去重或丢弃，各渠道的成功/失败通过同一事件 ID 写入操作历史。匿名访问不会进入通知队列。连接 IP 由 `ConnectInfo<SocketAddr>` 提供，嵌入者需要使用 `into_make_service_with_connect_info` 才能记录实际来源，否则记录为未知。代理头仅为未验证证据。

`POST /api/auth/logout` 撤销当前 Cookie 对应的服务端会话并清除同路径 Cookie。`SessionAccess` 将受保护的 WebSocket/SSE 绑定到其来源会话，连接检查不延长空闲期限，终端输入会重新验证凭据。过期会话在访问或创建会话时清理，连接每秒检查过期/凭据变化；达到 64 个会话时撤销最旧会话。HTTPS 配置、期限和限流参数见 [配置说明](../../../../docs/configuration.md#登录与-https)。
