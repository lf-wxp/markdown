//! 知识库构建辅助工具。
//!
//! 子命令：
//! - `summary`      生成 `src/SUMMARY.md`
//! - `prep-index`   建索引前处理构建产物（移除 print.html、标记导航页为不索引）
//! - `serve`        启动本地静态预览服务
//!
//! `summary` 的设计原则：**增量、幂等、不破坏手工排序**
//!
//! - 已在 `SUMMARY.md` 中出现的条目保持原有顺序不变
//!   （`rust/` 按「由浅入深」、`philosophy/` 按哲学史时间线手工排列，不能被字母序覆盖）
//! - 磁盘上新增的文件追加到所属分区末尾并在输出中提示
//! - 磁盘上已删除的文件从 `SUMMARY.md` 中移除并提示
//! - 章节标题优先取文件内的一级标题（`# xxx`），回退到文件名

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// 分区定义：(目录名, 侧边栏 part 标题, 章节页显示名)
const SECTIONS: &[(&str, &str, &str)] = &[
    ("english", "English", "English"),
    ("philosophy", "Philosophy", "Philosophy"),
    ("rust", "Rust", "Rust"),
    ("clipping", "Clipping", "Clipping"),
];

/// 不作为独立章节收录的文件
const EXCLUDE: &[&str] = &["SUMMARY.md", "README.md", "index.md"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let check = args.iter().any(|a| a == "--check");

    match cmd {
        "summary" => match run_summary(check) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        "prep-index" => match run_prep_index() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        "serve" => {
            let port = args
                .iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(8000);
            match run_serve(port) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            print_help();
            ExitCode::SUCCESS
        }
    }
}

fn print_help() {
    println!(
        "\
知识库构建辅助工具

用法:
    cargo xtask summary            生成 src/SUMMARY.md
    cargo xtask summary --check    校验 SUMMARY.md 是否最新（CI 用）
    cargo xtask prep-index         建索引前处理 book/ 产物
    cargo xtask serve [--port N]   启动本地预览（默认 8000）

通常不需要直接调用，请使用: cargo make build / serve / check
"
    );
}

/// 项目根目录：xtask 的父目录
fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask 应位于项目根目录下")
        .to_path_buf()
}

/// 读取文件的一级标题（`# xxx`）
fn read_h1(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("# ").map(|t| t.trim().to_string())
    })
}

/// 章节显示名：优先文件内 H1，回退文件名
fn display_title(path: &Path) -> String {
    read_h1(path).unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// 从现有 SUMMARY.md 中提取某分区已有的相对路径顺序
///
/// 解析形如 `- [标题](./rust/xxx.md)` 的行，取出括号中的路径部分。
fn existing_order(summary: &str, section: &str) -> Vec<String> {
    let prefix = format!("{section}/");
    let index_path = format!("{prefix}index.md");
    let mut order = Vec::new();

    for line in summary.lines() {
        let Some(rel) = extract_link_path(line) else {
            continue;
        };
        if rel.starts_with(&prefix) && rel != index_path && !order.contains(&rel) {
            order.push(rel);
        }
    }
    order
}

/// 从一行 Markdown 中抽出 `](./path)` 里的 path
///
/// 不使用正则以免引入依赖；标题中可能含 `]`（如 `ref 和 &`），
/// 因此从右侧定位 `](./` 更稳妥。
fn extract_link_path(line: &str) -> Option<String> {
    let start = line.rfind("](./")? + 4;
    let rest = &line[start..];
    let end = rest.find(')')?;
    Some(rest[..end].to_string())
}

/// 扫描磁盘上某分区的所有 md 文件（不含 index.md）
fn scan(src: &Path, section: &str) -> io::Result<Vec<String>> {
    let dir = src.join(section);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    // BTreeSet 保证扫描结果稳定有序，使同一份磁盘状态总产出一致的输出
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if EXCLUDE.contains(&name) {
            continue;
        }
        files.insert(format!("{section}/{name}"));
    }
    Ok(files.into_iter().collect())
}

struct BuildResult {
    content: String,
    added: Vec<String>,
    removed: Vec<String>,
}

