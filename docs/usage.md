# 使用说明

安装与访问入口见 [README](../README.md)，服务设置与数据目录见 [配置说明](configuration.md)，本机命令见 [CLI 说明](aow-cli.md)。

使用 Agent 功能前，请在运行 AOW 的机器上安装对应 CLI 并登录账号。

## 项目与 Pull Requests

首页会进入 Project AOW。注册本机 Git 仓库后，可在页面中管理关联 Worktree、Notes、文件、Git Diff、Terminal，以及本地 Agent 会话。右侧 **Pull Requests** 面板通过配置的 Provider 展示当前账号创建的 Open PR，详情按「概览、文件变更、检查、讨论」组织。Settings → Pull Requests 可管理域名映射和 Python 脚本：脚本仅支持只读预览或上传替换，保存后由 AOW 管理脚本文件。

首次初始化内置 `github.com → github` 及适配脚本，脚本内部调用 gh，需要服务器可运行 python3、gh 并完成 gh 登录。其他平台通过用户脚本实现统一 API，脚本自行决定 CLI 或 API 的调用方式。多个 remote 对应不同仓库时可在列表选择；标签页、深链接和手机详情保留 Provider 与 remote 身份。默认配置只初始化一次，不覆盖用户修改或重新添加已删除的 Provider。完整 API、字段及存储规则见 [Review Provider 脚本协议](../frontend/src/features/pr/review-providers.md)，设置页中也可查看。

## 多节点访问

在 **Settings → Nodes** 中可配置其他机器的 AOW 地址，每行填写一个完整的 HTTP(S) 地址，保存后点击桌面或手机端首页左上角的 Logo / AOW 文字即可展开菜单；点击节点地址会在新窗口或新标签页打开，保留当前页面。列表允许包含当前节点，可以将同一份配置复制到所有机器；菜单按协议、域名/IP、端口和部署路径过滤当前节点，不同端口或不同 Base Path 仍作为独立节点显示。建议填写部署入口地址，例如 `https://example.com/tools/aow/`；当前部署的 `/aow/`、`/m/` 入口也会被识别为自身。空行和重复地址会自动整理，留空保存可清空列表。配置保存在服务端 `aow-settings.json` 的 `node_addresses` 字段中。

## 工作区与终端

桌面右下角的**浮动工作区**按钮打开全局工作区，支持拖动、调整大小、最大化和最小化；窗口位置、打开的 Tab 和选中项在刷新后恢复。点击窗口中的 `+` 可新建 Markdown、Terminal 或打开系统文件浏览器。项目中的文件、Diff、会话、PR、自动化任务和新建入口支持右键选择「打开 · 浮动工作区」，已有 Tab 也支持此选项。打开位置变化后，文件路径、所属项目和终端工作目录保持原来的上下文；已有终端复用同一个进程。文件编辑和保存状态在工作区之间共享。

主仓库的 **Terminals** 面板可分别为 User Terminals 和 CLI Terminals 开启「显示所有Worktree」。点击其他 Worktree 的终端会直接在主仓库打开 Tab，保留原终端的工作目录和进程，当前 Worktree 不切换。原 Worktree 保留占位 Tab，点击「在当前工作区显示」或主仓库 Tab 右键菜单的「还原」即可收回显示。关闭 Tab 会同时清理两处展示，后台终端继续运行；主仓库中的打开位置在刷新后恢复，也可通过列表右键菜单移到浮动工作区。

项目工作区和浮动工作区共用可折叠的 Tab 分组，按 Terminal、文件、Diff 等类型自动归组。每组始终保留一个类型图标，以连续底线连接组内 Tab；点击图标或按 Enter / Space 可收起、展开。收起时底线缩短至图标宽度，在图标右下角显示 1～9，超过 9 显示 `…`，悬停查看完整数量。折叠只隐藏 Tab 标题，当前内容、终端进程和自动保存继续保留。各 Worktree 与浮动工作区分别记住折叠状态，刷新后恢复；主动打开资源会展开对应分组。

浮动窗口右上角的 Pin 默认开启，保持窗口固定显示。取消固定后，鼠标离开窗口约 200ms 会自动隐藏，返回窗口原来的范围时自动显示；编辑内容、终端进程和自动保存继续保留。菜单、对话框和拖动操作期间暂停自动隐藏。Pin 状态会记住，拖动、缩放后的窗口范围也随之更新；主动最小化后通过浮动按钮或打开入口重新显示，鼠标经过原区域不会唤出。

