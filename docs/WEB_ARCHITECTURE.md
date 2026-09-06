# FeedNote Web 与多维表格方案

## 目标

FeedNote Web 用于跨设备查看和编辑个人计划。飞书多维表格是计划的主数据库；桌面 SQLite 只保存本地缓存和离线队列，飞书任务只承担提醒。

## 不可逾越的边界

1. 浏览器包、LocalStorage、Redux 和响应 JSON 中不得出现 `FEISHU_APP_SECRET`、用户访问令牌或刷新令牌。
2. 所有 Base 请求必须由服务端发起；浏览器只能调用同源 `/api`。
3. 首版只允许 `FEISHU_WEB_ALLOWED_OPEN_IDS` 中的飞书用户进入。不能仅凭“登录成功”放行。
4. 写请求必须同时通过 HttpOnly 会话和 CSRF 校验。
5. 计划内容只写入指定的 Base 和表，不能扫描用户的其他飞书文档。
6. 秘密备忘录不进入 Web API，也不进入计划 Base。
7. 删除采用 `已删除=true` 的软删除；不从 Base 物理删除记录。
8. 更新必须携带版本号；版本冲突返回 409，不允许静默覆盖另一端修改。
9. 飞书任务是派生数据。任务时间或完成状态回流后，必须先更新 Base，再投影到桌面缓存。
10. 旧的普通飞书计划表保留为历史数据，不自动删除。

## 数据模型

Base：`FeedNote 云计划`，表：`计划`。

核心字段：`标题`、`状态`、`时间`、`内容`、`详情`、`链接`、`注意事项`、`标签`、`来源`、`提醒提前分钟`、`计划ID`、`版本`、`更新来源`、`飞书任务GUID`、`所有者ID`、`已删除`、`更新时间`。

`计划ID` 是跨端稳定业务主键，Base `record_id` 只用于 API 定位。`所有者ID` 为未来多用户隔离预留，服务端仍须在每次读写时验证当前用户。

## 认证链路

1. 浏览器访问 `/api/auth/login`。
2. 服务端生成一次性 state，并在 OAuth 链接中显式请求 `auth:user.id:read`、`offline_access`、`base:record:retrieve`、`base:record:create` 和 `base:record:update` 后跳转到飞书授权页。
3. 回调校验 state，以 code 换取用户 token，并读取用户 `open_id`。
4. `open_id` 命中 allowlist 后，服务端建立 HttpOnly 会话。
5. token 只保存在服务端内存；服务重启后用户重新登录。正式多用户版需将 refresh token 加密持久化。

## 本地运行

1. 在飞书开发者后台添加重定向 URL：`http://127.0.0.1:4174/api/auth/callback`。
2. 运行 `powershell -ExecutionPolicy Bypass -File .\scripts\configure-web.ps1`。
3. 分别运行 `npm run web:api` 和 `npm run web:dev`。
4. 浏览器打开 `http://127.0.0.1:4173`。

## 后续多用户化

将 allowlist 替换为用户表，按 `open_id` 保存每个用户的 Base token/table id；refresh token 必须使用 KMS 或部署平台密钥加密。任何请求都不能接受浏览器传入的 Base token 作为最终授权依据。
