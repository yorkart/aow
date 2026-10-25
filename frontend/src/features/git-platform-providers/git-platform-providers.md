# Git Platform Provider 协议（v4）

通过 stdin 接收一个 JSON 请求，通过 stdout 返回一个 JSON 对象。Node.js 单文件 `.mjs` 脚本实现 `describe` 并声明至少一种只读能力。平台能力按 `repository.*`、`pull_requests.*` 分组，Pull Requests 是平台 Provider 的一个使用方；平台 CLI/API 调用方式及 URL 规则由脚本决定。诊断日志写入 stderr。

机器可读协议通过无需登录的 `GET /api/public/git-platform-provider-schema` 获取；部署在子路径时需加上应用前缀。该接口包含每个操作的 `inputSchema`、`outputSchema`、共享类型、错误格式、语义和示例，可直接交给 AI 工具生成实现。`inputSchema` 描述请求的 `params`，完整进程请求由 `transport.requestSchema` 定义；本地 `$ref` 从整个文档根节点解析。协议仅声明读操作，不提供创建、评论、审批、合并等写 API。

v4 统一使用 `pull_requests` 命名；GitLab MR 也转换为相同字段。只读协议不等同于运行沙箱，上传的脚本以服务进程的账号权限执行，应只上传信任的实现。

平台管理接口为 `GET/PUT /api/aow/git-platform-providers`；`POST /api/aow/git-platform-providers/test` 校验上传脚本。`GET /api/git-platform-targets?repo=...&operations=repository.info` 按域名和声明的能力筛选远端；`operations` 可省略或用逗号分隔多个操作名，同时满足全部操作的 Provider 才会返回。

工作台提供 `GET /api/my-pull-requests?repo=<仓库绝对路径>`、`GET /api/my-pull-requests/<number>?repo=...` 和 `GET /api/my-pull-requests/<number>/diff?repo=...&path=<仓库相对路径>`。这三个接口均可携带 `provider` 和 `remote`；列表默认返回当前账号创建的 Open PR，可传 `state=all` 同时获取 Merged 和 Closed PR。桌面侧栏按 Open PRs、Merged PRs、Closed PRs 分组，Open PRs 的菜单可过滤 Draft。

## 配置和仓库匹配

设置 → Git 平台 仅上传 `.mjs` 单文件。文件名（含扩展名）就是唯一 Provider ID，并原样保存；同名上传更新原 Provider，不同文件名新增 Provider。文件名最多 100 个 ASCII 字符，以字母或数字开头，其余只允许字母、数字、点、下划线和短横线；不允许大小写冲突。不改名，也不生成另一份随机文件名。显示名称和 Remote domains 由脚本的 `describe` 返回，页面只读展示；用户通过上传和移除管理 Provider。

上传时运行 `describe` 校验，成功后立即保存并生效；同名上传替换原脚本，移除立即删除对应脚本并停止使用。页面以列表展示 Provider ID、Display name 和 Remote domains，不提供启停开关、表单或脚本预览。单文件必须自包含，可用 Node.js 内置模块，不能依赖相邻文件或由 AoW 执行 npm install。脚本可以调用已安装的 CLI，运行时 Node.js 和工具通过 Settings → Environment 的 PATH 查找，登录状态由用户准备。

GitHub、GitLab 默认实现位于源码 `git-platform-providers/github.mjs` 和 `git-platform-providers/gitlab.mjs`，随发布包携带，不嵌入 Rust。服务初始化时从包资源安装到配置目录的 `git-platform-providers/`；开发构建从源码目录读取。首次初始化提供 `github.com` 和 `gitlab.com`，分别使用 `gh` 和 `glab`。企业自建实例可复制脚本，修改 `describe.hosts` 后以独立文件名上传。

服务启动时扫描应用配置目录 `git-platform-providers/` 目录（默认位置为 `~/.aow/git-platform-providers/`），不限定平台或文件名列表。直接放入该目录的 `.mjs` 文件无需预先登记，重启后会执行 `describe` 校验并加载名称、域名和只读能力。只扫描目录直属的普通文件；格式、大小或协议校验失败的脚本会跳过并记录服务端日志。相同域名冲突时保留已加载 Provider，跳过冲突脚本并记录日志；通过设置上传时会拒绝域名冲突。

