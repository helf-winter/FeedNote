# FeedNote

FeedNote 是一个本地持久化、可追溯的个人记忆系统。它先可靠保存原始输入，再由 Memory Engine 调用用户授权的模型自动分类、命名和摘要；只有遇到无法忠实理解的歧义时才询问用户。

## 当前能力

- 快速输入和原始记录时间线；
- SQLite 本地持久化与全文搜索；
- 记忆类型、来源和版本时间线；
- 智谱 `glm-5.3` 自动分类、标题和摘要；
- 自动判断新建、更新已有记忆或建立关联，更新始终创建新版本；
- Embedding 不可用时自动回退 SQLite 全文检索；
- 指代不明或关键上下文缺失时进入待澄清区；
- 模型不可用时安全降级为普通记事本；
- 永久删除确认和 JSON 数据导出；
- 系统托盘与 `Alt + Shift + Space` 全局唤起；
- Windows 任意应用选中文字后显示投喂圆点，明确选择“喂”后才保存并调用模型；
- AI 从同一文本控件的有限周边内容提取完整时间，缺时间时直接询问；
- 透明桌面计划卡片与屏幕右侧折叠 logo；
- 计划卡片自动提取时间、标题、事项类型、链接和注意事项，链接交给系统浏览器打开；
- 可选 ntfy / Webhook 手机提醒，支持准时或提前 5/15/30/60 分钟推送；
- 可选飞书电子表格同步，一条计划对应一行，计划更新和完成状态会更新原行；
- 可选飞书投递表只读分析，按公司与岗位去重，把待投递、待笔试和备注中的明确后续动作转成计划；
- 浏览器预览模式，便于不启动桌面壳时检查界面。

完整边界和架构见 [TECHNICAL_DESIGN.md](TECHNICAL_DESIGN.md)。

## 磁盘位置

为了避免占用 C 盘，本项目将大型内容固定在项目目录：

```text
.tooling/rustup      Rust 工具链
.tooling/cargo       Cargo 缓存
.tooling/npm-cache   npm 缓存
.tooling/target      Rust 构建产物
node_modules         前端依赖
data/feednote.db     应用数据
data/secrets.env     本机密钥，不进入数据库或导出
```

这些路径均已加入 `.gitignore`。请通过 `scripts` 中的脚本运行项目，以保证环境变量生效。

## 运行

已经构建好的版本可以直接双击项目根目录的 `FeedNote.exe`。它不需要启动开发服务器，数据会写入同目录的 `data` 文件夹。

开发时只预览界面：

```powershell
.\scripts\preview.ps1
```

浏览器访问 `http://127.0.0.1:1420`。

运行桌面应用：

```powershell
.\scripts\dev.ps1
```

执行全部测试：

```powershell
.\scripts\test.ps1
```

构建桌面可执行文件：

```powershell
.\scripts\build.ps1
```

## 模型配置

基础记录、搜索、导出不依赖模型。当前默认使用智谱 Anthropic 兼容接口，密钥只从下面的文件读取：

```text
data\secrets.env
```

最小配置：

```env
ANTHROPIC_AUTH_TOKEN=
```

LLM 地址和模型使用应用内默认值 `https://open.bigmodel.cn/api/anthropic` 与 `glm-5.3`。Embedding 默认使用 `embedding-3`、512 维；若需独立密钥，可在同一文件添加 `EMBEDDING_API_KEY=`。目前账号没有 Embedding 通用额度时，应用会自动使用本地 FTS5 检索，不影响分类。

未来本地小模型通过 Provider 接口接入；原始数据结构和 Memory Engine 不与智谱绑定。

## 手机提醒配置

手机提醒默认关闭。首版推荐使用 ntfy：在手机安装 ntfy，订阅一个足够长且不可猜测的私有主题，然后在 `data\secrets.env` 添加：

```env
MOBILE_PUSH_ENDPOINT=https://ntfy.sh/feednote-替换为足够长的随机主题
MOBILE_PUSH_TOKEN=
```

受保护主题或自托管 ntfy 可在 `MOBILE_PUSH_TOKEN` 填写访问令牌。也可以在应用设置中选择 `webhook`，此时同一地址会收到只包含 `event`、`title`、`body`、`linkUrl` 和 `scheduledAt` 的 JSON。配置完成后，在“设置 -> 手机提醒”中开启并发送测试提醒。

## 飞书计划表配置

飞书同步默认关闭，使用企业自建应用的应用身份。应用需要发布，并开通以下最小权限：

```text
sheets:spreadsheet:create
sheets:spreadsheet:read
sheets:spreadsheet:write_only
```

在 `data\secrets.env` 添加：

```env
FEISHU_APP_ID=
FEISHU_APP_SECRET=
```

也可以运行 `scripts\configure-feishu.ps1`，App Secret 会使用隐藏输入并写入密钥文件，不显示在终端。

然后打开“设置 -> 飞书计划表”，开启同步并点击“初始化并同步”。FeedNote 会创建“FeedNote 计划”电子表格，写入现有计划，并在后续新增、补充时间或标记完成时更新对应行。同步失败不会回滚本地记录，后台会保留待同步状态并重试。

“设置 -> 飞书分析来源”可另行填写一个已有投递表链接。该通道只读取 `状态`、`公司/事项`、`岗位/方向`、`链接`、`备注` 五列，不改写来源表；没有公司名称的空行会忽略。直接待办使用确定性规则，备注中的自然语言安排才调用 LLM。来源没有行级时间戳时，相对时间必须询问用户，不能把扫描时间当作原始时间。

## 数据边界

- 原始投喂只追加，AI 无修改和删除权限；
- AI Gateway 只返回结构化判断，Memory Engine 校验后才可写数据库；
- 普通分类由 Memory Engine 校验后自动写入新版本；
- 同一主题的新进展可自动更新已有记忆，并继承全部原始来源；
- 删除任一来源会清除受影响的派生版本，再从剩余原文重建；
- 只有歧义与高风险冲突进入待澄清区；
- 永久删除只能由用户在界面中主动确认；
- 云调用只发送当前输入和最多 6 条、每条最多 800 字的候选记忆；
- 密钥不进入前端、数据库、日志和 JSON 导出；
- 不监听剪贴板、键盘，也不扫描目录。
- 选区监听阶段只读取 UI Automation 选区几何，不读取正文；点击圆点后只在内存暂存选中文字及前后各 1000 字，点击“不喂”立即丢弃且不联网、不落库；
- 选区功能不使用剪贴板、模拟复制或 OCR，不支持未暴露 Windows 无障碍文本接口的软件；
- 手机推送默认关闭；开启后只向用户配置的 HTTPS 地址发送计划卡片字段，不发送原始投喂、周边上下文、记忆库或本机路径；
- 飞书同步默认关闭；开启后只上传本地计划 ID、状态、时间、标题、内容、链接、注意事项、来源标题和更新时间；
- 飞书分析来源默认关闭且严格只读；本地只保存行指纹、计划映射和汇总统计，来源行删除不会自动删除本地记录；
- 计划不会自动创建系统日历、发送邮件或操作其他账号；已授权的手机提醒和飞书同步之外，其他外部动作仍被禁止。