浮动工作区的内置项目 ID 和 Git 仓库目录名均为 `__aow_floating`，安装时初始化，已有安装在服务启动时补建。它不显示在 Projects 和 Pinned 中，使用普通项目的 Notes、终端和 Git 逻辑。默认仓库为 `~/.local/state/aow/repos/__aow_floating/`，Notes 为 `<notes-root>/localhost/<服务运行用户名>/__aow_floating/`。自定义 state dir 时，仓库位于相应的 `repos/__aow_floating/`；修改 Notes root 不会移动 Git 仓库。旧 `repos/__global__/` 目录不自动迁移。

## 文件浏览与上传

系统文件浏览器只占一个 Tab，重复打开会选中已有 Tab。首次进入服务运行用户的 Home，采用三栏布局：左栏为 `/`、`/tmp`、`Home` 快捷导航及收藏 / Pin 目录，中栏为目录树，右栏为当前目录的文件列表。文本文件旁的「打开」按钮在浮动工作区中打开文件，可按文件权限编辑、自动保存到原路径。当前目录、目录树和文件列表中的文件夹均支持 Pin / 取消 Pin；点击收藏会定位并展开目录树。收藏保存在服务端 `aow-settings.json` 中，刷新页面后仍可使用，已失效的收藏也可以取消。可拖入本地文件或使用上传按钮上传到当前目录，下载文件，以及复制文件或文件夹的完整路径。最小化浮动工作区或切换 Tab 会保留当前目录、展开状态、选中项、滚动位置和上传进度，上传继续进行。左下角保留 Settings 齿轮入口。

Project Explorer 和 Notes Explorer 支持右键“粘贴”上传剪贴板图片，也支持文件树获得焦点后按 Ctrl/Cmd+V 上传浏览器提供的文件或截图。右键目录上传到该目录，右键文件上传到其所在目录，空白区域上传到 Explorer 根目录。截图自动命名；同名文件在服务端追加序号，保留已有文件。上传队列显示进度、失败原因，并支持取消和重试。浏览器无法直接读取剪贴板时，会提示使用快捷键或选择文件；普通文件的菜单读取能力取决于浏览器，暂不支持整目录粘贴。切换 Explorer 根目录或关闭对应工作区时会取消尚未完成的上传。

## Terminal Agent 图标

Linux 和 macOS 上的普通 Terminal 会自动识别其中运行的 Codex、Claude Code、TraeCode CLI（`traecli`）和 Hermes，桌面和手机标签页会显示对应 Agent 图标，退出后恢复终端图标。检测约每 1.5 秒刷新，通过读取本机进程信息实现，无需安装 hook 或修改 Agent 配置。分割标签页取首个检测到 Agent 的窗格，手机端按窗格分别显示。

检测范围是 terminald 所在主机、同一 PTY 的进程；终端中再进入 SSH、容器或 tmux 的内部会话暂不识别。Linux 使用 `/proc`，macOS 使用原生进程接口，两者共用 Agent 匹配和前台进程选择规则。

桌面分割窗格识别到上述 Agent 后，标题栏提供 **Conversation** 按钮，在当前窗格打开会话详情，点击 **Terminal** 返回；切换会保留终端进程和连接。Claude 优先通过 `claude agents --json` 按前台进程 PID 获取完整 `sessionId`，命令不支持或查询失败时，和 Codex、TraeCode CLI 一样按工作目录和原始会话标题筛选历史记录。完整标题唯一匹配时自动打开；重名、截断标题或没有匹配时，可搜索标题或 ID 并手动选择。详情中的「重新选择」可更正关联，手动选择在当前浏览器会话中记忆，Agent 进程或会话标题变化后重新识别。

该功能不安装 hook，也不更改 Agent 的终端标题配置或用户自定义窗格名称。标题匹配属于推断；Claude 新会话尚未写入记录时可稍后刷新。详情可见时每 10 秒刷新，隐藏后暂停查询。前台 PID、实际工作目录和进程身份由更新后的 Linux/macOS terminald 提供；旧版 daemon 仍可手动选择会话。会话关联读取 Agent 自身的配置目录，并在读取前后校验 PID 和启动时间；进程退出、身份变化或环境不可读取时不使用服务端配置代替。macOS 对系统保护的进程可能省略环境变量，此时自动关联和新提醒绑定不可用。

