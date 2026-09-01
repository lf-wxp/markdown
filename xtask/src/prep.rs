//! 建索引前对 mdBook 产物的处理。

use std::fs;
use std::io;
use std::path::Path;

/// 给 HTML 的 `<main>` 标签加上 `data-pagefind-ignore="all"`
///
/// 导航页（首页与各章节 index）只是链接汇总，且首页列举了各类关键词，
/// 会命中几乎所有查询造成噪音，因此在建索引前标记为忽略。
pub fn mark_ignored(html: &str) -> Option<String> {
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

pub fn run(root: &Path) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
