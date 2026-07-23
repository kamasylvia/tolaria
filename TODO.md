# Tolaria Fork TODO

> Fork: `kamasylvia/tolaria`（origin = GitHub fork SSH；upstream = `refactoringhq/tolaria`）
> 维护人：Laniakea Kamasylvia
> 最近更新：2026-07-17

---

## 0. 项目目标

在 tolaria fork 上新增两个核心功能：

1. **Typst 预览** —— 笔记内联渲染 Typst 文档，支持多文件项目 + `#import` 相对路径锚点（参考 tinymist）
2. **Trilium 风格 Softlinks** —— 一个笔记在树里多个位置出现（克隆），不复制文件

---

## 1. 仓库拓扑（已就位）

| Remote | URL | 用途 |
|---|---|---|
| `origin` | `git@github.com:kamasylvia/tolaria.git` | fork 主推送目标（SSH） |
| `upstream` | `git@github.com:refactoringhq/tolaria.git` | 上游（仅 fetch，吸收更新） |

- fork 是 `refactoringhq/tolaria` 的真 fork（GitHub fork 关系）
- fork 当前领先 upstream（Typst 预览 tinymist 集成、folder 单层/递归 toggle、quick launcher、startup shell、HTML export/PDF 等自有工作）
- fork 与 upstream 定期手动同步（最近一次 2026-07-23，合并 upstream 8 提交）
- **ADO 已废除**：pipeline 67（tolaria-upstream-sync）已于 2026-07-17 删除，ADO 不再承担 CI/同步职责
- GitHub fork 的 Actions 之前已禁用（`enabled: false`）—— **TODO 确认是否需要重新启用**

### CI 策略

- 不跑远程 build/test CI（上游是私有仓库，CI 验证靠本地 pre-push hooks）
- 上游同步：手动 `git fetch upstream && git merge upstream/main`，处理冲突后 push
- 本地 pre-push hook（`.husky/pre-push`）是唯一质量门禁

### 分支约定（2026-07-23 定）

| 分支 | 角色 | 推 remote? |
|---|---|---|
| `tinymist` | **开发分支** —— 所有功能/修复在此 commit + 测试 | 否（本地） |
| `main` | **推送分支** —— tinymist 验证通过后 ff/merge 过来，从这里 `git push origin main` | 是（唯一推送源） |

- pre-push hook 限制只允许从 `main` 推送（`.husky/pre-push` 检查当前分支）
- 工作流：tinymist 开发 → `git checkout main && git merge --ff-only tinymist` → `git push origin main`
- 上游同步：在 tinymist 上 `git fetch upstream && git merge upstream/main` 解冲突 → 验证 → ff 到 main → push
- 历史：旧的 `fix/typst-preview-packages-cjk-fonts` 分支（手写 Typst World 方案）已于 2026-07-23 删除，被 main 上的 tinymist-world 方案替代

---

## 2. 开发环境（已就位）

- 工作目录：`/Volumes/UNITEK/Documents/Development/tolaria`（外置盘）
- Shell：fish 4.7.1（默认）；`~/.config/fish/conf.d/dev-caches.fish` 重定向 CARGO_HOME/RUSTUP_HOME/PNPM_HOME 到 `/Volumes/UNITEK/`
- pnpm 10.34.5（brew `pnpm@10`，与上游 CI 一致）
- rustup stable 1.96.1（toolchain 在 `/Volumes/UNITEK/.rustup`）
- npm registry 镜像：`registry.npmmirror.com`
- 凭证：`~/.config/fish/conf.d/secrets.fish`（chmod 600，含 `AZURE_DEVOPS_EXT_PAT`、`GITHUB_TOKEN`）；infisical 连接在 `.infisical.json`（gitignored）

### 内置 SSD 保护

内置盘仅 19 GB free。全局缓存全重定向到外置盘：
- `CARGO_HOME=/Volumes/UNITEK/.cargo`
- `RUSTUP_HOME=/Volumes/UNITEK/.rustup`
- `PNPM_HOME=/Volumes/UNITEK/.pnpm` + `pnpm config store-dir /Volumes/UNITEK/.pnpm/store`
- 项目 `node_modules/` 和 `src-tauri/target/` 跟随项目落外置盘

---

## 3. 功能 A：Typst 预览

**状态**：✅ 已完成（2026-07-23，已 push 到 origin/main）

实现采用 tinymist-world 集成方案（ADR-0171 选型的演进）：`CompileOnceArgs::resolve_system()` + `typst::compile` + `svg_merged`，字体/包/VFS 全交 tinymist。typst_preview.rs 仅 189 行薄壳。详见 commit `5dd8990b` 及后续（DOMPurify glyph 保留、分页视觉、folder 单层/递归 toggle）。

### 选型（已定）

**Option 3：`typst` Rust crate 在 src-tauri 编译，SVG 经 `invoke` 回前端 inline 渲染**

- 纯 Rust 依赖，零 JS bundle 增重
- SVG 经 `dompurify.sanitize` inline 注入主 webview（复用 `FilePreview.tsx` 的 `<object>` PDF 模式思路）
- `typst::World` trait 的 `main()` + `FileId`-相对-root 是 `#import`/`#include` 相对路径的锚点机制