fn build(src: &Path, current: &str) -> io::Result<BuildResult> {
    let mut lines: Vec<String> = vec![
        "# Summary".into(),
        String::new(),
        "[Introduction](./README.md)".into(),
        String::new(),
        "---".into(),
        String::new(),
    ];
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for (section, part_title, index_title) in SECTIONS {
        let on_disk = scan(src, section)?;
        let index_path = src.join(section).join("index.md");
        if on_disk.is_empty() && !index_path.exists() {
            continue;
        }

        let prior = existing_order(current, section);
        // 保留已有顺序（过滤掉磁盘上已不存在的），再追加新增文件
        let mut ordered: Vec<String> =
            prior.iter().filter(|r| on_disk.contains(r)).cloned().collect();
        removed.extend(prior.iter().filter(|r| !on_disk.contains(r)).cloned());

        let new_items: Vec<String> = on_disk
            .iter()
            .filter(|r| !ordered.contains(r))
            .cloned()
            .collect();
        added.extend(new_items.iter().cloned());
        ordered.extend(new_items);

        lines.push(format!("# {part_title}"));
        lines.push(String::new());

        let indent = if index_path.exists() {
            lines.push(format!("- [{index_title}](./{section}/index.md)"));
            "  "
        } else {
            ""
        };

        for rel in &ordered {
            let title = display_title(&src.join(rel));
            lines.push(format!("{indent}- [{title}](./{rel})"));
        }
        lines.push(String::new());
    }

    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.push(String::new()); // 以单个换行结尾

    Ok(BuildResult {
        content: lines.join("\n"),
        added,
        removed,
    })
}

/// 返回 Ok(true) 表示成功/无差异，Ok(false) 表示 --check 模式下发现差异
fn run_summary(check: bool) -> io::Result<bool> {
    let root = project_root();
    let src = root.join("src");
    let summary_path = src.join("SUMMARY.md");

    if !src.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("找不到源目录: {}", src.display()),
        ));
    }

    let current = fs::read_to_string(&summary_path).unwrap_or_default();
    let result = build(&src, &current)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if check {
        if result.content != current {
            writeln!(out, "SUMMARY.md 已过期，请运行: cargo xtask summary")?;
            for rel in &result.added {
                writeln!(out, "  + {rel}")?;
            }
            for rel in &result.removed {
                writeln!(out, "  - {rel}")?;
            }
            return Ok(false);
        }
        writeln!(out, "SUMMARY.md 已是最新")?;
        return Ok(true);
    }

    if result.content == current {
        writeln!(out, "SUMMARY.md 无需更新")?;
        return Ok(true);
    }

    fs::write(&summary_path, &result.content)?;
    writeln!(out, "已更新 src/SUMMARY.md")?;
    for rel in &result.added {
        writeln!(out, "  + 新增 {rel}")?;
    }
    for rel in &result.removed {
        writeln!(out, "  - 移除 {rel}")?;
    }
    Ok(true)
}

/// 给 HTML 的 `<main>` 标签加上 `data-pagefind-ignore="all"`
///
/// 导航页（首页与各章节 index）只是链接汇总，且首页列举了各类关键词，
/// 会命中几乎所有查询造成噪音，因此在建索引前标记为忽略。
fn mark_ignored(html: &str) -> Option<String> {
    // 已处理过则跳过，保证幂等
    if html.contains("data-pagefind-ignore") {
        return None;
    }
    let idx = html.find("<main")?;
    let after = idx + "<main".len();
    // 只处理 `<main>` 或 `<main ...>`，避免误伤 <mainsomething>
    let next = html[after..].chars().next()?;
    if next != '>' && !next.is_whitespace() {
        return None;
    }
    let mut out = String::with_capacity(html.len() + 32);
    out.push_str(&html[..after]);
    out.push_str(" data-pagefind-ignore=\"all\"");
    out.push_str(&html[after..]);
    Some(out)
}

fn run_prep_index() -> io::Result<()> {
    let root = project_root();
    let book = root.join("book");
    if !book.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "未找到 book/ 目录，请先执行 mdbook build",
        ));
    }

    // print.html 是 mdBook 生成的「全书合并页」，若被索引会导致：
    //   1) 标签计数翻倍（每个标签在合并页中重复出现一次）
    //   2) 搜索结果混入一条聚合了所有内容的噪音条目
    let print_page = book.join("print.html");
    if print_page.exists() {
        fs::remove_file(&print_page)?;
        println!("  已移除 print.html（避免污染搜索索引）");
    }

    // 收集首页与各一级子目录下的 index.html
    let mut targets = vec![book.join("index.html")];
    for entry in fs::read_dir(&book)? {
        let path = entry?.path();
        if path.is_dir() {
            let idx = path.join("index.html");
            if idx.is_file() {
                targets.push(idx);
            }
        }
    }
    targets.sort();

    let mut marked = 0;
    for path in &targets {
        if !path.is_file() {
            continue;
        }
        let html = fs::read_to_string(path)?;
        if let Some(new) = mark_ignored(&html) {
            fs::write(path, new)?;
            marked += 1;
        }
    }
    println!("  已标记 {marked} 个导航页为不索引");

    // 说明：GitHub Pages 默认经 Jekyll 处理会跳过 _ 开头的目录（Pagefind 索引在
    // _pagefind/ 下），需要 .nojekyll 才能访问。mdBook 已自动生成该文件，此处无需处理。
    debug_assert!(book.join(".nojekyll").exists(), "mdBook 应已生成 .nojekyll");
    Ok(())
}