## Agent 任务停止提醒

后端自动探测 Terminal 实例中的 Agent，并在任务停止时向已登录的桌面和手机网页推送通知卡片。卡片左侧显示 Agent Logo，以项目名为标题、Terminal Tab 名称为第二行，并显示时间和关闭按钮；不展示会话标题、Session ID，不抢占当前输入焦点。同一 session 被多个 Tab 监听时合并展示来源项目和 Tab 名称，同一个 Tab 的多个窗格只显示一次来源。项目名与 Tab 名称在发送时读取，未注册项目的目录使用目录名。切换页面或最小化 Terminal 不会取消监听；关闭网页期间的页面提醒不补发。默认启用页面提示，可在 **Settings → 通知 → Agent 任务完成** 中关闭或选择通知渠道。

- Codex / TraeCode CLI：捕获有效的原始终端标题后，按 Agent、工作目录和标题匹配最近更新的最多 5 个会话；候选一旦确定就固定。标题、Agent 或已知进程实例变化时替换绑定，相同绑定的普通轮询不会重算候选。新版 terminald 提供进程信息时使用实际 cwd 和 Agent 进程环境；旧版只有 Agent 和 title 时，使用窗格保存的 cwd 与 Web server 的会话目录配置，也能注册提醒。
- Claude：通过 `claude agents --json` 按前台 PID 定位 session ID，不用标题猜测；依赖支持该命令且 transcript 写入 `system/turn_duration` 轮次结束记录的版本。命令暂时失败时保留已有绑定，不降级到文本回复推断。
- 每个 shell 实例只有一个绑定，实例关闭会取消订阅；同一个 session 的日志读取由多个实例共用。新加入监听的文件从 EOF 开始，重新加入也从当前 EOF 开始，不补读历史。
- 全局最多监听 50 个不同 session，超限时淘汰 rollout 最久未变化的监听，不停止 Agent。淘汰后等下一次注册才重新尝试，不自动补回；长时间没有日志变化的运行中任务也可能被淘汰。
- Codex / TraeCode CLI 仅用 `task_complete` / `turn_complete` 触发提醒，Claude 用 `turn_duration`；工具完成、授权等待和普通 assistant 消息不触发。每条新增结束记录产生一次通知，不按 title 或 session 合并不同任务。
- 完成事件发送前从已绑定会话的原生存储读取最新会话名，不依赖 OSC 是否再次更新；名称暂时不可读时沿用上次已知名称。名称刷新不改变锁定的候选会话。重复注册复用监听器时也同步名称，并保留原有 rollout 读取位置。

**Settings → IM** 中可配置飞书企业自建应用机器人，只需填写 App ID 和 App Secret。飞书开发者后台需开启并发布机器人能力，开通 `application:application:self_manage`（管理应用自身资源）和 `im:message:send_as_bot`（以应用的身份发消息），并确保应用所有者在可用范围内。后端自动查询当前应用 owner，无需配置接收人。密钥保存后不回传网页，编辑时留空保留；更换 App ID 时需重新填写密钥。

配置机器人后，通知渠道中会出现“飞书推送”，可与“页面提示”多选，启用通知时至少选择一项。保存立即生效；移除机器人前需先取消选择飞书推送。飞书推送由服务端处理，网页关闭后仍会发送。IM 和通知设置保存在本机状态目录的 `notification-settings.json`，文件权限为 `0600`，不进入 Git 配置仓库；重启后自动恢复设置。文件包含明文应用密钥，请按凭据管理。

工作台所有内容 Tab 使用分类 URL：Terminal/Agent、文件浏览器、文件、工作区/暂存区/Commit Diff、PR、Conversation Session、自动化任务和执行记录均支持直接打开、登录后定位和刷新恢复。打开、切换或关闭 Tab 时，地址栏通过 `replaceState` 跟随当前活动内容；桌面浮动工作区和移动端使用相同资源定位规则。

