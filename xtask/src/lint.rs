//! 内容规范检查。
//!
//! README 里的写作约定每一条都是踩坑换来的，但过去只写在文档里——而约定写进
//! 文档等于没写：同一类问题已经复发过。先是 6 个文件用 `_` / `\*` 当列表标记
//! （34 行渲染成字面符号），修好之后又出现 9 个文件用 `·` 当列表标记（257 行）。
//! mdbook 对这类问题一律不报警，构建全绿，产物却是坏的。
//!
//! 这里只收录**能机械判定、且确实会导致渲染错误**的规则。
//! 拿不准的（例如「疑似表格漏了竖线」）不做，误报比漏报更消耗信任。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 这些字符出现在行首会被当成正文，而不是列表标记
///
/// `·`（间隔号）与 `•`（项目符号）多来自对话工具的富文本粘贴；
/// `_` 与 `\*` 是早先手写时的误用。
const BAD_BULLETS: &[&str] = &["·", "•", "‧", "\\*", "_"];

#[derive(Debug)]
pub struct Finding {
    pub rel: String,
    pub line: usize,
    pub rule: &'static str,
    pub msg: String,
}

impl Finding {
    fn new(rel: &str, line: usize, rule: &'static str, msg: impl Into<String>) -> Self {
        Finding {
            rel: rel.to_string(),
            line,
            rule,
            msg: msg.into(),
        }
    }
}

/// 检查单个文件，返回所有问题
pub fn lint_file(rel: &str, content: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_fence = false;
    let mut h1_lines: Vec<usize> = Vec::new();

    for (i, raw) in lines.iter().enumerate() {
        let no = i + 1;
        let trimmed = raw.trim_start();

        // 代码块内一律跳过：Rust 代码块里的 `# ` 是 mdBook 的隐藏行语法，
        // 按 H1 统计会全是误报
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        if raw.starts_with("# ") {
            h1_lines.push(no);
        }

        if let Some(marker) = bad_bullet(raw) {
            findings.push(Finding::new(
                rel,
                no,
                "bullet",
                format!("行首的 `{marker}` 不是列表标记，会渲染成字面符号，应改用 `-`"),
            ));
        }

        // 闭合标签后不留空行，紧随其后的 Markdown 不会被解析
        // （曾导致 23 篇摘录的引用块渲染成字面的 `>`）
        //
        // HTML 注释不算：注释块在 `-->` 处即结束，下一行照常按 Markdown 解析，
        // 否则每个带 `<!-- topic: -->` 的文件都会被误报。
        if trimmed.starts_with('<')
            && !trimmed.starts_with("<!--")
            && trimmed.ends_with('>')
            && lines.get(i + 1).is_some_and(|n| !n.trim().is_empty())
        {
            findings.push(Finding::new(
                rel,
                no,
                "html-blank",
                "裸 HTML 块之后需要一个空行，否则下一行 Markdown 不会被解析",
            ));
        }
    }

    match h1_lines.len() {
        // 没有 H1 时 xtask 会回退到文件名作侧边栏标题，多半不是想要的结果
        0 => findings.push(Finding::new(rel, 1, "h1", "缺少一级标题（`# 标题`）")),
        1 => {}
        n => {
            for line in &h1_lines[1..] {
                findings.push(Finding::new(
                    rel,
                    *line,
                    "h1",
                    format!("文件有 {n} 个一级标题，只应保留一个，其余下沉为 `##`"),
                ));
            }
        }
    }

    findings
}

/// 检查仓库内部的相对链接是否指向真实存在的文件
///
/// 只查内部链接。外链要联网，在 CI 里既慢又不稳定——对方限流或临时抽风都会
/// 变成一次假失败，不适合放在每次构建的必经路径上。而内部链接才是改名、拆分
/// 文件时真正会断的那一类，正好是这个仓库刚做过的事。
pub fn check_links(rel: &str, content: &str, src: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut in_fence = false;
    // 链接是相对于所在文件的目录解析的
    let dir = Path::new(rel).parent().unwrap_or(Path::new(""));

    for (i, raw) in content.lines().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        for target in extract_targets(raw) {
            let Some(path) = local_path(&target) else {
                continue;
            };
            // 以 / 开头的链接，mdBook 按 src 根目录解析
            let resolved = match path.strip_prefix('/') {
                Some(rest) => src.join(rest),
                None => src.join(dir).join(&path),
            };
            if !resolved.exists() {
                findings.push(Finding::new(
                    rel,
                    i + 1,
                    "link",
                    format!("链接指向的文件不存在：{target}"),
                ));
            }
        }
    }

    findings
}

