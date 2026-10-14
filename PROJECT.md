# NekoSportsWorldTool 多账号功能 · 项目书

> 本文档是本 fork 的单一事实来源（SSOT），随进度更新。
> 路径：仓库根 `PROJECT.md`（已入库）；`AGENTS.md` 只放工程约定，进度看这里。

## 一、项目目标

用户可输入多组账号，应用自动完成 **登录 → 自查 → 跑步 → 登出** 的批量链路；
每账号固定绑定独立的 **账号 / 密码 / 城市 / 经纬度 / 设备信息**，登录后自动使用该账户的
uuid 与设备身份。范围：**只做 GUI**，不做 CLI，不并发。

## 二、当前状态（2026-10-13）

| 项 | 状态 |
|---|---|
| 功能实现 | ✅ 完成，迭代 5 轮 |
| 本地验证 | ✅ `cargo check --all-targets` 0 警告 · `cargo test --lib` 85/85 |
| 构建产物 | ✅ CI（msvc）出 exe，已部署到 `dist\版本\` |
| 真机实测 | ⬜ 待做（4 项风险验证，见 §八） |
| 上游同步 | ✅ 已合入 upstream 6b677e1（无分叉） |

提交链：`bfe2db1` → `cf9f15a` → `b7d4968` → `e94057e` → `60b3c7b` → `3c7311c` → `d04be4c` → `7b538f5`

## 三、已实现功能

### 3.1 存储（槽位化）

```
data_dir（= exe 所在目录）
├── accounts.json            索引：id / username / password / remember / label / enabled
└── accounts/<账号>/
    ├── identity.json        设备身份（platform/device_id/idfa/imei/os/device_name/
    │                        manufacturer/mac/install_time/city/anchor）
    └── session.json         登录态（uid/token/unid/name/weight/profile）
```

- 槽位目录名 = 账号本身（只保留 `A-Za-z0-9_-`，重名加 `-2`/`-3`，全被过滤则回退随机 id）
- **老账号**索引里的 id 仍是 uuid hex，兼容不迁移
- `ApiClient::slot: Option<String>` 只决定**落盘位置**；`slot = None` 保持旧版单账号行为（CLI 不受影响）

### 3.2 账号页（入口：顶栏「账号」按钮，标签栏不显示）

- **添加账号**：账号 + 密码一行；设备信息 = 设备信息页同款字段 + 「随机生成」
  （随机 UUID / 机型 / 系统版本 / IDFA，保留位置）+「填充当前设备」+「默认位置」；
  设备与位置左右两列（行距 8px、设备字段宽 300，UUID/IDFA 不截断）；Android 多一个品牌字段
- **列表**：左右两列 ——
  - 左列（每行齐平）：`[勾选] ●名称 手机号  [详情][切换][删除]`
  - 右列常驻：状态 + 设备摘要（`UUID · 平台 机型 系统 · 城市(经纬度)`）
  - 点「详情」右列换成完整明细：槽位 / 账号+登录状态 / UUID / IDFA / IMEI /
    MAC + 安装时间（**上报字段 appInstallTime，非添加时间**）/ 平台机型系统 / 位置
  - 切换：装入该账号的设备身份 + 会话 + 顶栏凭据，顶栏显示「▶ 当前账号」，之后所有单账号操作作用于它
- **批量**：`一键：登录 → 自查 → 跑步 → 登出（选中/总数）`，按钮显示进度、运行时锁定单账号操作

### 3.3 批量链路

```
for 账号 in 已勾选账号（严格串行）:
    登录（有密码走完整链含 GT4 滑块；否则复用本地会话）
    自查 cheat
    跑步 run_full_flow（距离/配速/海拔/人脸/路线取全局配置）
    登出
    sleep 60~180s 随机（最后一个不等）