否决方案：
- tinymist sidecar（~20-30 MB 原生二进制 + WebSocket + CSP 改 `ws://127.0.0.1:23635`，过重）
- typst.ts WASM（JS bundle 涨 5-10 MB，与"keep the app lean"冲突）

### 落点（架构调研已确认）

**Rust 侧**：
- `src-tauri/Cargo.toml` 加：`typst = "0.14"`、`typst-svg = "0.14"`、`typst-assets = "0.14"`（内嵌字体）；可选 `typst-pdf = "0.14"`（复用现有 PDF 预览）
- 新建 `src-tauri/src/commands/typst.rs`：`#[tauri::command] render_typst(entry_path, root_path, main_path) -> Result<String, String>` 返回合并多页 SVG
- 实现 `typst::World`（参考 `typst-cli` / `typst-as-lib`，约 80 行）
- `src-tauri/src/commands/mod.rs` 加 `pub mod typst;` + re-export
- `src-tauri/src/lib.rs:485` 在 `app_invoke_handler!` 宏注册 `commands::render_typst`
- `src-tauri/src/vault/mod.rs:329 classify_file_kind` 加 `.typ`/`.typst` → 新 `fileKind: "typst"`

**前端**：
- `src/utils/filePreview.ts`：`FilePreviewKind` 加 `'typst'`，扩展名集合加 `'typ'`/`'typst'`；放宽 `if (entry.fileKind && entry.fileKind !== 'binary') return null` 守卫
- `src/components/FilePreview.tsx` 的 `FilePreviewBody` 加 `previewKind === 'typst'` 分支
- 复用 `vault_watcher` 监听 `.typ` 变更重渲染
- `FilePreviewHeaderIcon` 加 typst 图标

### 入口文件锚点（4 层优先级）

`#import` 相对路径要锚到项目主文件（大概率 `main.typ` 但可能不是）：

1. **frontmatter 提示**：`typst_root: report/main.typ` → 以该文件为入口、其父目录为 root
2. **会话手动 pin**：preview 工具栏"Pin entry file"（对齐 tinymist 的 `tinymist.pinMainToCurrent`）
3. **自动探测**：同目录有 `main.typ` → 一键"Preview as project (main.typ)"
4. **单文件默认**：root = 笔记父目录，main = 笔记本身（对齐 tinymist `singleFile`）

### 交付清单

- [ ] ADR-0171：Typst 预览渲染选型（引 ADR-0002/0108/0136/0154）。编号接上游 0170（`measurable-crash-safe-startup`）
- [ ] `src-tauri/src/commands/typst.rs` + World impl + `render_typst` 命令
- [ ] `src-tauri/src/vault/mod.rs:329 classify_file_kind` 加 `.typ`（`TEXT_EXTENSIONS` 在 `mod.rs:260`）
- [ ] `src/utils/filePreview.ts` + `src/components/FilePreview.tsx` 加 typst 分支
- [ ] 入口锚点 4 层解析 + UI（frontmatter / pin / auto-detect / single-file）
- [ ] vitest（前端分发）+ `cargo test`（World 实现）
- [ ] 本地化（`src/lib/locales/en.json` + `pnpm l10n:translate`）
- [ ] PostHog：`typst_preview_opened`、`typst_root_pinned`
- [ ] native QA：`pnpm tauri dev` 截图验证

### 关键参考

- **ADR-0168**（Sandboxed standalone HTML file previews，2026-07-20）—— 最近的"standalone 文件预览"范例，editor pane 里 sanitized opaque-origin iframe + raw-mode 源码编辑切换。Typst 预览形态最接近这个 ADR，`src/components/HtmlFilePreview.tsx` 是直接的代码模板
- **ADR-0136**（macOS Webview PDF Export）—— 若 Typst 走"编译 PDF 再预览"分支，复用这条路径
- **ADR-0086/0098/0110/0121** —— image/pdf/media preview 演进链，`FilePreview.tsx` 的扩展规则与 `<object>` PDF 模式来源
- tinymist 项目/入口模型：<https://myriad-dreamin.github.io/tinymist/feature/project.html>
- `typst` crate `World` trait：<https://docs.rs/typst/latest/typst/trait.World.html>
- typst.ts VFS（备选方案）：<https://github.com/Myriad-Dreamin/typst.ts/discussions/376>
- dompurify 已在依赖（`3.4.12`），SVG 净化直接用

---

## 4. 功能 B：Trilium 风格 Softlinks

**状态**：⏳ 待开工

### 选型（已定）

**frontmatter `_clones:` 关系 + 虚拟节点物化，无 SQLite**

为何不用 SQLite：ADR-0002 是有条件禁（禁 DB 作主存，允许"可删除重建的派生索引"）。Trilium 用 SQLite 是因为 Trilium 的 note 不是文件；Tolaria 的 note 是文件。权威 SQLite 违反 0002，派生 SQLite 又与现有 JSON cache 重复。symlink 被 scanner `follow_links(true)` 扫成重复 entry，破坏 path-as-identity（ADR-0035），同样否决。**无需修订 ADR-0002**。