Provider 元数据和默认版本记录保存在当前配置目录的 `git-platform-providers.json`，该清单不决定哪些 `.mjs` 文件可以被发现；脚本原样保存为 `git-platform-providers/<上传文件名>.mjs`。升级只更新仍与上一发布版本一致的默认源码，保留用户修改和移除决定；从设置中移除时也删除对应脚本文件；被移除的默认 Provider 不会因重启再次添加。配置中的摘要只记录默认版本更新依据，不参与文件命名。Settings 使用 revision 防止另一窗口覆盖新配置；发生冲突时重新打开设置并合并修改。

匹配 HTTPS、HTTP、ssh://、git:// 和 `git@host:owner/repo.git` 的 fetch remote，去掉 `.git`。HTTPS 非默认端口作为 host 的一部分，SSH 端口不参与域名匹配。域名精确匹配，不使用子串或通配符；SSH alias 可以直接由 describe.hosts 返回，脚本可自行映射到实际 API host。无匹配时提示配置。多个不同仓库匹配时，桌面和手机均要求选择 remote；选择按本地仓库保存在浏览器。相同远端仓库的多个 remote 别名可自动选定。Provider、remote 和 PR 编号共同保存在 Tab 与分享链接中。

## 请求和响应

每次调用启动一个 Node.js 进程，工作目录为请求仓库；`describe` 没有仓库上下文。stdin 是完整 JSON，读取至 EOF。每次 stdout 只输出一个 JSON 对象，不要混入 CLI 日志。stderr 可以输出诊断日志。运行超时 60 秒，stdout 最大 16 MiB，stderr 最大 64 KiB；超时和取消时结束进程组。单个脚本最大 1 MiB，最多 32 个 Provider，设置请求最大 4 MiB。

```json
{
  "version": 4,
  "operation": "pull_requests.detail",
  "repository": {
    "root": "/workspace/project",
    "host": "github.com",
    "path": "owner/repository",
    "remote": "origin"
  },
  "params": { "number": 42 }
}
```

成功：退出码 0，输出 `{"version":4,"result":{...}}`。
业务失败：退出码 0，输出 `{"version":4,"error":{"code":"auth_required","message":"请先登录 CLI"}}`。
运行失败：非零退出码，stderr 描述错误。应用校验版本、封装格式、各 API 数据字段和资源编号；错误不会当成空列表。

`result` 和 `error` 必须二选一。字段名使用 snake_case。下面约定的字段应全部返回；未知字符串用 `""`，未知数量按其字段语义处理，未知可空值用 `null`，没有条目用 `[]`。时间使用 ISO 8601，编号必须为正整数，文件路径为仓库相对路径。应用会填充 `repository` 本地路径以及 `provider`、`provider_name`、`remote`，脚本不需要返回这些 Provider 身份字段。

## describe

请求：`operation: "describe"`，`repository: null`，`params: {}`。

```json
{"version":4,"result":{"name":"Example Git","hosts":["git.example.com"],"operations":["pull_requests.list","pull_requests.detail","pull_requests.diff"]}}
```

返回显示名称 `name`、精确域名数组 `hosts` 和能力数组 `operations`，至少声明一种协议支持的只读操作，无需实现 PR 能力。Pull Requests 功能只选择同时声明 `pull_requests.list`、`pull_requests.detail`、`pull_requests.diff` 的 Provider。名称会去除首尾空白，域名会转为小写。describe 不需要网络、平台 CLI 或登录状态，不返回 Provider ID。上传时校验 describe 不代表实际仓库查询已通过。

支持提交外链时，在 `operations` 中追加 `"repository.commit_links"`。应用只在脚本声明此能力后调用。

## repository.info（可选）

在 `describe.operations` 中追加 `"repository.info"`。请求：`operation: "repository.info"`，`params: {}`；`repository` 与 PR 查询相同，包含 `root`、`host`、`path`、`remote`，不包含 remote URL 中的账号密码。

```json
{"version":4,"result":{"avatar_url":"https://avatars.example.com/team.png"}}
```

`avatar_url` 为仓库所属用户或组织的头像，不是当前登录用户的头像。没有可用头像时返回 `null`；请求或鉴权失败应返回 error。URL 必须是无账号密码的绝对 HTTP(S) URL，并能由用户的浏览器直接加载；不要返回需额外 Authorization 请求头的地址或在 URL 中嵌入访问令牌。