```

- 开始时间：今天 7:00 ~ now − 用时 − 5s 之间抽样，**绝不落在未来**（有单测）
- 失败也执行登出，不留挂着的会话；单账号失败只标记状态，不中断整批

### 3.4 设备身份规则（风控红线）

- 每账号一份 `identity`，`freshen()` 只在 **device_id 为空**时生成；MAC / 安装时间每次创建新生成；
  创建后终身复用（逐次重置会触发 10121 设备风险）
- 新建账号默认**清空 UUID**，保证与当前设备不同；IDFA / IMEI 留空则不上报
- IMEI 已真正接入协议：Android 请求头 `IMEI` + 登录体 `imei`（此前写死空串，
  设备页那个 IMEI 输入框其实写进了 `idfa`、请求从不使用 —— 已修）
- 默认城市锚点：**都匀市 26.286711 / 107.511853**；
  旧版大连默认（38.901678/121.540241 或「大连市」）仍判定为「未配置」，单账号与批量都会拒绝提交

## 四、数据文件一览

| 文件 | 说明 | gitignore |
|---|---|---|
| `config.json` | 全局跑步参数（距离/配速/海拔/人脸/路线/更新检查） | ✅ |
| `identity.json` / `session.json` | 单账号（默认槽位）设备与会话 | ✅ |
| `accounts.json` | 多账号索引，**含明文密码** | ✅ 已加入 |
| `accounts/` | 各账号槽位目录 | ✅ 已加入 |
| `points_cache.json` / `fence_cache.json` | 点位/围栏缓存，按锚点共享，账号无关 | ✅ |

## 五、构建与部署

| 场景 | 命令 |
|---|---|
| 本机验证（只能 check/test） | `cargo +stable-x86_64-pc-windows-gnu check --all-targets` / `test --lib` |
| 出 exe（唯一可行路径） | GitHub Actions：`gh workflow run release.yml --repo ZhYue314/NekoSportsWorldTool --ref main -f mode=full`，取 artifact `NekoSportsWorldTool-win-x64`，解压后放 `dist\版本\nekosportsworldtool.exe` |
| 同步上游 | `git fetch upstream` → `git merge upstream/main` → `git push origin main`（见 AGENTS.md） |

## 六、已知限制

1. **本机不能 `cargo build`**：gnu 链接器碰到既有问题 —— cdylib 导出 8.5 万符号 > PE 序号上限 65535
   （基线代码同样失败，与多账号改动无关）；CI 用 msvc 不受影响
2. **老账号槽位目录名** 仍是 uuid hex：程序运行中改名会让内存旧列表覆盖回索引，需删掉重建或关程序后迁移
3. **CI 的 `ubuntu-latest/aarch64` job 红**：手动触发时选了 `full`（GUI）模式，Linux 缺 GUI 依赖，与代码无关
4. 顶栏单账号登录与多账号共存：顶栏凭据会随「切换」同步，但直接在顶栏输入别的号登录会写进当前槽位

## 七、待办 / 待实测（必须真实账号）

1. 同 IP 两账号先后 登录→自查→跑→登出，看自查接口是否出标记（决定 60~180s 间隔是否够）
2. GT4 滑块对连续多账号登录的容忍度
3. 批量中途杀进程，检查 `accounts/<账号>/` 下 identity/session 是否仍配对
4. 提交 body 的 `address` 与轨迹城市是否一致（尤其跨城市账号）

可选打磨：批量进度条/失败账号单独重试、老账号目录迁移、README 补多账号用法

## 八、开发环境（本机）

- Rust 工具链：`D:\apps\rustup`（rustup + `stable-x86_64-pc-windows-gnu`），**未写入 PATH**
- MinGW-w64 GCC 16.2：`D:\apps\mingw16\mingw64`（旧的 `D:\apps\winGW` 是 GCC 8.1，太老不可用）
- GitHub 走代理 `127.0.0.1:7897`（Clash Verge，位于 `D:\apps\ClashVerge`）
- gh CLI：`D:\apps\githubCLI\bin\gh.exe`，登录账号 `ZhYue314`

## 九、变更记录

| 提交 | 内容 |
|---|---|
| `bfe2db1` | 槽位存储 + 批处理引擎 + 账号页（首版） |
| `cf9f15a` | AGENTS.md 远端与构建约定 |
| `b7d4968` | 顶栏账号入口按钮；创建表单补城市/经纬度 |
| `e94057e` | 勾选参与批量、账号详情明细行、切换可见化、默认位置改都匀、标签栏移除账号入口 |
| `60b3c7b` | 表单支持 UUID/IDFA/IMEI；IMEI 接入协议；详情折叠按钮；双列布局 |
| `3c7311c` | 表单复用设备信息逻辑 + 「随机生成」；账号密码一行；新建账号清空 UUID；槽位目录名 = 账号 |
| `d04be4c` | 新增项目书 PROJECT.md，AGENTS.md 指路 |
| `7b538f5` | 表单加宽加高（UUID/IDFA 不截断）；账号列表左右两列、右列常驻设备摘要与状态 |