/// 极简静态文件服务器，仅用于本地预览
///
/// 不使用 `mdbook serve`：它会重建 book/ 并清除 Pagefind 索引，导致搜索失效。
fn run_serve(port: u16) -> io::Result<()> {
    let root = project_root().join("book");
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "未找到 book/ 目录，请先执行 cargo make build",
        ));
    }

    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("预览地址: http://127.0.0.1:{port}  (Ctrl-C 退出)");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let root = root.clone();
                // 单线程逐个处理即可满足本地预览；出错不中断服务
                if let Err(e) = handle_request(s, &root) {
                    if e.kind() != io::ErrorKind::BrokenPipe {
                        eprintln!("请求处理失败: {e}");
                    }
                }
            }
            Err(e) => eprintln!("连接失败: {e}"),
        }
    }
    Ok(())
}

fn handle_request(mut stream: TcpStream, root: &Path) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }

    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let decoded = percent_decode(path.split('?').next().unwrap_or("/"));

    let mut rel = decoded.trim_start_matches('/').to_string();
    if rel.is_empty() || rel.ends_with('/') {
        rel.push_str("index.html");
    }

    // 阻断路径穿越
    let target = root.join(&rel);
    let safe = target
        .canonicalize()
        .ok()
        .filter(|p| p.starts_with(root.canonicalize().unwrap_or_else(|_| root.to_path_buf())));

    match safe.and_then(|p| fs::read(&p).ok().map(|b| (p, b))) {
        Some((p, body)) => {
            let ctype = content_type(&p);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
                body.len()
            )?;
            stream.write_all(&body)?;
        }
        None => {
            let body = b"404 Not Found";
            write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )?;
            stream.write_all(body)?;
        }
    }
    stream.flush()
}

/// 解码 URL 中的 %XX 转义（中文文件名必需）
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        // Pagefind 的索引分片，需按二进制返回
        Some("pf_index") | Some("pf_fragment") | Some("pf_meta") => "application/octet-stream",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_link_path() {
        assert_eq!(
            extract_link_path("  - [所有权](./rust/所有权.md)").as_deref(),
            Some("rust/所有权.md")
        );
    }

    /// 标题中含 `]` 或 `&` 时仍能正确解析（如 `ref 和 &`）
    #[test]
    fn extracts_link_path_with_special_title() {
        assert_eq!(
            extract_link_path("  - [ref 和 &](./rust/ref和&.md)").as_deref(),
            Some("rust/ref和&.md")
        );
        assert_eq!(
            extract_link_path("  - [a]b](./rust/x.md)").as_deref(),
            Some("rust/x.md")
        );
    }

    #[test]
    fn ignores_non_link_lines() {
        assert_eq!(extract_link_path("# Rust"), None);
        assert_eq!(extract_link_path(""), None);
    }

    #[test]
    fn preserves_manual_order() {
        let summary = "\
# Rust

- [Rust](./rust/index.md)
  - [Tips](./rust/tips.md)
  - [所有权](./rust/所有权.md)
";
        // 顺序应与 SUMMARY 中一致，而非字母序
        assert_eq!(
            existing_order(summary, "rust"),
            vec!["rust/tips.md", "rust/所有权.md"]
        );
    }

    /// index.md 不应作为普通条目进入顺序列表
    #[test]
    fn skips_index_in_order() {
        let summary = "- [Rust](./rust/index.md)\n  - [Tips](./rust/tips.md)\n";
        assert_eq!(existing_order(summary, "rust"), vec!["rust/tips.md"]);
    }

    #[test]
    fn marks_main_tag() {
        let html = r#"<body><main class="x">hi</main></body>"#;
        let out = mark_ignored(html).expect("应插入属性");
        assert!(out.contains(r#"<main data-pagefind-ignore="all" class="x">"#));
    }

    /// 重复执行不应叠加属性（幂等）
    #[test]
    fn mark_is_idempotent() {
        let html = r#"<main data-pagefind-ignore="all">hi</main>"#;
        assert!(mark_ignored(html).is_none());
    }

    /// 不应误伤名字以 main 开头的其他标签
    #[test]
    fn does_not_match_similar_tag() {
        assert!(mark_ignored("<mainframe>x</mainframe>").is_none());
    }

    #[test]
    fn decodes_percent_encoding() {
        assert_eq!(percent_decode("/rust/%E6%89%80%E6%9C%89%E6%9D%83.html"), "/rust/所有权.html");
        assert_eq!(percent_decode("/plain.html"), "/plain.html");
    }

    #[test]
    fn maps_pagefind_content_types() {
        assert_eq!(content_type(Path::new("a.pf_index")), "application/octet-stream");
        assert_eq!(content_type(Path::new("a.html")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("a.js")), "text/javascript; charset=utf-8");
    }
}