终端 Tab 链接使用 `/aow/tabs/terminal/<tab-id>`；旧 `/aow/tabs/<tab-id>` 仍可访问。页面通知通过点击整张卡片（也支持 Enter / 空格）在当前页面激活对应 Tab，成功后永久移除；多个来源时打开首个可用 Tab。跳转失败时保留卡片、显示错误并提供“移除通知”，不创建新资源。提醒一直挂在右侧，默认展开最近 5 条，其余折叠为带数量的堆叠；点击堆叠只展开可滚动列表，不跳转或删除。展开后点击具体卡片即可跳转，处理通知时列表保持展开，不新增内容 Tab 或入口按钮。队列不设条数上限、不自动过期，通过当前浏览器的 IndexedDB 保存，刷新后恢复。点击卡片右上角关闭按钮或“全部关闭”只隐藏弹框，保留未读状态；关闭状态也会保存，刷新后不会再次弹出，同一 Tab 后续产生的新提醒仍正常弹出。移除失效通知或激活任一来源 Tab 会永久删除对应提醒，当前已激活 Tab 的新提醒不再加入队列；网页关闭期间的通知不补录，不跨设备同步。PR 路由按 Settings 中配置的 Provider 动态解析，并携带 remote。

桌面左侧 Projects 和 Pinned 列表默认在对应 Worktree 条目的最右侧显示未读通知数量，数量为 0 时隐藏。角标与页面通知共用同一份持久化队列：新提醒计入对应 Worktree，关闭单个或全部弹框不扣减未读数量；激活来源 Tab 或移除失效通知时同步扣减，刷新后恢复。同一条通知在同一个 Worktree 中只计一次，即使它关联多个 Tab；没有来源 Worktree 的旧版通知不计入角标。

飞书消息由后台发送，需要在 **Settings → 通知 → AOW 访问地址** 设置接收设备可访问的 HTTP(S) 地址，例如 `https://aow.example.com`；也可点击“使用当前访问地址”填入浏览器的站点地址。配置保存在本机 `notification-settings.json` 的 `notifications.public_base_url`，网页保存立即生效。没有配置时，飞书继续显示普通 Tab 文本；配置后为每个来源 Tab 生成独立链接，长结论的每片卡片都会保留这些链接。

也可以在已有配置文件的 `notifications` 对象中添加 `"public_base_url": "https://aow.example.com"`，保留其他字段和 IM 凭据；手动编辑文件后重启 Web server 生效。旧配置缺少该字段时按空地址处理，无需迁移。访问地址不是飞书回调接口，链接仍走原有 PIN 登录。

结束事件携带当前轮次的 `conclusion`（最终回复），由各 Agent 的 `TaskStopParser` 实现提取并在结束边界固定。Codex / TraeCode CLI 优先使用结束记录的 `last_agent_message`，否则取当前轮已捕获的最终回复；Claude 收集同一 assistant 消息的文本块，在 `turn_duration` 时交付。工具输出和思考内容不作为结论；监听前的历史不回读，缺失结论时传 `null`，不借用上一轮内容。页面提示忽略结论。

飞书使用 JSON 2.0 卡片，标题为 `<project>·<会话>·<agent>·完成`，会话使用当前会话标题，Agent 使用实际执行者的显示名称；多个来源项目去重后用顿号连接。正文顶部展示 Tab、会话标题和完整 Session ID，下方展示 Markdown 格式的“本轮结论”，不再重复列出 Agent、项目或工作目录。超长结论保留全文，按完整 HTTP JSON 请求序列化后的 UTF-8 字节数分片，每张最多 28,000 字节，为飞书 30 KB 限制预留余量；每张重复顶部信息并在标题后标记 `1/N`，按顺序发送。优先在段落/换行处拆分，跨片的围栏代码块会闭合后重新打开。每片具有独立发送标识，重试只复用当前片的标识。没有结论时显示“未捕获到本轮结论”。

飞书通过 Rust HTTP 客户端发送，应用 token 只缓存在内存、临近过期时按需重新获取；更换凭据会替换客户端和缓存。每条任务事件有独立发送标识，token 失效或限流重试时复用该标识。网络超时不自动重发，避免不确定的重复投递。发送队列最多保存 64 条待发通知，发送失败或队列满时记录服务端日志；不持久化待发事件，服务重启后不补发。IM 实现通过 `ImProvider` trait 接入，与 Agent 会话监听独立。

