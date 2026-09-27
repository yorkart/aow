# 统一认证模块

业务路由只接入 `AuthService` 和 `require_auth`，不依赖具体的登录方式。

认证子路由由本模块定义，在 [routes.rs](../routes.rs) 的 `build_router` 中统一挂载；公共中间件和 Base Path 挂载也由该入口装配。

| 文件 | 职责 |
| --- | --- |
| `mod.rs` | 注册登录方式、分发认证请求、查询登录状态、统一校验会话 |
| `provider.rs` | `LoginProvider` 接口和认证后的身份契约 |
| `password.rs` | 本地账号文件读取、密码哈希校验、凭据变更后的会话失效 |
| `sessions.rs` | 生成会话 token、保存签发方式与身份、撤销会话 |
| `http.rs` | 登录及状态 API、Cookie 读写与 Base Path 作用域 |
| `authorization.rs` | 受保护路由和公开访问规则 |

`POST /api/auth/login` 的 `method` 指定登录方式，其余字段交给对应 provider。当前方式为 `password`，字段为 `username`、`password`；省略 `method` 时兼容原有账号密码请求。未知方式直接拒绝，不会回退到其他方式。`GET /api/auth/status` 返回统一登录状态和 `methods` 列表。

新增登录方式时，实现 `LoginProvider` 并在 `AuthService::persistent` 中注册。认证成功只返回 `VerifiedIdentity`，由统一会话服务签发 token。身份包含 provider 内的用户标识和可用于撤销会话的版本信息；provider 必须校验自己的会话，不能接受其他方式签发的身份。凭据验证在 HTTP 层的 blocking 任务中执行，避免阻塞异步请求线程。

前端对应 `frontend/src/features/auth/`：新增登录表单并在 `loginMethods.ts` 注册相同的方式 ID，复用 `authApi.ts`、`LoginGate` 和 `AuthScreen`。页面只展示服务端已配置且客户端支持的方式。

当前授权策略要求登录后访问工作台 API、文件页和帮助页；健康检查、登录入口和精确匹配的只读分享接口由统一规则放行。新增公开认证入口时在本模块注册路由并明确更新公开规则，不能将整个 `/api/auth/` 前缀放行。

账户文件格式和 `aow account` 初始化命令保持兼容，已有安装无需重新设置密码。
