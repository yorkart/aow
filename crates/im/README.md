# aow-im

独立的 IM 接入 crate，不依赖 AoW server、Agent 或自动化业务类型。

- `ImProvider::send(&Message, delivery_id)`：通用发送接口。
- 飞书：应用 token 缓存、当前 owner 查询、JSON 2.0 卡片渲染和分片。
- 微信：Tencent iLink 扫码、配对码、HTTP 直发、上下文更新及持久化。
- `ImConfig` 保存本机凭证，`ImConfigView` 只包含公开状态；不为凭证实现 `Debug`。

微信通知用空行分隔标题、Tab、会话、Session ID、链接和正文标题，避免客户端将单换行合并到同一段落。正文保留原始 Markdown 换行与代码块；整条消息仍限制长度并优先保留完整来源链接。

调用方负责持久化渠道配置和组装业务消息。`ImConfig::build(state_dir)` 创建客户端；`Provider::start()` 启动可选的微信接收轮询，`send()` 不依赖轮询。替换或移除渠道后调用 `Provider::retire()`，停止旧客户端并删除该绑定的上下文。客户端析构会停止轮询，保留有效会话以供下次启动。

微信 `LoginManager` 提供 `start`、`poll`、`cancel`。`poll` 接收同步提交回调；只有当前二维码有效且未被取消时才提交凭证。回调需要先成功保存凭证，再返回成功。长期凭证不出现在 `LoginView`。

微信只向扫码账号私聊投递。发送策略参考 [Hermes weixin.py](https://github.com/NousResearch/hermes-agent/blob/ac4181fdfaa4fa3b0682ea7b592c7a3ee6465e62/gateway/platforms/weixin.py)，扫码和请求格式依据 [Tencent 协议](https://github.com/Tencent/openclaw-weixin/blob/24de5c9eb0dd5e595d7e2d090ed8a3f82870d42c/docs/protocol_zh_CN.md)。协议细节和限制见 [接入调研](../../docs/wechat-bot-integration.md)，用户配置步骤见 [使用说明](../../docs/usage.md)。

运行模拟协议测试：`cargo test -p aow-im`。测试不使用真实 IM 账号。