项目使用 `origin`，没有 `origin` 时使用唯一的 remote；多个 remote 且没有 `origin` 时不自动选择。选定的 remote 无匹配 Provider 时显示默认图标，不借用其他 remote 的 Provider，也不随当前分支变化。未声明此能力时显示默认图标，PR 功能不受影响。查询不依赖仓库是否存在 PR，也不读取仓库中的 logo 文件。

桌面和手机通过 `GET /api/aow/projects/<id>/avatar` 独立加载头像，不阻塞项目注册或列表。AoW 只保存头像 URL，不下载图片，前端直接引用，图片加载失败时回退到默认图标。查询失败时保留此前保存的头像 URL。已解析 URL 缓存 15 分钟，无头像或查询失败缓存 1 分钟；Provider 配置、remote 地址或执行 PATH 改变时重新查询。头像脚本的 describe 和 repository.info 合计最多 8 秒，整个元数据查询最多 12 秒。

## repository.commit_links（可选）

请求：`operation: "repository.commit_links"`，`params: {"commit":"<完整提交 SHA>"}`。`repository` 结构与 PR 查询相同，包含本地根目录、配置匹配的 host、仓库路径和 remote 名称，不包含 remote URL 中的账号密码。

```json
{
  "version": 4,
  "result": {
    "remote_url": "https://code.example.com/projects/123",
    "commit_url": "https://code.example.com/projects/123/revisions/0123456789abcdef0123456789abcdef01234567"
  }
}
```

两个字段均需返回；无法提供的链接填 `null`。链接必须是无账号密码的绝对 HTTP(S) URL。脚本可以将 SSH alias、仓库路径或 remote host 映射到不同的网页域名和路径，应用不要求链接域名等于 remote 域名。

提交详情由本地 Git 提供，remote 优先取当前分支的 tracking remote，其次为 origin，再取其他 remote。应用只调用该 remote 匹配的 Provider，不借用其他 remote 的 Provider。Git 服务层不识别 GitHub、GitLab 等平台，也不将 clone URL 推断为网页 URL。

链接解析是可选补充，总预算为 3 秒（包含配置读取、remote 匹配、describe 和 repository.commit_links）。没有匹配的 Provider、未声明能力或返回 null 时不显示外链；脚本失败、链接无效或超时时也保留完整的本地提交详情，错误记录在服务端日志中，不回退到内置 URL 拼接规则。脚本宜在本地直接生成链接，避免网络请求延迟提交详情。

## pull_requests.list

请求：`operation: "pull_requests.list"`，`params: {}` 或 `params: {"state":"open"}` 返回当前账号在该远端仓库创建的 Open PR；`params: {"state":"all"}` 返回该账号创建的所有状态 PR。获取当前用户、分页和平台数据转换由脚本完成，建议按更新时间倒序返回。

默认 GitHub 和 GitLab 脚本均支持 `state=all`；自定义实现也应支持该参数。

```json
{
  "repository": "/workspace/project",
  "current_branch": "feature",
  "current_user": {"id":"1","username":"alice","display_name":"Alice"},
  "pull_requests": [{
    "number":42,"status":"open","draft":false,"title":"Example",
    "source_branch":"feature","target_branch":"main",
    "url":"https://github.com/owner/repository/pull/42",
    "created_at":"2026-09-23T01:00:00Z","updated_at":"2026-09-23T02:00:00Z"
  }]
}
```

`pull_requests[]` 是 Summary，`current_user` 是 User。Summary 的 `url` 可为 null，`status` 使用 `open`、`closed`、`merged`。Provider 应完整处理分页，不把失败误报为空结果。

## pull_requests.detail

请求：`operation: "pull_requests.detail"`，`params: {"number":42}`。返回 Summary 全部字段，加上：

