# 微信 Bot 接入调研

调研日期：2026-09-25。用户已确认目标是普通微信（个人聊天或微信群）。本文建议首版验证 ClawBot 私聊推送，普通微信群列为待解决的能力缺口；企业微信资料仅作技术对照。协议调研已用于下述首版实现，并完成下文记录的单账号真实推送验证。

腾讯官方实现核对到 `Tencent/openclaw-weixin` 的 `24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c`（2026-09-21），包版本 `2.4.9`。下文区分官方源码行为、社区实测反馈和 AoW 的设计建议。

## 结论

普通微信已有可参考的官方实现：微信 ClawBot / iLink，通过扫码获取 Bot 凭证，以 HTTPS 长轮询接收消息，再调用发送接口。AoW 可以参考其协议，用现有 Rust HTTP 栈实现 `WechatClient`，用户无需额外运行 OpenClaw。

主动推送已有更贴近本需求的实现参考：Hermes Agent 内置微信适配器，支持保存扫码凭证后按需发送，并在特定会话错误下去掉 `context_token` 重试一次；上游有该方式恢复定时通知的实测反馈。[11][12][14] 因此可以把“扫码配置一次，任务完成后直接推送”作为 AoW 首版目标。官方协议尚未承诺上下文有效期、发送额度或无上下文发送的适用范围，降级后仍可能被拒，需用目标账号验证并显示具体失败状态。

| 路线 | 接收位置 | 配置与运行要求 | 对本需求的适用性 |
| --- | --- | --- | --- |
| 微信 ClawBot / iLink | 普通微信 Bot 私聊；当前官方插件只声明 `direct` | 扫码授权，服务端持续长轮询，保存会话上下文 | 符合普通微信目标，但主动推送能力必须验证 |
| 企业微信消息推送，原“群机器人” | 配置 Webhook 的企业微信群 | 保存 Webhook，事件发生时直接 POST | 适合群内事件通知，工程改动更小；接收位置不同 |
| 非官方个人号协议、Hook 或桌面自动化 | 取决于实现 | 通常依赖个人号登录状态或特定微信客户端 | 稳定性和维护成本不适合 AoW 默认渠道 |

当前官方插件的 `chatTypes: ["direct"]` 是明确的能力声明。协议类型中的 `group_id` 字段不能作为普通微信群已受支持的依据。[1][2]

推荐的首版范围是：一个微信账号扫码绑定，向绑定者的 ClawBot 私聊发送 Agent 任务完成和自动化失败通知。普通微信群暂不纳入已承诺能力；若后续要求必须发到普通微信群，需要另行验证可用接入路线，当前调研不能据此承诺支持。

## 普通微信如何接入

所有远程调用都由 AoW 后端发起。消息收取使用长轮询，因此无需为微信开放公网回调；通知中的 AoW 链接仍需手机能访问部署地址。

| 步骤 | 接口 | AoW 处理方式 |
| --- | --- | --- |
| 获取二维码 | `POST /ilink/bot/get_bot_qrcode?bot_type=3` | 从 `https://ilinkai.weixin.qq.com` 发起，请求可带空 `local_token_list`；将二维码展示到设置页 |
| 等待授权 | `GET /ilink/bot/get_qrcode_status?qrcode=...` | 处理扫码、验证码、过期、重定向和已绑定等状态 |
| 保存账号 | 授权结果 | 保存 `bot_token`、`ilink_bot_id`、`baseurl`、扫码者 `ilink_user_id`，凭证不回传前端 |
| 更新通知会话 | `POST /ilink/bot/getupdates` | 收到扫码者消息后核对发送者，保存 `context_token` 和轮询游标；测试发送若提示会话未就绪，再引导用户发一条消息 |
| 发送通知 | `POST /ilink/bot/sendmessage` | 使用目标用户 ID、最新上下文和事件发送标识，推送文本通知 |
| 维护运行状态 | 持续 `getupdates`；启动/停止通知接口 | 更新上下文和游标，处理断网、凭证失效、取消绑定与服务重启 |