/// 取出一行里所有 `](目标)` 形式的链接目标
fn extract_targets(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;

    while i + 1 < bytes.len() {
        if bytes[i] != b']' || bytes[i + 1] != b'(' {
            i += 1;
            continue;
        }
        let start = i + 2;
        // URL 里可能带成对括号，按深度找真正的收尾
        let mut depth = 1usize;
        let mut j = start;
        while j < bytes.len() {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if j >= bytes.len() {
            break; // 没闭合，不是链接
        }
        out.push(line[start..j].to_string());
        i = j + 1;
    }

    out
}

/// 把链接目标归一成仓库内的相对路径；外链、锚点等返回 None
fn local_path(target: &str) -> Option<String> {
    // 去掉可选标题：`[x](./a.md "标题")`
    let t = target.split_whitespace().next()?;
    let lower = t.to_ascii_lowercase();
    let external = ["http://", "https://", "mailto:", "//", "data:", "javascript:"];
    if t.starts_with('#') || external.iter().any(|p| lower.starts_with(p)) {
        return None;
    }
    // 只保留路径部分，丢掉锚点与查询串
    let path = t.split(['#', '?']).next()?;
    if path.is_empty() {
        return None;
    }
    Some(percent_decode(path))
}

/// 解开 `%E4%B8%AD` 这类转义——仓库里有中文文件名，链接可能是编码过的
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// 若该行以会被误认成列表标记的字符开头，返回那个字符
fn bad_bullet(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    BAD_BULLETS.iter().copied().find(|m| {
        trimmed
            .strip_prefix(m)
            // 后面必须跟空白，避免误伤 `_italic_`、`·` 开头的正常词语
            .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('\t'))
    })
}

/// 把行首的错误列表标记换成 `-`，保留原有缩进；无改动时返回 None
pub fn fix_content(content: &str) -> Option<String> {
    let mut changed = false;
    let mut in_fence = false;
    let mut out: Vec<String> = Vec::with_capacity(content.lines().count());

    for raw in content.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push(raw.to_string());
            continue;
        }
        match bad_bullet(raw).filter(|_| !in_fence) {
            Some(marker) => {
                let indent = raw.len() - trimmed.len();
                out.push(format!("{}-{}", &raw[..indent], &trimmed[marker.len()..]));
                changed = true;
            }
            None => out.push(raw.to_string()),
        }
    }

    if !changed {
        return None;
    }
    let mut text = out.join("\n");
    if content.ends_with('\n') {
        text.push('\n');
    }
    Some(text)
}