```json
{
  "description":"Markdown body",
  "changes_count":1,"commits_count":1,
  "review_status":"approved","check_summary_status":"passed","mergeable":true,
  "reviewers":[{"id":"2","username":"bob","display_name":"Bob"}],
  "author":{"id":"1","username":"alice","display_name":"Alice"},
  "labels":["feature"],"milestone":null,"diverged_commits_count":0,
  "merge_checks":[{"name":"合并条件","passed":true,"reason":"条件已满足"}],
  "checks":[{
    "id":"check-1","name":"test","status":"completed","conclusion":"success",
    "details_url":null,"description":"","text":"Markdown output","required":false,
    "started_at":null,"completed_at":null
  }],
  "files":[{"path":"src/app.py","change_type":"M","additions":1,"deletions":1}],
  "threads":[{
    "id":"thread-1","path":"src/app.py","line":12,"status":"resolved",
    "author":"bob","body":"Please check","updated_at":"2026-09-23T02:00:00Z",
    "comments":[{
      "id":"comment-1","author":"bob","body":"Please check",
      "created_at":"2026-09-23T02:00:00Z","updated_at":"2026-09-23T02:00:00Z"
    }]
  }],
  "unresolved_threads":[],
  "warnings":[]
}
```

上面的示例省略了 Summary 字段；实际返回对象须包含它们。`unresolved_threads` 填入 `threads` 中尚未解决的代码讨论。线程状态为 `open`、`resolved` 或 `comment`（一般讨论），非代码讨论 `path` / `line` 为 null。`author`、`mergeable`、`milestone`、检查 URL/时间、合并条件 `passed`、文件行数可以为 null。

`change_type` 使用 `A` / `M` / `D` / `R` / `C`，表示新增、修改、删除、重命名、复制。Review 状态可用 `approved` / `changes_requested` / `review_required` / `unknown`；检查汇总可用 `passed` / `failed` / `pending` / `unknown`。其他平台状态按原值展示。

主数据失败应返回 error。可独立加载的检查、评论等部分失败时，可以返回已有详情并在 warnings 明确说明，不要假装已完整加载。变更文件列表受平台上限影响时也必须说明。

## pull_requests.diff

请求：`operation: "pull_requests.diff"`，`params: {"number":42,"path":"src/app.py","patch_only":false}`。

```json
{
  "repository":"/workspace/project","number":42,"path":"src/app.py",
  "original_path":null,
  "original":"before\n","modified":"after\n",
  "patch":"--- a/src/app.py\n+++ b/src/app.py\n@@ -1 +1 @@\n-before\n+after\n",
  "binary":false,"truncated":false
}
```

`original` / `modified` 是完整文本；新增/删除的一侧为空字符串，获取失败不能用空字符串冒充。重命名时 `original_path` 为旧路径。差异应使用平台 PR 的比较基点（通常 merge base），不能使用目标分支当前最新版本替代。

移动端传 `patch_only: true`，此时必须提供 unified diff `patch`，可以不返回两侧全文（填 null）。没有文本差异时 patch 为空字符串。二进制文件设置 binary=true；内容太大或不完整设置 truncated=true，无法提供的内容用 null，UI 将明确提示。

## 默认平台实现

`repository.info` 使用 `gh api --hostname <host> repos/<owner>/<repo>`，读取 `owner.avatar_url`，复用服务器上 gh 的登录状态。用户浏览器仍需能访问返回的头像地址。自定义脚本按需添加此可选能力。

GitHub 默认脚本使用 gh api 和 gh pr view，显式指定远端 host/repository，分页加载 PR、文件和讨论。代码讨论使用 GraphQL 的 isResolved，并处理线程内评论分页；辅助部分失败展示 warnings。diff 读取 merge base 与 head SHA 对应的内容；超过 1 MiB 或不可取得完整文本时提示不完整。GitHub files API 的 3000 文件上限会在详情中显示提示。

`repository.commit_links` 由此 `.mjs` 脚本按 GitHub 的 URL 规则生成，使用配置匹配的 host；无需调用 gh、登录或访问网络。GitLab 使用对应的 `/-/commit/<SHA>` 网页路径；其他平台的 URL 规则由上传的脚本实现。

实现参考：[gh api](https://cli.github.com/manual/gh_api)、[gh pr view](https://cli.github.com/manual/gh_pr_view)、[GitHub Pull Requests REST API](https://docs.github.com/en/rest/pulls/pulls)、[GitHub GraphQL Pull Requests](https://docs.github.com/en/graphql/reference/pulls)。

GitLab 默认实现使用 `glab api --method GET`，显式指定主机并处理分页，读取 MR、讨论、审批状态、流水线和作业。讨论按 resolvable/resolved 转换为线程状态，辅助信息失败通过 warnings 展示。diff 使用 `diff_refs.base_sha` 和 head SHA，重命名时分别读取旧、新路径。`repository.info` 返回 namespace 的用户／组织头像，缺失时返回 null。
