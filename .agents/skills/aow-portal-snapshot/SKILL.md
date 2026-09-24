---
name: aow-portal-snapshot
description: 生成和刷新 AoW 产品门户的真实前端数据快照，适配前端与 API 变化，验证桌面、手机交互和 GitHub Pages 静态构建。用于维护 AoW 门户中的交互工作台与分享截图。
---

# AoW 门户快照

产出与指定 AoW 源码版本一致、可在 GitHub Pages 独立运行的门户快照，以及真实前端截图。保留产品组件和交互，用实际采集数据驱动界面。

所有命令在当前任务的 AoW Git 根目录执行，确认存在 `frontend/`、`crates/`、`website/`。技能源码位于 `.agents/skills/aow-portal-snapshot/`，随仓库维护；不要硬编码某位用户的工作区路径。

## 实现定位

先读仓库 `website/README.md`；只在适配、采集或验证失败时展开相关实现：

| 入口                                           | 作用                                                           |
| ---------------------------------------------- | -------------------------------------------------------------- |
| `website/snapshot/main.tsx`                    | 直接加载正式桌面、手机组件和产品 Tab 路由；挂接门户场景入口    |
| `website/snapshot/transport.mjs`               | 用采集响应处理读取请求；向真实 xterm 回放 PTY 数据；拦截写操作 |
| `website/snapshot/basePath.ts`                 | 静态部署路径和独立内存存储                                     |
| `website/scripts/capture-snapshot.mjs`         | 用隔离的真实 AoW 服务采集 HEAD 的仓库文件、Git 和终端记录      |
| `website/scripts/build-snapshot.mjs`           | 打包当前正式前端及快照适配，不另画工作台                       |
| `website/scripts/check-snapshot-freshness.mjs` | 比较采集的源码指纹与当前文件，发现快照过期                       |

## 执行流程

1. **确认快照来源。** 默认采集当前 HEAD；用户指定分支、tag 或 commit 时，在对应的隔离检出中操作。记录完整 SHA，并检查产品源码的未提交与未跟踪变更。当前采集器从 HEAD 克隆文件，但编译器使用工作区源码，构建与采集需保持源码一致。有未提交改动时保留用户工作，说明哪些变化未被采集；来源不一致时不能判定校验通过。

2. **重新构建采集所需的正式程序。** 不复用来源不明的旧二进制，也不运行安装脚本去替换用户正在使用的服务。

   ```bash
   npm ci --prefix frontend --ignore-scripts --no-audit --no-fund
   npm run build --prefix frontend
   npm ci --prefix vt-worker --ignore-scripts --no-audit --no-fund
   npm run build --prefix vt-worker
   cargo build --locked -p aow-server -p aow-terminald
   ```

3. **采集并验证。** 采集器会在临时克隆和独立状态目录启动真实服务，调用 API 和 PTY，结束后清理进程。随后构建门户并运行检查。

   ```bash
   node website/scripts/capture-snapshot.mjs
   frontend/node_modules/.bin/tsc -p website/snapshot/tsconfig.json --pretty false
   node website/scripts/build.mjs
   node --test website/tests/*.test.mjs
   node website/scripts/check-snapshot-freshness.mjs
   ```

   检查 `data.json` 的采集提交、时间、文件内容、路径脱敏及终端原文。`sourceState` 保存从 Git 采集的源码指纹和提交列表，校验不依赖旧提交对象继续存在；压缩历史时仍保留真实采集提交号。文件和提交数量从当次结果读取，不沿用上一次采集的数字。任何步骤失败，都先处理失败原因；不要手改采集提交号、删除失败断言或伪造响应来通过检查。

4. **用真实浏览器验收当前界面。** 复用已有预览服务，或运行 `node website/scripts/preview.mjs`；默认入口 `/aow/`。按当前环境的浏览器技能操作，用界面上的实际控件验证，避免把按钮名称和布局冻结为旧版。

   - 桌面：从项目打开文件、展开目录、切换并关闭标签、预览 Markdown、打开内部文档链接、读取提交与 Diff、下载已采集文件。
   - 终端：回看实际执行的命令和原始输出；接管或输入不能生成虚构结果或访问真实后端。
   - 手机：项目进入、文件和终端浏览、底部导航、返回；验证手机宽度下无溢出，快照标识保持可见。
   - 门户：场景入口、重置、首屏安装命令复制；直接刷新工作台深层链接后仍能恢复。
   - 只读边界：会话和自动化保持采集时状态；写操作明确提示限制，未收录数据明确提示缺失。查看可用的错误和网络诊断，避免漏掉新增 API 请求。

5. **更新分享图并整理结果。** 从通过验收的真实快照截取 `website/assets/workspace.png`，重新生成站点以包含图片。记录来源版本、数据范围、适配变更、通过与未执行的检查。`website/_site/`、`website/_snapshot/` 和构建依赖不进入版本控制。交付采集数据、分享图和必要的适配改动，并说明未完成的检查。

## 随改版维护

- 正式前端组件和样式始终为唯一界面实现；组件入口移动时更新导入，不能退回复制布局或另造一套产品界面。
- 前端增加 API、字段或查询参数时，从对应真实服务重新记录，再更新读取映射和必要测试。缺失接口不能用空数组或成功结果掩盖；有记录的空状态才能展示为空。
- WebSocket 协议变化时更新回放适配，继续把采集的 PTY 数据交给正式终端组件。不能添加命令白名单来模拟执行结果。
- `capture-policy.mjs` 跳过快照数据自身及构建产物的 Git Diff，防止递归收录上一版数据；真实提交文件列表保持原样，遗漏记录写入 `omittedCommitDiffs`。迁移快照输出路径时同步维护这个规则。
- 基本交互变化时更新入口联动与浏览器验收，保持产品实际行为。必要的快照说明和只读限制放在包装层。
- 采集范围默认隔离仓库。不要读取用户已有的私人 Agent 会话或任务，也不要将主机地址、环境变量或凭据带入快照。增加展示场景应在隔离环境真实执行，并保存可追溯结果；任务未授权新增采集范围时保持现有范围。
