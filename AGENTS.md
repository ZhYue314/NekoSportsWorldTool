# AGENTS.md

## 项目文档

- **项目书（进度 SSOT）**：仓库根 [`PROJECT.md`](PROJECT.md) —— 功能范围、当前状态、数据结构、
  构建部署、已知限制、待实测清单、变更记录。**动手前先看它，改完记得同步它**。

## Git 远端约定（强制）

本仓库是 fork，两个 remote 各司其职，**不允许改动 remote 指向**：

| remote | 地址 | 用途 |
|---|---|---|
| `origin` | `https://github.com/ZhYue314/NekoSportsWorldTool.git` | **自己的 fork**，唯一允许 `git push` 的目标 |
| `upstream` | `https://github.com/YanamiNeko/NekoSportsWorldTool.git` | **原作者仓库**，只允许 `git fetch` / `git merge`，**禁止 push** |

分支 `main` 的 upstream 跟踪必须保持 `origin/main`（`git config branch.main.remote` == `origin`）。
注意 `git remote rename` 会自动改写分支跟踪，动 remote 后务必检查：

```powershell
git config branch.main.remote    # 必须是 origin
```

### 同步原作者新代码

```powershell
git fetch upstream                      # 只拉取，不合并
git merge upstream/main                 # 合到本地 main；冲突时逐个手工解决，不得丢弃任何一方改动
git push origin main                    # 推回自己的 fork
```

### 禁止事项

- `git push upstream` 或 `git push https://github.com/YanamiNeko/...`（推原作者仓库）
- `git remote set-url origin ...` 把 origin 改回原作者地址
- `git push --force`（含 `--force-with-lease`）到任何 remote
- 未 commit 直接 pull/merge 覆盖工作区；先 commit 再同步

### 身份与认证

- git 凭据走 gh keyring（`D:\apps\githubCLI\bin\gh.exe`），登录账号为 `ZhYue314`
- 访问 GitHub 需经本机代理 `http://127.0.0.1:7897`（Clash Verge）；
  若 fetch/push 报 `Failed to connect ... over proxy`，先启动 `D:\apps\ClashVerge\clash-verge.exe`

## 构建

- CI 使用 `x86_64-pc-windows-msvc`（见 `.github/workflows/release.yml`），本机需 MSVC Build Tools 才能 `cargo build`
- 本机 gnu 工具链只能用于 `cargo check` / `cargo test --lib`；
  `cargo build` 会因既有问题失败（cdylib 导出符号 > 65535，PE 序号上限）
- 详见 `PROJECT.md` §五 / §六