监听注册和网页事件传输仅存于内存，服务重启后重新探测并从 EOF 开始。后端每约 1.5 秒轮询，Claude PID 查询沿用 5 秒成功缓存。事件通过受 PIN 保护的 `/api/terminals/task-stops` SSE 接口发送，不开放给匿名会话分享页。

验证：`cargo test -p aow-agents --features sessions sessions::tail`、`cargo test -p aow-server terminal::notifications`；在 `frontend/` 下执行 `npm run test:notifications` 和 `npm run test:tab-links`。

## Conversation

TraeCode CLI 适配面向 **2.0**。

会话详情按轮次展示用户输入、Agent 处理过程和最终结论。处理过程包含公开进度说明、工具名称和执行状态，工具详情可展开查看已记录的命令、参数和执行输出，不展示内部推理。支持 Codex、TraeCode CLI、Claude 和 Hermes 的本地会话记录。

连续工具调用默认合并为英文概要，例如 `Read files, edited files, ran commands`；结合工具名称和会话记录中的命令分类去重汇总，保留调用次数及失败/执行中状态。点击概要可展开工具列表，Agent 的进度说明会分隔前后两组调用。

悬停在时间线左侧的气泡或扳手图标上，可查看对应记录的本地时间；工具组显示组内记录的时间范围，展开后每次调用也有独立的时间提示。没有时间的记录显示“时间未记录”，部分缺失的工具组会附带说明。

最新一轮默认展开处理过程，历史轮次默认收起，可手动展开查看；刷新同一会话会保留手动展开状态，新一轮出现时上一轮自动收起。桌面端可通过对话导航或「最新一轮」跳转，手机端使用相同的折叠规则。过长的过程仅保留每轮最近 500 条摘要，并显示截断提示。

会话详情顶部的「分享」可创建并复制动态只读链接，桌面、Terminal 内会话详情和手机均可操作。同一会话复用有效链接；「取消分享」会立即停止服务端读取，再次创建会生成新链接。创建、查看分享设置和取消分享仍需要原有 PIN 登录；访客只需持有链接，无需 PIN，分享 token 不会产生登录 Cookie，也不能用于访问文件、Terminal 或其他工作台接口。

分享页展示当前会话及后续对话，包括处理过程和工具详情，沿用最近 200 轮的展示范围；页面可见时每 10 秒刷新，隐藏时暂停，恢复可见立即刷新。分享取消或源记录不可用后，页面在下次读取时清空正文并提示。分享关系原子保存到 `<state-dir>/session-shares.json`，服务重启后继续有效；内容从本地会话记录读取，服务需要保持在线，本地文件和附件不会因此开放访问。

链接使用当前浏览器访问的服务域名。接收者必须能够访问该地址；通过 localhost 或 SSH 本地转发访问时，需改用接收者可达的服务地址。若反向代理另有登录门禁，需要允许匿名 `GET/HEAD /share/{token}`、`GET/HEAD /api/public/session-shares/{token}` 及前端静态资源，其他接口继续保留原有保护。分享响应禁止缓存，并设置不发送 Referer 和不收录索引的响应头。

分享验证：`cargo test -p aow-server session_shares`；在 `frontend/` 下执行 `npm run test:shares`。

## Hermes

Hermes 适配已在 **0.18.0** 验证。将 `hermes` 所在目录加入 Settings 的执行 PATH 后，可自动发现或手动注册 Hermes；内置启动使用经典 `--cli` 界面，恢复会话使用 `--resume <session-id>`。`aow-cli agent create --agent hermes` 支持等待默认输入提示符就绪并提交初始任务；自定义皮肤改变提示符时可直接使用普通 Terminal。

Conversation 和分享页只读访问 Hermes 的 `state.db`，支持用户输入、工具调用与结果、最终回复、历史恢复和标题刷新。原地压缩后保留原始对话，合并复制的消息并隐藏内部摘要；撤销的消息保持隐藏。`HERMES_HOME` 指向根目录时跟随该根的 `active_profile`；指向 `profiles/<name>` 时固定使用该 profile。未指定时使用 `~/.hermes` 及其 active profile。注册多个实例时，建议各自显式设置 profile 目录。不需要开启 JSON 导出，也不会迁移数据库或安装 hook。