/// 递归收集 `src/` 下的所有 md 文件（相对路径）
fn collect(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, base, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

/// 返回 `Ok(true)` 表示没有问题
pub fn run(src: &Path, fix: bool) -> io::Result<bool> {
    let mut files = Vec::new();
    collect(src, src, &mut files)?;

    let mut findings = Vec::new();
    let mut fixed_files = 0;
    let mut fixed_lines = 0;

    for rel in &files {
        let path = src.join(rel);
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel == "SUMMARY.md" {
            continue; // 生成物
        }
        let mut content = fs::read_to_string(&path)?;

        if fix {
            if let Some(new) = fix_content(&content) {
                fixed_lines += lint_file(&rel, &content)
                    .iter()
                    .filter(|f| f.rule == "bullet")
                    .count();
                fs::write(&path, &new)?;
                content = new;
                fixed_files += 1;
            }
        }

        findings.extend(lint_file(&rel, &content));
        findings.extend(check_links(&rel, &content, src));
    }

    if fix {
        println!("已修复 {fixed_files} 个文件、{fixed_lines} 行列表标记");
    }

    if findings.is_empty() {
        println!("内容检查通过（{} 个文件）", files.len());
        return Ok(true);
    }

    let mut current = String::new();
    for f in &findings {
        if f.rel != current {
            println!("\n{}", f.rel);
            current = f.rel.clone();
        }
        println!("  {}:{} [{}] {}", f.line, " ".repeat(0), f.rule, f.msg);
    }
    println!("\n共 {} 处问题", findings.len());
    if findings.iter().any(|f| f.rule == "bullet") {
        println!("其中列表标记可自动修复: cargo xtask lint --fix");
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_bad_bullet_markers() {
        let content = "# T\n\n· 一\n  · 二\n\\* 三\n_ 四\n- 正常\n";
        let f = lint_file("a.md", content);
        let bullets: Vec<_> = f.iter().filter(|x| x.rule == "bullet").collect();
        assert_eq!(bullets.len(), 4);
        assert_eq!(bullets[0].line, 3);
        assert_eq!(bullets[1].line, 4);
    }

    /// 正文里的间隔号（人名、书名）不应被误判
    #[test]
    fn ignores_interpunct_inside_text() {
        let content = "# T\n\n就像阿瑟·叔本华所说\n《圣经·出埃及记》\n";
        assert!(lint_file("a.md", content).is_empty());
    }

    /// 下划线强调不应被误判成列表标记
    #[test]
    fn ignores_emphasis_underscore() {
        let content = "# T\n\n_强调_ 文本\n";
        assert!(lint_file("a.md", content).is_empty());
    }

    /// 代码块里的 `# ` 是 mdBook 隐藏行语法，不能算 H1
    #[test]
    fn skips_code_fences() {
        let content = "# T\n\n```rust\n# fn hidden() {}\n· not a bullet\n```\n";
        assert!(lint_file("a.md", content).is_empty());
    }

    #[test]
    fn flags_h1_count() {
        let one = lint_file("a.md", "# T\n\n## 二级\n");
        assert!(one.is_empty());

        let none = lint_file("a.md", "## 只有二级\n");
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].rule, "h1");

        let many = lint_file("a.md", "# 一\n\n# 二\n\n# 三\n");
        assert_eq!(many.len(), 2, "第一个之外的都要报");
        assert_eq!(many[0].line, 3);
        assert_eq!(many[1].line, 5);
    }

    #[test]
    fn flags_html_block_without_blank_line() {
        let bad = lint_file("a.md", "# T\n\n<div class=\"x\">y</div>\n> 引用\n");
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].rule, "html-blank");

        let good = lint_file("a.md", "# T\n\n<div class=\"x\">y</div>\n\n> 引用\n");
        assert!(good.is_empty());
    }

    /// 引用块内的 HTML（摘录里的日期行）不应被误判
    #[test]
    fn ignores_html_inside_blockquote() {
        let content = "# T\n\n> 正文\n> <p align=\"right\">2023</p>\n> 结尾\n";
        assert!(lint_file("a.md", content).is_empty());
    }

    /// 元信息注释后面紧跟 H1 是本仓库的标准写法，不能报错
    #[test]
    fn ignores_html_comment_metadata() {
        let content = "<!-- topic: 倒装结构 -->\n<!-- label: 短 -->\n# 标题\n\n正文\n";
        assert!(lint_file("a.md", content).is_empty());
    }

    #[test]
    fn fix_preserves_indent_and_trailing_newline() {
        let content = "# T\n\n· 一\n   · 嵌套\n- 正常\n";
        let fixed = fix_content(content).expect("应有改动");
        assert_eq!(fixed, "# T\n\n- 一\n   - 嵌套\n- 正常\n");
        // 修完之后应当没有问题，且再修一次不再变化（幂等）
        assert!(lint_file("a.md", &fixed).is_empty());
        assert!(fix_content(&fixed).is_none());
    }

    #[test]
    fn fix_leaves_code_fences_alone() {
        let content = "# T\n\n```\n· 代码里的点\n```\n";
        assert!(fix_content(content).is_none());
    }

    fn link_fixture() -> crate::testutil::TmpDir {
        let tmp = crate::testutil::TmpDir::new("lint-links");
        crate::testutil::write(&tmp.path().join("rust/存在.md"), "# 存在\n");
        crate::testutil::write(&tmp.path().join("english/index.md"), "# 索引\n");
        tmp
    }

    #[test]
    fn flags_only_missing_local_links() {
        let tmp = link_fixture();
        let content = concat!(
            "# T\n\n",
            "[在](./存在.md)\n",
            "[不在](./缺失.md)\n",
            "[跨目录](../english/index.md)\n",
            "[带锚点](./存在.md#小节)\n",
            "[外链](https://example.com/x.md)\n",
            "[邮件](mailto:a@b.c)\n",
            "[本页锚点](#小节)\n",
        );
        let f = check_links("rust/a.md", content, tmp.path());
        assert_eq!(f.len(), 1, "只有 ./缺失.md 应该报错：{f:?}");
        assert_eq!(f[0].line, 4);
        assert_eq!(f[0].rule, "link");
    }

    /// 仓库里有中文文件名，链接可能是 %XX 编码过的
    #[test]
    fn resolves_percent_encoded_paths() {
        let tmp = link_fixture();
        let content = "# T\n\n[编码](./%E5%AD%98%E5%9C%A8.md)\n";
        assert!(check_links("rust/a.md", content, tmp.path()).is_empty());
    }

    #[test]
    fn ignores_links_inside_code_fences() {
        let tmp = link_fixture();
        let content = "# T\n\n```md\n[示例](./完全不存在.md)\n```\n";
        assert!(check_links("rust/a.md", content, tmp.path()).is_empty());
    }

    /// 以 / 开头的链接按 src 根目录解析，不是文件系统根目录
    #[test]
    fn resolves_root_relative_from_src() {
        let tmp = link_fixture();
        let content = "# T\n\n[根相对](/english/index.md)\n";
        assert!(check_links("rust/a.md", content, tmp.path()).is_empty());
    }

    #[test]
    fn handles_parens_and_titles_in_targets() {
        assert_eq!(
            extract_targets(r#"见 [a](./x.md "标题") 与 [b](./y_(1).md)"#),
            vec!["./x.md \"标题\"", "./y_(1).md"]
        );
        assert_eq!(local_path("./x.md \"标题\"").unwrap(), "./x.md");
    }

    /// 图片也是链接，同样会因为改名失效
    #[test]
    fn checks_image_links() {
        let tmp = link_fixture();
        let content = "# T\n\n![图](./没有这张图.png)\n";
        let f = check_links("rust/a.md", content, tmp.path());
        assert_eq!(f.len(), 1);
    }
}