二维码状态不能只实现成功/失败。当前源码包含 `wait`、`scaned`、`confirmed`、`expired`、`need_verifycode`、`verify_code_blocked`、`scaned_but_redirect`、`binded_redirect`。手机验证码应通过 AoW 设置页提交到后端；最后一种状态需核对已有本地凭证，不能当作返回了新 token。[1][3]

鉴权 Bot POST 请求使用 `AuthorizationType: ilink_bot_token` 和 `Authorization: Bearer <bot_token>`，还带 `X-WECHAT-UIN`、`iLink-App-Id`、`iLink-App-ClientVersion` 及 `base_info`。二维码请求的头部和鉴权规则不同，应按官方实现分别构造。协议版本信息集中维护，`bot_agent` 可声明为 `AoW/<version>`。[1]

文本消息的关键结构如下；这是协议结构示意，完整调用仍需上述请求头和真实授权上下文：

```json
{
  "msg": {
    "from_user_id": "",
    "to_user_id": "<扫码者的 ilink_user_id>",
    "client_id": "<本条通知的唯一 ID>",
    "message_type": 2,
    "message_state": 2,
    "context_token": "<该用户最近入站消息的上下文>",
    "item_list": [
      { "type": 1, "text_item": { "text": "项目 · Agent · 任务完成\n本轮结论摘要\nhttps://aow.example.com/aow/tabs/terminal/<tab-id>" } }
    ]
  },
  "base_info": {
    "channel_version": "<已验证的协议兼容版本>",
    "bot_agent": "AoW/<version>"
  }
}
```

`bot_token` 是账号凭证，`context_token` 是消息上下文，两者独立。持续空轮询、保存 token 到磁盘、设置“正在输入”都不能被假定为延长上下文有效期。当前官方代码已经支持上下文落盘，因此部分旧 issue 中“只保存在内存”的描述不适用于本次核对的版本。[4]

## 主动通知的边界

| 证据 | 可以得出的结论 |
| --- | --- |
| 官方发送代码缺少上下文时会记录警告并继续请求；协议明确指出这不能证明服务端接受 | 结合 Hermes 的降级策略验证无上下文发送，保留会话未就绪等失败状态 |
| Issue #81、#202 反馈连续发送约 10 条受限、无交互约 24 小时失效 | 提示存在额度/会话限制，但这些数字不是正式服务端契约 |
| Issue #225、#286、#309 报告的失效时间各异；#309 有 2.4.9 下定时通知失败的反馈 | 不应把 24 小时或任何固定时间写死为可靠窗口，需在目标账号实测 |
| Issue #264 等报告 `ret: 0` 但手机未显示 | API 受理与实际到达应区分；首次接入须以微信端实际收到为准 |

`ret=-2` 的公开反馈涉及过期、次数和限流等情况，不能统一解释成某一种错误，更不能假定重试可以刷新上下文。Hermes 会结合 `errmsg`，对 `prepare failed` / `unknown error` 等特定响应尝试一次不带上下文的发送；这是改变请求后的降级，并非刷新上下文。如果仍失败，才提示用户发消息初始化会话或重新绑定。`-14` 在腾讯官方收消息实现中作为账号会话失效处理，并暂停请求一小时；这是客户端退避策略，AoW 可提供明确的重新授权入口。[1][5][6][7][8][9][12]

首版通知建议采用“项目、会话、结果摘要、详情链接”的单条纯文本。官方插件配置 `textChunkLimit: 4000`，这是客户端分片设置，不是本次已确认的服务端字节上限。AoW 应使用保守长度并测试中文、emoji 与长 URL；超长结论通过详情页查看，可减少发送次数。当前飞书的完整 Markdown 卡片分片策略不宜直接套用。[2]

发送继续使用稳定的 `client_id` 关联事件，但不能仅凭该字段就承诺服务端幂等。网络超时后沿用 AoW 对不确定投递不自动重发的策略；上下文受限时记录状态，在设置页提示用户向 Bot 发消息恢复。