终端自动关联及完成提醒需要 Hermes `runtime/active_sessions.json` 中与当前 PID 对应的记录。Hermes 配置启用 `max_concurrent_sessions` 时会生成这份记录；未启用或记录失效时，在 Conversation 中手动选择会话，不自动订阅完成提醒，即使同目录只有一个候选也不推断归属。完成提醒从当前 SQLite 消息位置开始，只接受没有工具调用且明确标记 `stop`、`end_turn` 或 `stop_sequence` 的 assistant 记录；压缩复制的历史回复、工具输出、验证续跑和不完整回复不触发提醒。

自动化使用 `hermes chat --cli --quiet --query=<prompt>` 创建持久会话，按任务设置传入 `--yolo`。Hermes 在执行结束时向 stderr 输出 session ID，因此运行期间可能暂时没有会话链接；执行退出后才校验并保存该 ID。`--oneshot` 不提供这一协议，不能作为自动化启动参数。

验证：`cargo test -p aow-agents --all-features`、`cargo test -p aow-automations --test runtime hermes`；前端执行 `npm run build` 和 `npm run test:sessions`。

原生回归：将 `AOW_HERMES_TEST_PYTHON` 设置为 Hermes venv 的 Python，运行 `cargo test -p aow-agents --all-features --test hermes_native -- --ignored --nocapture --test-threads=1`。用例只在临时 profile 中检查压缩、重复消息、profile 解析和新 CLI 归属；CLI 用例需要 PTY 权限，不调用模型服务。非标准安装可用 `AOW_HERMES_TEST_SOURCE` 指定 Hermes 源码目录。

真实会话检查：在已打开的 Hermes 终端显示 `/status` 并保持空闲，设置 `AOW_HERMES_TEST_PID`、`AOW_HERMES_TEST_SESSION_ID` 和 `AOW_HERMES_TEST_RUNTIME_ID` 后，运行 `cargo test -p aow-server --test hermes_live -- --ignored --nocapture`。它只读核对进程、会话关联、原生消息、工具结果和屏幕；完成提醒通过临时数据库重放该会话验证。原生前台进程识别可用 `cargo test -p aow-terminald native_scan_matches_an_existing_hermes_process -- --ignored --nocapture` 检查。若同时设置 `AOW_HERMES_TEST_EXPORT` 为临时目录，可在前端目录使用同一变量运行 `node --test --test-name-pattern='exported native Hermes' tests/session-snapshot.test.mjs`，验证这份真实快照的桌面和手机展示。导出内容包含该会话的公开消息和工具输出，仅应保存在本机私有目录。

## 手机访问

手机打开同一个服务地址即可进入移动界面，也可以直接访问 `/m` 或 `/m/`。
`/?ui=desktop` 强制使用桌面版，`/?ui=mobile` 强制使用手机版。
自动选择以首次打开时的视口宽度（820px 及以下）为准，横竖屏切换和软键盘不会切换前端。

移动端提供项目/工作区导航、Terminal、Agent 会话快照、文件与 Notes 预览、Git Diff、只读 PR 列表与详情，以及自动化任务和执行记录查看。
Pinned 统一保存在服务端的 `aow-settings.json` 中，电脑与手机共用。旧版浏览器置顶在后端升级重启、原电脑网页刷新后自动迁移。
Terminal 的分割窗格会展开为独立标签。若已有其他窗口控制输入，新打开的页面会先以只读方式同步终端画面；需要编辑时再点击「接管」抢占唯一控制权。手机取得控制权后按当前工作区适配终端行列数，键盘开合保持行列数不变，字号和横竖屏变化时重新适配，支持手势滚动终端内容；桌面重新接管后按桌面窗格适配，分割布局保持不变。

## 自动化

GitHub 安装器和 `aow update` 会把 Runner 的稳定入口更新为所选 release；后续计划任务和手动任务自动使用新 Runner，已开始运行的任务不受影响。Runner 不是常驻进程，因此不需要单独的 start 命令。

点击左侧 Pinned 上方的“自动化”，创建任务并设置 Agent、项目、工作区和运行计划。支持 Codex、TraeCode CLI、Claude Code、Hermes；每次运行都会创建新会话。详情页显示概述和执行历史，可复制每次运行的 session ID。

Linux 使用 systemd user timer，macOS 使用当前登录用户的 launchd。定时器触发独立 Runner，前端或 Web 服务重启不会影响任务触发及已启动的执行。未实现应用层补跑或重试，也不读取 Agent rollout。