### 数据模型（仿 Trilium branch-on-parent）

容器笔记 frontmatter 持有 edge（`_`-前缀系统字段，ADR-0008 约定）：

```yaml
---
title: Q3 Review
type: Project
_clones:
  - "[[People/Alice]]"
  - "[[Teams/Sales]]"
---
```

tree builder 物化虚拟 clone 子节点（无文件），点击跳转本体；本体编辑即所有 clone 看到的内容（单一真相）。

### 落点

**Rust**：
- `src-tauri/src/vault/entry.rs`：`VaultEntry` 加 `clones: Vec<String>`
- `src-tauri/src/vault/cache.rs:25`：`CACHE_VERSION` 14 → 15（携带派生 clone 索引）
- tree 物化：`scan_vault_folders` 或新命令按 `clones` 在容器目录下生成虚拟 `FolderNode`

**前端**：
- `src/types.ts:295 SidebarSelection` 加 clone 变体
- `src/components/FolderTree.tsx` + `folder-tree/FolderTreeRow.tsx`：渲染 clone 节点 + Trilium 式角标
- 删除语义：从 `_clones:` 移除一项 = 删该位置（Trilium delete-branch）；删本体 = 永久删除（ADR-0045），clone 变悬空 stub
- 重命名：本体改名骑现有 wikilink-retarget（ADR-0036/0075）
- wikilink 解析（`src/utils/wikilink.ts`）无需改

### 交付清单

- [ ] ADR-0172：Softlinks 数据模型（`_clones:` + 虚拟节点 + `CACHE_VERSION` 升级）
- [ ] `VaultEntry` 加 `clones` 字段 + frontmatter 解析
- [ ] tree 物化虚拟 `FolderNode`
- [ ] 前端 `FolderTree` clone 节点渲染 + 角标
- [ ] 删除/重命名语义
- [ ] `cargo test`（关系解析 + tree 物化）+ vitest（前端 tree 渲染、wikilink 不回归）
- [ ] 本地化 + PostHog（`softlink_created`/`softlink_removed`）
- [ ] native QA 截图

---

## 5. 上游同步策略

- **日常**：`git fetch upstream && git merge upstream/main`，处理冲突后 push 到 origin
- **冲突高发区**：`EditorContentLayout.tsx`、`FilePreview.tsx`、`vault/mod.rs:329 classify_file_kind`、`FolderTree.tsx`、`Sidebar.tsx`、`FolderNode`
- 改动尽量**独立新文件**（Typst 渲染独立组件、softlink 关系独立解析模块），降低与上游热区的冲突面
- 上游是私有仓库，无法预知 CI 状态；同步后必须本地验证（`pnpm test`、`cargo check`）

### 已知坑：上游 CI failing 时 merge

上游 `41d4ac53`（2026-07-21）这一刻 CI 是红的，`src/App.test.tsx` 的 `auto-advances to the next inbox item` 测试在上游就 failing。merge 后这个测试在本地也 failing。

应对方案（待定）：
- 选项 A：本地 pre-push 加 `LAPUTA_FORK_SYNC` 跳过开关（仅同步上游时跳 build/test gates）
- 选项 B：上游 failing 时延后同步，等上游修了再 merge
- 选项 C：fork 自己修这个测试（但要区分"上游引入的 bug"和"上游的测试本身有问题"）

---

## 6. 验证清单（每个功能完成前）

- `pnpm lint && npx tsc --noEmit && pnpm test && pnpm test:coverage`（≥70%）
- `cargo test && cargo llvm-cov --manifest-path src-tauri/Cargo.toml --no-clean --fail-under-lines 85`
- CodeScene 文件级 before/after（如启用）；新文件 10.0；触过文件不得变差
- Codacy：无新 Critical/High（pre-push 自动跑）
- 本地化：`pnpm l10n:translate && pnpm l10n:validate`
- Playwright smoke（仅触核心流程）
- native QA：`pnpm tauri dev` + 截图
- `git status --short -- demo-vault demo-vault-v2` 干净

---

## 7. 执行顺序

1. ✅ 环境（工具链、磁盘重定向、infisical、git remote）
2. ✅ origin 重定向到 GitHub fork + 废除 ADO pipeline 67
3. ✅ 本地 main 与 GitHub fork main 一致（reset --hard）
4. ⏳ **TODO.md + .gitignore `.infisical.json`** → commit + push（当前）
5. ⏳ Typst 预览：brainstorming → spec → plan → 实现（§3）
6. ⏳ Softlinks：brainstorming → spec → plan → 实现（§4）
7. ⏳ ADR-0171（Typst）/ 0172（softlinks）跟随各自 commit

---

## 8. 待决问题（已决）

- [x] **GitHub Actions**：暂不启用。基础功能完成后再考虑。
- [x] **CI**：暂时不启用。基础功能完成后再考虑。
- [x] **CodeScene**：不启用。
- [ ] 上游 CI failing 时同步策略选 A/B/C？（基础功能完成后再定）
