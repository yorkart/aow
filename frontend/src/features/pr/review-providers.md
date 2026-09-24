# Review Provider 脚本协议 v2

通过 stdin 接收一个 JSON 请求，通过 stdout 返回一个 JSON 对象。Python 3 脚本实现 describe、list、detail、diff 四个 API，可选实现 `commit_links` 提供仓库和提交网页链接；具体 CLI/API 调用方式及平台 URL 规则由脚本决定。诊断日志写入 stderr。

v2 统一使用 Pull Request 命名，列表字段为 `pull_requests`。未经修改的旧版内置 GitHub 脚本会在服务启动时自动升级，保留 Provider 的名称、域名和启用状态。自定义或修改过的脚本不会被覆盖，需要更新为 v2 后重新上传；版本不匹配会明确报错。已有 v2 脚本未实现可选的 `commit_links` 时，原有 PR 功能继续可用，提交详情不显示远程链接。

工作台提供 `GET /api/my-pull-requests?repo=<仓库绝对路径>`、`GET /api/my-pull-requests/<number>?repo=...` 和 `GET /api/my-pull-requests/<number>/diff?repo=...&path=<仓库相对路径>`。这三个接口均可携带 `provider` 和 `remote`；列表只返回当前账号创建的 Open PR。

## 配置和仓库匹配

Settings → Pull Requests 管理 Provider：唯一 ID、显示名称、启用状态、remote 域名和 Python 脚本。脚本自行决定调用 CLI、HTTP API 或其他方式，AOW 只传入操作、仓库信息和参数。Python 和脚本调用的工具使用 Settings → Environment 的 PATH。运行所需工具与登录状态由用户准备。

首次初始化写入 `github.com → github` 和内置 GitHub 脚本；该脚本内部调用 `gh`。后续重启保留修改、停用和删除结果，不重新覆盖默认值。其他平台由用户添加适配器。页面仅提供脚本的只读预览；需要修改时，在本地编辑文件后点击“上传脚本”替换内容。上传仅在浏览器中读取文本供预览，点击保存后才生效。页面不接收或展示脚本路径。

配置保存在当前配置仓库的 `review-providers.json`，脚本由应用保存为 `review-providers/<内容摘要>.py`。先保存脚本版本，再发布引用它的配置，执行期间不会读到半份脚本。文件随现有配置仓库纳入版本记录。Settings 使用 revision 防止另一个窗口覆盖新配置；遇到冲突请重新打开设置并合并修改。

匹配 HTTPS、HTTP、ssh://、git:// 和 `git@host:owner/repo.git` 的 fetch remote，去掉 `.git`。HTTPS 非默认端口作为 host 的一部分，SSH 端口不参与域名匹配。域名精确匹配，不使用子串或通配符；SSH alias 可以直接作为域名配置，脚本可自行映射到实际 API host。无匹配时提示配置。多个不同仓库匹配时，桌面和手机均要求选择 remote；选择按本地仓库保存在浏览器。相同远端仓库的多个 remote 别名可自动选定。Provider、remote 和 PR 编号共同保存在 Tab 与分享链接中。

## 请求和响应

每次调用启动一个 Python 3 进程，工作目录为请求仓库；`describe` 没有仓库上下文。stdin 是完整 JSON，读取至 EOF。每次 stdout 只输出一个 JSON 对象，不要混入 CLI 日志。stderr 可以输出诊断日志。运行超时 60 秒，stdout 最大 16 MiB，stderr 最大 64 KiB；超时和取消时结束进程组。单个脚本最大 1 MiB，最多 32 个 Provider，设置请求最大 4 MiB。

```json
{
  "version": 2,
  "operation": "detail",
  "repository": {
    "root": "/workspace/project",
    "host": "github.com",
    "path": "owner/repository",
    "remote": "origin"
  },
  "params": { "number": 42 }
}
```

