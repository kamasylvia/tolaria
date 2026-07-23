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

**状态**：✅ 已完成（2026-07-23，已 push 到 origin/main `464f00ad`）

### 实际实现（tinymist-world 集成，ADR-0171 选型的演进）

原选型（Option 3：typst crate 自实现 World）在实现中演变为**委托 tinymist-world**——后者已成熟解决字体/包/VFS/诊断，自实现属重复造轮。typst_preview.rs 从设计稿的 ~80 行 World + 手写字体/包逻辑，收敛为 189 行薄壳。

- **Rust**：`src-tauri/src/commands/typst_preview.rs`（189 行）—— `#[tauri::command] render_typst(path, vault_path, main_path)` 调 `tinymist_world::args::CompileOnceArgs::resolve_system()` → `snapshot()` → `typst::compile::<PagedDocument>` → `typst_svg::svg_merged`（12pt 页间距）
- **依赖**：`typst/typst-svg/typst-layout/typst-assets 0.15.1` + `tinymist-world 0.15.0 (features=["system"])`。`system` 拉 fontdb/reqwest/lsp-types/clap/rayon（重依赖，一次性编译）
- **前端**：`src/components/TypstPreview.tsx` —— invoke `render_typst` → DOMPurify sanitize（保留 `<use>`/`xlink:href`，typst 只输出内部 `#frag`）→ iframe `srcDoc`（灰底 + svg drop-shadow + 12pt gap 分页视觉）
- **字体/包**：全交 tinymist（SystemFontSearcher 算 FontInfo 后丢 fontdb；HttpRegistry 下拉 + 缓存 @preview/@local 包）
- **file kind**：`src-tauri/src/vault/mod.rs classify_file_kind` `.typ`/`.typst` → `"typst"`
- **测试**：`cargo test --lib commands::typst_preview`（3：single-file SVG、CJK distinct-glyph、error diagnostics）；`TypstPreview.test.tsx`（6：含 `<use>` 保留 regression）

关键 commit：`ee248111`（tinymist 集成）、`5dd8990b`（push 含 DOMPurify/分页/folder toggle）、`464f00ad`（docs）。详见 git log。

### 待办（ Typst 预览后续增强，未做）

- [ ] **入口锚点完善**：当前只支持 `main_path` 显式 + 单文件（path 自身）。设计稿的 4 层里缺：①frontmatter `typst_root:` 提示 ②自动探测同目录 `main.typ` ③会话手动 pin UI。tinymist 的 `EntryOpts` 已支持 root/main 配置，接入即可
- [ ] **增量 live preview**：当前每次全量 compile。tinymist-preview actor crate（WebSocket + IncrSvgDocServer）可做增量流式，但过重，按需评估
- [ ] **本地化**：TypstPreview 错误/加载文案未走 `src/lib/locales/en.json`（当前硬编码英文）
- [ ] **PostHog**：`typst_preview_opened` / `typst_root_pinned` 事件未埋点
- [ ] **search 查询 × 树**：folder toggle 与 search 的交互（命中笔记自动展开祖先 folder）未做

### 关键参考

- **ADR-0171**（Typst 预览渲染选型）—— 原选型记录，实际实现是其演进（自实现 → tinymist-world）
- **ADR-0168**（Sandboxed standalone HTML file previews）—— iframe srcDoc + DOMPurify 模式参考
- tinymist World API：`CompileOnceArgs::resolve_system` / `SystemUniverseBuilder`（crates/tinymist-world/src/system.rs）

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