## AoW 首版实现

按要求新建独立 `aow-im` crate。它不依赖 server、Agent 或自动化 crate，接收通用 `Message`（标题、字段/链接、正文、格式及错误级别），承载协议、鉴权、卡片/文本渲染、扫码和会话生命周期。

| 位置 | 实现职责 |
| --- | --- |
| `crates/im/src/lib.rs`、`message.rs` | `ImProvider::send`、渠道配置及脱敏视图、通用消息模型 |
| `crates/im/src/feishu/`（入口 `mod.rs`） | 从原 `crates/server/src/im` 迁入的飞书鉴权、owner 查询、限流和卡片分片 |
| `crates/im/src/wechat/` | iLink HTTP、扫码/配对码/重定向、上下文与游标持久化、独立长轮询、单次直发和 Hermes 降级 |
| `crates/server/src/im_api.rs` | 现有 PIN 认证下的配置、扫码、状态、测试消息及解除本机绑定 API |
| `crates/server/src/notifications/` | 业务消息组装、配置落盘、页面/飞书/微信渠道及通知队列 |
| `frontend/src/features/im/` | 独立 IM 设置 UI，分别操作飞书和微信 |
| `crates/server/src/automations/notifications.rs` | 保存执行快照中的渠道选择，再交给通知层投递 |
| `frontend/src/features/automations/` | 飞书/微信失败提醒选择，桌面与手机详情显示 |

凭证继续使用原有本机 `notification-settings.json`，兼容现有飞书配置；上下文与游标放入 `im/wechat/<binding-id>.json`，使用原子写入和 `0600` 权限。轮询更新不修改通知偏好的 revision。替换/移除微信会停止旧轮询、使旧客户端失效并清理上下文。飞书改为按 provider 更新/删除，旧版整组配置 API 也会保留已绑定的微信。

扫码状态在内存中保存五分钟。取消/刷新后，即使旧状态请求稍后返回成功，也不能提交旧凭证。验证码通过 AoW 的 POST JSON 提交，不放入 AoW URL。二维码由本机生成 SVG，不向第三方图片服务暴露二维码内容。Tencent 返回的 API 地址仅允许 HTTPS、默认端口和 `weixin.qq.com` 下的主机；禁用 HTTP 自动跳转。

用户流程：Settings → IM → 微信 Bot → 扫码并按需输入手机验证码 → 在微信中向 Bot 发一条消息 → 页面检测到会话后发送测试通知 → 用户确认微信实际收到 → 在通知设置中选择微信。页面区分身份绑定、收到用户消息、测试请求提交和用户确认收件；发送请求成功不会自动标记验证完成。接收对象固定为扫码者；其他用户、群消息和 Bot 消息不能更新其上下文。自动化失败提醒可以独立选择微信，不依赖 Agent 任务完成开关。

发送参考 Hermes：有上下文则携带；没有上下文也可尝试直发。仅对明确的 `-14`，或 `-2` 且 `errmsg` 为 `prepare failed` / `unknown error` 的业务拒绝，去掉上下文并使用相同 `client_id` 重试一次。HTTP 错误、网络超时、未知业务错误不重试。首版只发送一条有长度上限的文本；没有收消息轮询也可发送，不依赖某一条永不断开的连接。

自动化测试使用本机模拟接口和浏览器 mock。2026-09-25 的单账号实测中，已扫码但没有消息上下文时，直接发送返回 `ret=-2`、`errmsg=prepare failed`；用户向 Bot 发消息后，运行中的 AoW 自动保存上下文，使用同一绑定身份发送成功，用户确认手机收到测试通知。这验证了该账号的首次激活和文本投递，未验证长期主动发送窗口或额度。生命周期通知 `notifystart` / `notifystop` 和媒体收发未在首版实现。

## 官方 SDK 核对