成功：退出码 0，输出 `{"version":2,"result":{...}}`。
业务失败：退出码 0，输出 `{"version":2,"error":{"code":"auth_required","message":"请先登录 CLI"}}`。
运行失败：非零退出码，stderr 描述错误。应用校验版本、封装格式、各 API 数据字段和资源编号；错误不会当成空列表。

`result` 和 `error` 必须二选一。字段名使用 snake_case。下面约定的字段应全部返回；未知字符串用 `""`，未知数量按其字段语义处理，未知可空值用 `null`，没有条目用 `[]`。时间使用 ISO 8601，编号必须为正整数，文件路径为仓库相对路径。应用会填充 `repository` 本地路径以及 `provider`、`provider_name`、`remote`，脚本不需要返回这些 Provider 身份字段。

## describe

请求：`operation: "describe"`，`repository: null`，`params: {}`。

```json
{"version":2,"result":{"operations":["list","detail","diff"]}}
```

用于检查协议版本和必需 API 的声明。Settings 的“检查协议”执行当前预览的脚本（包括尚未保存的上传内容），不保存它，也不代表实际仓库查询已通过。

支持提交外链时，在 `operations` 中追加 `"commit_links"`。应用只在脚本声明此能力后调用。

## commit_links（可选）

请求：`operation: "commit_links"`，`params: {"commit":"<完整提交 SHA>"}`。`repository` 结构与 PR 查询相同，包含本地根目录、配置匹配的 host、仓库路径和 remote 名称，不包含 remote URL 中的账号密码。

```json
{
  "version": 2,
  "result": {
    "remote_url": "https://code.example.com/projects/123",
    "commit_url": "https://code.example.com/projects/123/revisions/0123456789abcdef0123456789abcdef01234567"
  }
}
```

两个字段均需返回；无法提供的链接填 `null`。链接必须是无账号密码的绝对 HTTP(S) URL。脚本可以将 SSH alias、仓库路径或 remote host 映射到不同的网页域名和路径，应用不要求链接域名等于 remote 域名。

提交详情由本地 Git 提供，remote 优先取当前分支的 tracking remote，其次为 origin，再取其他 remote。应用只调用该 remote 匹配的已启用 Provider，不借用其他 remote 的 Provider。Git 服务层不识别 GitHub、GitLab 等平台，也不将 clone URL 推断为网页 URL。

链接解析是可选补充，总预算为 3 秒（包含配置读取、remote 匹配、describe 和 commit_links）。没有匹配的 Provider、未声明能力或返回 null 时不显示外链；脚本失败、链接无效或超时时也保留完整的本地提交详情，错误记录在服务端日志中，不回退到内置 URL 拼接规则。脚本宜在本地直接生成链接，避免网络请求延迟提交详情。

## list

请求：`operation: "list"`，`params: {}`。返回当前账号在该远端仓库创建的 Open PR；获取当前用户、分页和平台数据转换由脚本完成。

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

## detail

请求：`operation: "detail"`，`params: {"number":42}`。返回 Summary 全部字段，加上：

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

## diff

请求：`operation: "diff"`，`params: {"number":42,"path":"src/app.py","patch_only":false}`。

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

## 内置 GitHub 适配器

内置脚本使用 gh api 和 gh pr view，显式指定远端 host/repository，分页加载 PR、文件和讨论。代码讨论使用 GraphQL 的 isResolved，并处理线程内评论分页；辅助部分失败展示 warnings。diff 读取 merge base 与 head SHA 对应的内容；超过 1 MiB 或不可取得完整文本时提示不完整。GitHub files API 的 3000 文件上限会在详情中显示提示。

`commit_links` 由此 Python 脚本按 GitHub 的 URL 规则生成，使用配置匹配的 host；无需调用 gh、登录或访问网络。其他平台的 URL 规则由用户注册的脚本实现。

实现参考：[gh api](https://cli.github.com/manual/gh_api)、[gh pr view](https://cli.github.com/manual/gh_pr_view)、[GitHub Pull Requests REST API](https://docs.github.com/en/rest/pulls/pulls)、[GitHub GraphQL Pull Requests](https://docs.github.com/en/graphql/reference/pulls)。
