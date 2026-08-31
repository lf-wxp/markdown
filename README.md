# markdown

个人知识库，基于 [mdBook](https://rust-lang.github.io/mdBook/) 构建，内容涵盖 English / Philosophy / Rust / Kindle Clipping。

## 目录结构

```
Cargo.toml        # workspace 定义
Makefile.toml     # cargo-make 任务定义（构建入口）
book.toml         # mdBook 配置
xtask/            # Rust 构建工具：SUMMARY 生成 / 索引预处理 / 预览服务
theme/
  typography.css  # 正文排版样式（标题层次 / 代码块 / 表格 / 引用块）
  pagefind.css    # 搜索界面样式，同时定义共用的颜色变量
  pagefind-init.js# 搜索前端逻辑
src/
  SUMMARY.md      # 侧边栏目录，由 xtask 生成，不要手工新增条目
  README.md       # 首页
  english/        # 英语笔记
  philosophy/     # 哲学笔记
  rust/           # Rust 笔记
  clipping/       # Kindle 摘录
```

## 环境要求

只需 Rust 工具链，**无 Python / Node.js 依赖**。

```sh
cargo install cargo-make   # 任务运行器
cargo make setup           # 安装 mdbook / mdbook-toc / pagefind
```

## 构建与预览

```sh
cargo make build     # 构建到 book/
cargo make serve     # 构建后启动预览 http://127.0.0.1:8000
cargo make check     # CI 校验：目录是否最新 + 构建是否有警告
cargo make summary   # 仅重新生成 SUMMARY.md
cargo make clean     # 清理 book/
```

预览端口可覆盖：`SERVE_PORT=9000 cargo make serve`

> 不要使用 `mdbook serve`：它会重建 `book/` 并清除 Pagefind 索引，导致搜索失效。
> `cargo make serve` 使用 xtask 内置的静态服务器，不会破坏索引。

开发 xtask 本身：

```sh
cargo make test      # 单元测试
cargo make lint      # clippy
```

## GitHub Pages 部署

`.github/workflows/deploy.yml` 会在 push 到 `main` 时自动构建并发布到
`https://lf-wxp.github.io/markdown/`，也可在 Actions 页手动触发（`workflow_dispatch`）。

### 首次启用（必须手动做一次）

**Settings → Pages → Build and deployment → Source** 选择 **GitHub Actions**

未启用时 `deploy` 阶段会失败并报：

```
Error: Failed to create deployment (status: 404)
Ensure GitHub Pages has been enabled
```

设置好后对失败的运行点 **Re-run jobs** 即可。

> 这一步无法自动化。`actions/configure-pages` 虽有 `enablement: true` 参数，
> 但实测 `GITHUB_TOKEN` 无权创建 Pages 站点，会报
> `Create Pages site failed. Resource not accessible by integration`。

> 日志中的 `Node.js 20 is deprecated ... actions/deploy-pages@v4` 属于弃用警告，
> GitHub 会自动转用 Node 24 执行，不影响部署。等官方发布 v5 后再升级即可。

工作流包含的质量门禁：

1. `cargo test --package xtask` —— 10 个单元测试
2. `xtask summary --check` —— 目录未更新则失败，避免新笔记漏出现在侧边栏
3. mdbook 输出含 `WARN`/`ERROR` 即失败 —— 拦截「泛型未加反引号导致内容丢失」这类问题
4. 校验 pagefind 为 Extended 版 —— 否则中文搜索会失效
5. 校验 `_pagefind/pagefind.js` 与 `.nojekyll` 存在

### 子路径部署的两个坑

Pages 部署在 `/<repo>/` 子路径下，有两处需要处理（均已实测通过）：

1. **`.nojekyll`**：Pages 默认经 Jekyll 处理会跳过 `_` 开头的目录，
   而搜索索引正在 `_pagefind/` 下。mdBook 已自动生成该文件，CI 中有校验。
2. **`site-url`**：`book.toml` 中设为 `/markdown/`。mdBook 会据此给
   `404.html` 注入 `<base href="/markdown/">`，否则访问深层不存在的 URL 时
   404 页的 css/js 会取不到。普通页面不受影响（仍用相对路径），
   因此本地与 Docker 部署照常工作。

   > 若改用自定义域名或部署到根路径，把 `site-url` 改为 `/`。

搜索脚本通过 mdBook 注入的 `path_to_root` 推导站点根，已在模拟子路径
环境下验证：索引可加载、结果链接自动带 `/markdown/` 前缀。

## Docker 部署

无需本地安装任何工具链，构建在容器内完成。

```sh
# Compose v2（docker compose）或 v1（docker-compose）均可
docker compose up -d --build            # 构建并启动，访问 http://localhost:8080
NOTES_PORT=9000 docker compose up -d    # 自定义端口
docker compose down                     # 停止
```

或直接用 docker：

```sh
docker build -t notes .
docker run -d -p 8080:80 --name notes notes
```

镜像特性：

- **多阶段构建**：运行阶段只有 nginx + 3.3M 静态产物，最终镜像 **80.7MB**
- **平台无关**：工具安装交由 [cargo-binstall](https://github.com/cargo-bins/cargo-binstall)
  自动探测当前平台并匹配官方预编译发布物，Dockerfile 中**不含任何架构判断**。
  amd64 / arm64 通用，多平台构建直接用 buildx：

  ```sh
  docker buildx build --platform linux/amd64,linux/arm64 -t notes .
  ```

- **构建即质检**：mdbook 输出若含 `WARN`/`ERROR` 会中止构建，
  避免把内容丢失的产物打进镜像
- **只读根文件系统**：compose 配置了 `read_only: true` + tmpfs，已实测可正常服务
- **健康检查**：内置 `HEALTHCHECK`，`docker inspect` 可见 `healthy`

### nginx 配置要点

`docker/nginx.conf` 有两处踩过坑的地方，改动时请留意：

1. **不要在 server 内使用 `types` 块**。server 级别的 `types` 会**完全替换**
   全局 `mime.types` 而非追加，结果连 `.html` 都退化成
   `application/octet-stream`，浏览器直接下载而不渲染。
   Pagefind 的 `.pf_index` / `.pf_fragment` / `.pf_meta` 不在默认表中，
   会落到 `default_type`，恰好就是需要的 `application/octet-stream`，无需声明。
   （`types { include mime.types; }` 也不行，该文件自身带 `types` 包裹会语法报错。）
2. **`charset_types` 中不要列 `text/html`**，它默认已包含，
   重复声明会在启动时报 `duplicate MIME type` 警告。

本站有 43 个中文名 HTML，依赖 `charset utf-8` 与 nginx 的百分号解码。

缓存策略：HTML 用 `no-cache` 保证更新即时可见；`_pagefind/` 与带哈希的
静态资源用长期缓存。

## 搜索说明

mdBook 自带搜索使用 elasticlunr，按空白字符分词，**中文内容完全无法索引**
（实测搜索索引中 CJK token 数为 0）。因此本项目禁用自带搜索，改用
[Pagefind](https://pagefind.app/) 在构建后生成索引。

- 页面上按 `s` 或 `Cmd/Ctrl+K` 唤起搜索，`Esc` 关闭
- 索引仅覆盖正文（`--root-selector main`），不含侧边栏
- 导航页（首页与各章节 index）被标记为不索引：它们只是链接汇总，
  且首页列举了各类关键词，会命中几乎所有查询
- `print.html`（全书合并页）已关闭：它会导致标签计数翻倍并混入聚合噪音

### 必须使用 Extended 版 pagefind

```sh
cargo install pagefind --features extended --locked
```

`extended` feature 提供 CJK 分词。**不带该 feature 的标准版会使中文搜索
基本不可用**，实测对比：

| 查询 | 标准版 | Extended 版 |
| --- | --- | --- |
| 三体 | **0 条** | 5 条（首条《三体三部曲》） |
| 意义 | **0 条** | 8 条（首条《人生的意义》） |
| 智能指针 | 1 条 | 3 条 |
| 哲学 | 7 条 | 12 条 |
| 索引词数 | 6950（按字切分） | 5729（正确分词） |

唯一取舍是「所有权」的首条为《薛兆丰经济学讲义》而非 `rust/所有权`
（目标页在 26 条结果内）。

Extended 通过 cargo feature 提供，**不需要 Node.js**。
`cargo make build` 会在构建前自动校验，装错版本会直接报错并给出修复命令。

> 检测方式提示：`pagefind --version` 只输出 `pagefind 1.5.2`，不含 Extended
> 标识；只有实际运行时的横幅会打印 `Running Pagefind v1.5.2 (Extended)`。

已知限制：Pagefind 对 `zh-cn` 不做词干还原，无法跨词根匹配。

### 标签筛选

`src/clipping/` 下每篇摘录顶部有一行元数据，同时承担两个作用：

```html
<div class="clipping-meta">
  <span data-pagefind-filter="分类">哲学</span>
  <span data-pagefind-filter="标签">哲学</span>
  <span data-pagefind-filter="作者">韩炳哲</span>
</div>
```

- 页面上渲染为徽标，便于快速识别书目类型
- 被 Pagefind 识别为筛选维度（分类 / 标签 / 作者）

搜索时结果上方会出现标签筛选条，点击即可过滤，再次点击取消。
新增摘录时按同样格式添加即可自动进入筛选维度。

## 目录维护

`src/SUMMARY.md` 由 `xtask` 生成（`cargo make summary`），特性：

- **保留手工顺序**：已有条目顺序不变
  （`rust/` 按由浅入深、`philosophy/` 按哲学史时间线排列，不会被字母序覆盖）
- **自动收录新增**：新文件追加到所属分区末尾并打印提示
- **自动移除失效**：文件删除后对应条目一并移除
- **标题取自 H1**：优先使用文件内的一级标题，回退到文件名
- **幂等**：重复执行结果一致，可安全放进 CI

新增笔记后执行 `cargo make build` 即可，无需手工改 `SUMMARY.md`。
若想调整章节顺序，直接编辑 `SUMMARY.md`，后续生成会保持你的顺序。

## 写作约定

- 每篇文章只保留一个一级标题（`#`），其余用 `##` / `###` 逐级下沉
  （xtask 依赖 H1 生成侧边栏标题）
- **标题要短**（建议 30 字符内）。标题会进侧边栏与页内目录，
  用整句长文本当标题会把导航撑爆（曾用整句英文例句作 H4，导致侧边栏不可用）
- 正文中出现泛型、标签等内容（如 `Rc<T>`、`Box<dyn Trait>`）必须用反引号包裹，
  否则会被解析成 HTML 标签导致内容丢失
- **反引号只用于代码**。人名、书名等用 `**加粗**`；反引号会被渲染成
  带底色描边的行内代码，语义与视觉都不对
- **裸 HTML 块（如 `<div>`）后必须留一个空行**，否则紧随其后的 Markdown
  不会被解析（曾导致 23 篇摘录的引用块渲染成字面的 `>` 符号）
- **列表标记必须是 `-` 或 `*`**。曾有 6 个文件用 `_` 或 `\*`，
  34 行列表全部渲染成字面符号
- 提交前执行 `cargo make check`，确保无 `WARN` / `ERROR`

## 排版说明

`theme/typography.css` 在 mdBook 默认主题上强化结构感，针对本项目实际使用的
元素设计（引用块 2065 行、无序列表 361、行内代码 225、表格 94 行、
代码块 62、H2/H3/H4 共 161、`<mark>` 29 处）：

| 元素 | 处理 |
| --- | --- |
| H1 | 底部强调色渐变条，页面开头的视觉锚点 |
| H2 | 表面底色 + 左侧粗色条，长文档里明确切分章节 |
| H3 | 左侧细色条，与 H2 的实底块形成层级差 |
| 引用块 | 表面底色 + 左侧色条卡片；摘录时间戳弱化为脚注 |
| 代码块 | **表面底色** + 边框圆角；可运行块加强调色左边框 |
| 行内代码 | 表面底色 + 强调色文字，在中文正文里可被立刻辨识 |
| 表格 | 表头强表面色 + 强调色下边线、斑马纹、悬停高亮 |
| 列表 | 项目符号用强调色并加大 |
| 加粗 | 混入强调色，让关键结论在长段落中跳出 |
| `<mark>` | 下半部色带（荧光笔效果），不用实色块 |

### 色阶体系

变量定义在 `theme/pagefind.css` 中，默认值写在 `:root`，各主题用
`html.navy` / `html.coal` / `html.ayu` / `html.rust` 覆盖：

| 变量 | 用途 |
| --- | --- |
| `--pf-muted` | 次要文字（说明、时间戳、计数） |
| `--pf-border` | 分隔线与描边 |
| `--pf-surface` | 浮起表面（代码块、引用块、斑马纹） |
| `--pf-surface-2` | 更强表面（表头、悬停） |
| `--pf-accent` | 强调色，用于色条、边框等「面」元素 |
| `--pf-accent-fg` | 强调色的文字版，需在表面色上达到 4.5:1 |

5 个主题下的实测对比度（WCAG AA 要求 ≥ 4.5）：

| 主题 | 正文/页面 | 正文/表面 | 行内代码/表面 | 表面落差 |
| --- | --- | --- | --- | --- |
| light | 21.0 | 19.06 | 8.41 | 1.10 |
| rust | 11.54 | 10.93 | 7.67 | 1.06 |
| coal | 7.13 | 6.24 | 8.40 | 1.14 |
| navy | 9.46 | 7.91 | 7.69 | 1.20 |
| ayu | 10.72 | 9.17 | 9.45 | 1.17 |

维护时的几条经验：

- **主题选择器必须写 `html.<theme>`，不能写 `body.<theme>`**。
  mdBook 把主题 class 挂在 `<html>` 上（`<html class="navy ...">`），
  `<body>` 没有任何 class。写成 `body.navy` 永远不匹配，暗色主题会继续
  使用浅色表面值，造成「浅底 + 浅字」完全不可读（表格斑马纹曾因此不可读）
- **表面色必须与背景有落差**（比值 > 1.05）。mdBook 默认代码块与页面同底色，
  只靠边框区分，是整站看起来「淡」的主因
- **强调色要区分「面」与「字」两个变量**。同一个鲜亮蓝作色条很好，
  作行内代码文字只有 4.15:1，不达标
- 不要用 `--quote-bg` 做表面色，它与页面背景过于接近（light 下几乎不可见）
- 不要用 `--searchbar-fg` / `--searchresults-header-fg`，它们是为浅色控件设计的，
  在 navy/coal 下取值为纯黑
- 不要给容器加 `opacity`，它会连带作用于子元素，导致高亮文字与背景混色
- 校验对比度时不要用 JS 改 `className` 后读 `getComputedStyle`：
  变量不会完整生效，会测出假数据。应通过 `localStorage` 设主题后**重载页面**，
  或直接离线计算色值

## 插件说明

已启用：

- **mdbook-toc**：页内目录（在文件中插入 `<!-- toc -->`）
- **playground**（mdBook 内置，非插件）：Rust 代码块在线运行，
  实测 29 个含 `fn main` 的代码块生成了运行/复制按钮

实测不兼容，未启用：

- **mdbook-admonish** 1.20.0：与 mdBook 0.5.4 配置格式不兼容，
  构建报 `invalid type: null, expected any valid TOML value` 后中断

按当前内容特征评估后判定无价值（零使用场景）：

- **mdbook-mermaid**：全库 0 处 mermaid 语法
- **mdbook-katex**：全库 0 处数学公式

## License

[MIT](./LICENSE)
