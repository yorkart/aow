# AoW 产品门户

独立部署到 GitHub Pages 的静态产品门户，目标地址为 <https://yorkart.github.io/aow/>。安装命令直接放在首屏；工作台展示复用现有 AoW 前端，数据来自真实采集的仓库快照。

## 真实前端与数据快照

`website/snapshot/main.tsx` 直接导入 `frontend/src/aow/ProjectAow.tsx` 和 `frontend/src/mobile/MobileAow.tsx`。文件树、标签页、Markdown、Monaco 编辑器、Git 差异、xterm 终端、桌面和手机导航均使用产品组件和样式。

`website/snapshot/data.json` 保存采集时间、仓库地址、完整提交 ID、原始 API 响应和真实 PTY 输出。目前采集范围是提交 `18c1d91` 的 227 个源码与文档文件、5 次实际提交及其文件差异。工作台中的项目对应 AoW 仓库，Notes 对应该提交的 `docs/`。项目头像接口也采集真实响应，桌面和手机直接引用返回的头像 URL。

采集脚本在临时目录克隆当前提交，启动隔离的 AoW server 与 terminald，通过真实 API 注册项目、创建终端并读取数据。终端实际执行了 `git log --topo-order -5 --oneline` 和 `git status --short --branch`，保存其原始输出；仅移除采集结束标记。提交清单和终端均使用与 Git API 相同的拓扑顺序，保证合并分支后的提交记录一致。临时克隆产生的远程跟踪引用会移除，以免把本地克隆误显示为已推送到 GitHub。

展示范围和交互：

- 浏览、切换、关闭文件标签，展开源码目录，切换 Markdown 预览，查看提交和 Diff。
- 使用真实 xterm 组件回看采集的终端输出，保持旁观模式。
- 手机端使用同一份快照数据和原生移动组件；两处预览各自保留本次浏览状态。
- 会话与自动化来自隔离实例的真实空列表，不生成对话、任务或成功记录。
- IM 设置来自隔离实例的真实未绑定状态，可查看飞书入口及微信“扫码绑定 → 向 Bot 发消息 → 发送测试 → 确认收件”的四步引导；扫码、发送和保存操作保持只读限制。
- 需要后端的操作（保存、执行命令、创建任务、Git 推送等）提示只读限制。未采集的读取明确显示“未收录”，不补造响应。
- 刷新或“重置快照”恢复初始浏览状态。浏览状态使用独立的内存存储。

采集不读取已有 AoW 状态目录或私人 Agent 会话。导出的临时路径替换为 `/workspace/aow`，主机路径与 Agent 环境变量不带入页面；所采集的源码内容与提交逐文件核对。

重复采集时保留完整的提交文件列表，但跳过 `website/snapshot/data.json` 及构建产物自身的 Git Diff，并在 `omittedCommitDiffs` 中记录跳过原因，避免递归包含上一版快照。

## 本地构建与预览

需要 Node.js 20+ 和 npm。构建复用 frontend 的依赖与锁文件：

```bash
npm ci --prefix frontend
node website/scripts/preview.mjs
```

打开 <http://127.0.0.1:4173/aow/>。预览使用与项目 Pages 相同的 `/aow/` 前缀，默认仅监听本机；使用 `PORT=4174` 调整端口。修改后重新构建并刷新：

```bash
node website/scripts/build.mjs
```

构建将真实前端、编辑器 worker 与本地资源打包到 `website/_site/snapshot/`。发布后的页面只需要静态文件，无需 AoW server、API 或 WebSocket 服务。构建不会重新采集数据。

更新数据快照时，先编译同一产品版本的前端、VT worker 和真实后端，再运行：

```bash
npm run build --prefix frontend
npm ci --prefix vt-worker --ignore-scripts --no-audit --no-fund
npm run build --prefix vt-worker
cargo build --locked -p aow-server -p aow-terminald
node website/scripts/capture-snapshot.mjs
node website/scripts/build.mjs
```

采集仅使用当前 HEAD 已提交的文件，结束后停止隔离进程并删除临时目录。更新快照后应审阅 `data.json`，重新进行数据校验与浏览器验收。

## 门户快照技能