2026-09-25 补充核对了腾讯官方仓库、入口代码、README、npm 已发布包，并检索了 GitHub 和 crates.io。当前可确认的官方交付物是 OpenClaw 微信渠道插件；本次未找到面向任意应用独立集成的官方普通微信 Bot SDK，也未找到官方 Rust SDK。

| 交付物 | 本次核对版本 | 定位与接入要求 |
| --- | --- | --- |
| [`@tencent-weixin/openclaw-weixin`](https://www.npmjs.com/package/@tencent-weixin/openclaw-weixin) | `2.4.9` | 腾讯官方 TypeScript 插件，包含扫码、凭证保存、长轮询和消息收发；要求 Node.js `>=22.13.0`，npm peer dependency 为 OpenClaw `>=2026.5.12` |
| [`@tencent-weixin/openclaw-weixin-cli`](https://www.npmjs.com/package/@tencent-weixin/openclaw-weixin-cli) | `2.1.4` | 官方插件安装器；包描述为 `Lightweight installer for the OpenClaw Weixin channel plugin`，不是独立的登录/发消息 SDK |
| [`wechatbot`](https://crates.io/crates/wechatbot) | `0.4.0` | 可供 Rust 集成的社区 SDK，仓库为 `corespeed-io/wechatbot`，不属于腾讯官方 SDK |

插件的 [`index.ts`](https://github.com/Tencent/openclaw-weixin/blob/24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c/index.ts) 导出 OpenClaw 插件对象，通过 `register(api)` 注册 channel，并检查宿主兼容性。[`package.json`](https://github.com/Tencent/openclaw-weixin/blob/24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c/package.json) 声明 OpenClaw 宿主依赖，没有提供通用 SDK 的包入口。官方 README 也要求先安装 OpenClaw。

因此，原样复用官方插件的路线是运行 OpenClaw Gateway 并由 AoW 对接该服务；如果要保持 AoW 原生 Rust 后端，则仍需实现 HTTP 协议适配，或评估社区 Rust SDK。官方仓库采用 MIT 许可证，可以参考或抽取实现，但自行抽取后需要维护适配层。使用官方插件也不能自动解除前文提到的服务端主动发送限制。

## Hermes Agent 的实现参考

已核对 NousResearch 官方主仓库 `NousResearch/hermes-agent` 的 `ac4181fdfaa4fa3b0682ea7b592c7a3ee6465e62`（2026-09-25）。Hermes 自带普通微信适配器 `gateway/platforms/weixin.py`，用 Python `aiohttp` 直接请求腾讯 iLink API；二维码展示使用 `qrcode`，媒体加解密使用 `cryptography`，该渠道不依赖腾讯 OpenClaw 插件或微信专用 SDK。[11][12]

| 环节 | Hermes 的做法 | 对 AoW 的启发 |
| --- | --- | --- |
| 扫码授权 | `hermes gateway setup` 选择 Weixin；`qr_login()` 获取二维码、等待手机确认 | 设置页可承接同样的扫码流程 |
| 保存登录 | `save_weixin_account()` 将账号凭证保存到 `~/.hermes/weixin/accounts/<account_id>.json`，权限 `0600` | 登录与 HTTP 连接分开，重启可读取有效凭证 |
| 收取消息 | `WeixinAdapter.connect()` 启动 `getupdates` 长轮询，默认超时 35 秒；失败退避、恢复游标 | 接收任务负责更新上下文，不阻塞通知发送 |
| 发送消息 | `_send_message()` / `_send_items()` 调用 `sendmessage`，按接收人附带已保存的上下文 | 用普通 HTTP 请求完成一次通知投递 |
| 主动通知 | `send_weixin_direct()` 供 `send_message` / cron 使用；复用同事件循环内的在线适配器，或临时创建 HTTP 客户端及适配器后发送 | 一次推送不要求先启动或维持收消息的长轮询 |
| 失效处理 | 特定错误下去掉上下文重试一次；仍失败则报告会话未就绪；其他限流错误走冷却处理 | 区分会话拒绝、频率限制和网络结果不确定 |

发送降级发生在 `_send_text_chunk()`：先使用缓存的 `context_token`，若返回 `-14`，或 `-2` 且错误文本为 `prepare failed` / `unknown error`，就移除上下文并用同一个 `client_id` 再试一次。`bot_token` 仍然保留用于鉴权。若无上下文重试仍得到会话未就绪错误，则停止这条恢复路径并提示用户先发消息或重新绑定。媒体发送也有对应处理。[12]

这纠正了“上下文失效后一定要等待用户再发消息”的过强判断：Hermes 已实现可尝试的降级路径。[Issue #112709](https://github.com/NousResearch/hermes-agent/issues/112709) 报告移除失效上下文后普通发送与 cron 端到端恢复；当前源码也有覆盖成功与继续失败两条分支的 mock 回归测试。[13][14] 这些证据支持参考其实现，但不是对所有账号、所有服务端策略的送达保证；AoW 的本次单账号实测仍需要用户先发消息建立上下文。

源码与文档的细节需要交叉核对。例如当前 Hermes 代码获取二维码仍用 GET，并固定声明 `CHANNEL_VERSION = "2.2.0"`；本次核对的腾讯 `2.4.9` 则使用 POST 获取二维码并处理手机验证码等新状态。AoW 可借鉴 Hermes 的持久化、单次发送和降级设计，登录协议仍以腾讯当前实现为基线，避免整份照搬。

Hermes 虽提供群策略配置，但官方文档明确说明扫码得到的是 iLink Bot 身份，通常不能加入普通微信群，普通群事件也通常不会下发；仅打开群配置无法解决该限制。[11] 社区项目 HermesClaw 用于让多个 Agent 框架共用一个账号，Hermes 原生微信渠道无需该中转。

## 选择实现方式

| 方式 | 取舍 |
| --- | --- |
| **AoW 原生 Rust HTTP，建议** | 已有 `reqwest`、`tokio` 和凭证保存模式；协议参照腾讯官方源码，扫码凭证持久化、单次发送和会话错误降级参考 Hermes |
| 运行 OpenClaw + 官方插件作为中转 | 可复用其登录和收发实现，但新增 Node/OpenClaw 服务、进程与配置依赖；上游推送限制仍然存在 |
| 社区 Rust SDK，如 `wechatbot` | 可减少基础协议代码，但不是腾讯官方 Rust SDK；需单独核对最新扫码状态、上下文持久化、错误处理和依赖，不能直接依据 README 的成熟度声明选型 |

建议先做一个窄范围验证：一个账号、一个接收人、纯文本通知。确认主动通知限制可接受后，再完成设置页、持久化和两类事件接入。

## 企业微信技术对照

企业微信 Webhook 投递到配置它的企业微信群，无法直接满足本次普通微信目标。以下保留为渠道架构的技术对照。企业微信官方现称“消息推送（原群机器人）”，配置群的 Webhook 后发送：

```http
POST https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=<WEBHOOK_KEY>
Content-Type: application/json
```

```json
{
  "msgtype": "markdown",
  "markdown": { "content": "**Agent 任务完成**\n项目：AoW\n[查看详情](https://aow.example.com/aow/tabs/terminal/<tab-id>)" }
}
```

官方限制：每个消息推送不超过 **20 条/分钟**；文本最长 **2048 UTF-8 字节**，Markdown/Markdown v2 最长 **4096 UTF-8 字节**。需要消息格式化、队列节流和业务错误码处理，但无需扫码会话与入站长轮询。Webhook key 作为密钥保存和脱敏。[10]

## 实施前应验证的结果

| 验证 | 目的 |
| --- | --- |
| 首次扫码、验证码、二维码过期、重复扫码 | 验证完整登录流程及目标账号可用性 |
| 首次消息后发送任务完成/失败通知 | 验证接收者、正文、手机端实际到达和 AoW 链接 |
| 无新入站消息，间隔 2 分钟、1 小时、24/48 小时发送 | 实测异步长任务与无人值守通知边界；时间点是测试样本，不是承诺窗口 |
| 带失效上下文发送、命中特定错误后去掉上下文重试一次 | 验证 Hermes 式降级能否在目标账号恢复，区分成功与持续会话未就绪 |
| 服务端保存有效凭证后，仅调用单次发送路径 | 验证通知发送不依赖正在运行的收消息长轮询；单独记录缺少上下文时的服务端结果 |
| 持有同一上下文发送 10 条以上，以及长结论 | 验证额度、频率和正文长度限制，记录真实业务返回码 |
| 服务重启、断网恢复、重新绑定和解除连接 | 验证账号/上下文/游标持久化及旧后台任务停止 |
| 飞书与微信并存，独立编辑/删除配置 | 避免互相覆盖，保证页面与原有渠道仍正常 |
| 自动化任务修改后读取旧执行记录 | 保证通知渠道遵循执行时快照 |

协议错误、扫码状态和生命周期可通过 mock HTTP 验证；主动发送窗口、账号限制和微信端实际到达必须由真实账号验证。若长时间无交互时稳定发送是硬性要求，当前公开材料不足以确认普通微信路线满足要求。

## 资料

- [1] [腾讯官方后端协议](https://github.com/Tencent/openclaw-weixin/blob/24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c/docs/protocol_zh_CN.md)：接口、鉴权和客户端行为边界。
- [2] [腾讯官方渠道实现](https://github.com/Tencent/openclaw-weixin/blob/24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c/src/channel.ts)：私聊声明、出站发送、4000 分片设置及生命周期。
- [3] [腾讯官方二维码登录实现](https://github.com/Tencent/openclaw-weixin/blob/24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c/src/auth/login-qr.ts)。
- [4] [腾讯官方上下文保存实现](https://github.com/Tencent/openclaw-weixin/blob/24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c/src/messaging/inbound.ts)。
- [5] [Issue #81：连续回复次数反馈](https://github.com/Tencent/openclaw-weixin/issues/81)。
- [6] [Issue #202：主动消息与会话时效反馈](https://github.com/Tencent/openclaw-weixin/issues/202)。
- [7] [Issue #225](https://github.com/Tencent/openclaw-weixin/issues/225)、[Issue #286](https://github.com/Tencent/openclaw-weixin/issues/286)：不同环境下上下文失效反馈，属于用户观测。
- [8] [Issue #309：定时推送与最新版本反馈](https://github.com/Tencent/openclaw-weixin/issues/309)。
- [9] [Issue #264：API 成功但客户端不可见反馈](https://github.com/Tencent/openclaw-weixin/issues/264)。
- [10] [企业微信官方消息推送配置说明](https://developer.work.weixin.qq.com/document/path/91770)。
- [11] [Hermes 官方普通微信接入文档](https://github.com/NousResearch/hermes-agent/blob/ac4181fdfaa4fa3b0682ea7b592c7a3ee6465e62/website/docs/user-guide/messaging/weixin.md)。
- [12] [Hermes 微信适配器源码](https://github.com/NousResearch/hermes-agent/blob/ac4181fdfaa4fa3b0682ea7b592c7a3ee6465e62/gateway/platforms/weixin.py)：`qr_login`、`ContextTokenStore`、`_send_text_chunk`、`send_weixin_direct`。
- [13] [Hermes 微信发送回归测试](https://github.com/NousResearch/hermes-agent/blob/ac4181fdfaa4fa3b0682ea7b592c7a3ee6465e62/tests/gateway/test_weixin.py)：上下文降级成功、失败和媒体错误处理；属于 mock 测试。
- [14] [Hermes Issue #112709](https://github.com/NousResearch/hermes-agent/issues/112709)：用户报告去掉失效上下文后恢复主动发送与 cron 投递。
- 其他参考：[官方项目 README](https://github.com/Tencent/openclaw-weixin)、[社区多语言 SDK](https://github.com/corespeed-io/wechatbot)。