使用 `$aow-portal-snapshot` 生成和刷新门户快照，完成来源确认、程序构建、真实数据采集、静态构建、数据校验、桌面与手机验收和分享图更新。技能源码见 [SKILL.md](../.agents/skills/aow-portal-snapshot/SKILL.md)，位于仓库根目录的 `.agents/skills/`，随仓库版本管理。Codex 在本仓库或其子目录工作时会自动发现，无需另行安装到个人技能目录；路径约定见 [Codex 官方技能文档](https://developers.openai.com/codex/skills/)。

建议调用：“使用 `$aow-portal-snapshot` 刷新门户数据快照，并完成源码一致性校验与页面验收。”

源码一致性检查也可单独执行：

```bash
node website/scripts/check-snapshot-freshness.mjs
```

采集时通过 Git 保存产品源码、依赖和文档的文件指纹，以及实际读取的提交列表，记录在 `data.json` 的 `sourceState` 中。检查将这些指纹与当前文件内容、文件模式逐一比较，也识别未提交的新源码。压缩或改写 Git 历史后仍能校验，无需保留旧提交对象；采集提交号和终端记录继续如实保留。只有门户数据或技能文件变化时允许沿用已有采集版本。过期检查不能代替浏览器验收。

## GitHub Pages 发布

1. 在仓库 **Settings → Pages → Build and deployment** 中，把 **Source** 设置为 **GitHub Actions**。
2. 将站点与 `.github/workflows/pages.yml` 合入 `main`。
3. 在 **Actions → Product portal / GitHub Pages** 查看构建部署，也可手动运行。
4. 部署成功后，从 `github-pages` 环境打开站点。

工作流使用官方 `configure-pages`、`upload-pages-artifact`、`deploy-pages`，Actions 固定到完整 SHA。构建权限只读，部署单独授予 `pages: write` 和 `id-token: write`；Pull Request 只构建与检查。前端源码变化也会触发门户构建。

只发布 `website/_site/`，包含首页、404、`.nojekyll`、sitemap、本地资源和快照前端。生成目录已忽略，无需提交构建产物或维护 `gh-pages` 分支。

生产地址来自 `configure-pages` 的 `base_url`；默认按 `GITHUB_REPOSITORY` 生成。因此支持项目 Pages、用户 Pages、其他仓库名和已配置的自定义域名：

```bash
SITE_URL=https://example.com/ node website/scripts/build.mjs
```

门户资源、canonical、Open Graph 与 sitemap 使用正确的部署前缀。快照保留产品的 Tab 路由；刷新深层链接时，404 页面将快照路由送回静态入口恢复，其余不存在的路径仍显示正常的 404。

官方参考：[使用自定义工作流发布 Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)、[自定义 404](https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-custom-404-page-for-your-github-pages-site)。

## 校验

```bash
frontend/node_modules/.bin/tsc -p website/snapshot/tsconfig.json --pretty false
node website/scripts/build.mjs
node --test website/tests/*.test.mjs
```

测试检查三种部署前缀、嵌套 404、快照深层路由、本地资源与编辑器 worker；将快照文件与从原始 Git 提交采集的文件指纹逐一核对；验证读取不会修改快照、写操作被拒绝、终端输入不会生成虚构输出。另有隔离仓库测试验证旧提交对象不存在时仍可校验，并能检测文件、权限和符号链接变动。

## 目录与素材

```text
.agents/skills/aow-portal-snapshot/  门户快照生成与维护技能
website/
  index.html                门户首页
  404.html                  错误页和快照路由恢复入口
  styles.css / main.js      门户样式、导航与安装命令复制
  snapshot-embed.*           桌面和手机 iframe 容器与入口联动
  snapshot-route.js         GitHub Pages 深层路由恢复
  snapshot/                 真实前端入口、静态传输适配、采集数据
  scripts/capture-snapshot.mjs  通过真实服务采集数据
  scripts/check-snapshot-freshness.mjs  检查源码与快照是否一致
  scripts/build-snapshot.mjs    打包现有 AoW 前端
  scripts/build.mjs          构建 Pages 发布目录
  scripts/preview.mjs        本地预览
  tests/                    数据与部署校验
```

`assets/workspace.png` 仅用于分享预览，从当前真实前端快照页面截图。页面内的工作台使用交互式前端。品牌图标来自产品已有素材；Agent 图标来源见 [Agent icons](../frontend/src/assets/agents/README.md)。文件图标使用 VS Code Seti 主题，来源见 [Seti icons](../frontend/src/assets/seti/README.md)，构建时会将其 MIT 许可证和第三方声明复制到 `snapshot/third-party/seti/`。

原 `frontend/public/promo/` 已移除，工作台“产品介绍”入口指向 Pages 门户。
