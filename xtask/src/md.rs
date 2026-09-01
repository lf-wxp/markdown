//! Markdown 文件的读取与元信息解析。
//!
//! mdBook 0.5 不解析 frontmatter，因此元信息用 HTML 注释承载
//! （`<!-- topic: xxx -->`）：渲染时不可见，又便于本工具解析。

use std::fs;
use std::io;
use std::path::Path;

/// 不作为独立章节收录的文件
pub const EXCLUDE: &[&str] = &["SUMMARY.md", "README.md", "index.md"];

/// 元信息注释允许出现的行数上限：只扫描正文前几行，避免误伤正文里的注释
const META_SCAN_LINES: usize = 8;

/// 读取文件的一级标题（`# xxx`）
pub fn read_h1(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    h1_of(&content)
}

pub fn h1_of(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("# ").map(|t| t.trim().to_string())
    })
}

/// 章节显示名：优先文件内 H1，回退文件名
pub fn display_title(path: &Path) -> String {
    read_h1(path).unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// 读取文件顶部 `<!-- key: value -->` 注释里指定键的值
pub fn read_meta(path: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    read_meta_str(&content, key)
}

pub fn read_meta_str(content: &str, key: &str) -> Option<String> {
    let want = format!("{key}:");
    for line in content.lines().take(META_SCAN_LINES) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("<!--") {
            let inner = rest.trim_end_matches("-->").trim();
            if let Some(val) = inner.strip_prefix(&want) {
                return Some(val.trim().to_string());
            }
        }
    }
    None
}

/// 子文件声明的主题（`<!-- topic: xxx -->`），决定它归入索引页的哪些组
///
/// 支持用逗号分隔多个主题：跨知识点的句子（例如既是让步倒装、又是从句结构）
/// 会在每个相关主题下各出现一次，不必再被迫「只归入最核心的那个」。
pub fn read_topics(path: &Path) -> Vec<String> {
    read_meta(path, "topic")
        .map(|s| split_topics(&s))
        .unwrap_or_default()
}

pub fn split_topics(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split([',', '，']) {
        let t = part.trim();
        // 主题名里含全角括号和斜杠，但不含逗号，按逗号切分是安全的
        if t.is_empty() || out.iter().any(|x| x == t) {
            continue;
        }
        out.push(t.to_string());
    }
    out
}

/// 章节首页里跟在链接后的一句话说明（`<!-- desc: xxx -->`）
pub fn read_desc(path: &Path) -> Option<String> {
    read_meta(path, "desc").filter(|s| !s.is_empty())
}

/// 主题索引页的链接文字：优先 `<!-- label: xxx -->`，回退到文件内 H1
///
/// H1 有时是很长的原句（作侧边栏标题尚可接受），索引页需要更短的说法。
pub fn index_label(path: &Path) -> String {
    read_meta(path, "label")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| display_title(path))
}

/// 从一行 Markdown 中抽出 `](./path)` 里的 path
///
/// 不使用正则以免引入依赖；标题中可能含 `]`（如 `ref 和 &`），
/// 因此从右侧定位 `](./` 更稳妥。
pub fn extract_link_path(line: &str) -> Option<String> {
    let start = line.rfind("](./")? + 4;
    let rest = &line[start..];
    let end = rest.find(')')?;
    Some(rest[..end].to_string())
}

/// 收集某目录下的 md 文件名（不含被排除项），按数字前缀感知的顺序稳定排序
///
/// 排序只依赖磁盘状态，同一份内容总产出一致的输出。
pub fn md_files_in(dir: &Path) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
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
        files.push(name.to_string());
    }
    files.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    Ok(files)
}

/// 排序键：开头的数字前缀按**数值**比较，其余按字面比较
///
/// 纯字面排序下 `100-xxx.md` 会排到 `11-xxx.md` 前面——编号一旦从两位进到三位，
/// 整个章节的顺序就乱了。按数值比较后，补零宽度只剩美观意义，无需重排文件。
/// 无数字前缀的文件（如 `rust/closure.md`）排在有编号的之后，彼此按名称排序。
fn sort_key(name: &str) -> (u64, &str) {
    let digits: usize = name.chars().take_while(|c| c.is_ascii_digit()).count();
    match name[..digits].parse::<u64>() {
        Ok(n) => (n, name),
        Err(_) => (u64::MAX, name),
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

    /// 带 `—— 说明` 后缀的条目（章节首页格式）也要能取出路径
    #[test]
    fn extracts_link_path_with_trailing_desc() {
        assert_eq!(
            extract_link_path("- [导论](./导论.md) —— 哲学是什么").as_deref(),
            Some("导论.md")
        );
    }

    #[test]
    fn ignores_non_link_lines() {
        assert_eq!(extract_link_path("# Rust"), None);
        assert_eq!(extract_link_path(""), None);
    }

    #[test]
    fn reads_meta_comments() {
        let content = "<!-- topic: 倒装结构 -->\n<!-- desc: 一句话说明 -->\n# 标题\n正文\n";
        assert_eq!(read_meta_str(content, "topic").as_deref(), Some("倒装结构"));
        assert_eq!(read_meta_str(content, "desc").as_deref(), Some("一句话说明"));
        assert_eq!(read_meta_str(content, "label"), None);
        assert_eq!(h1_of(content).as_deref(), Some("标题"));
    }

    #[test]
    fn splits_multiple_topics() {
        assert_eq!(split_topics("倒装结构"), vec!["倒装结构"]);
        // 半角与全角逗号都支持，两侧空白忽略
        assert_eq!(
            split_topics("倒装结构, 从句结构，比较结构"),
            vec!["倒装结构", "从句结构", "比较结构"]
        );
        // 重复与空项被丢弃
        assert_eq!(split_topics("倒装结构,,倒装结构"), vec!["倒装结构"]);
        assert!(split_topics("").is_empty());
        // 主题名里的斜杠和全角括号不受影响
        assert_eq!(
            split_topics("非谓语动词（分词 / 动名词）"),
            vec!["非谓语动词（分词 / 动名词）"]
        );
    }

    /// 编号按数值比较，跨过 99 后顺序仍然正确
    #[test]
    fn sorts_numeric_prefixes_by_value() {
        let mut names = vec![
            "100-c.md".to_string(),
            "9-a.md".to_string(),
            "11-b.md".to_string(),
            "02-z.md".to_string(),
        ];
        names.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        assert_eq!(names, vec!["02-z.md", "9-a.md", "11-b.md", "100-c.md"]);
    }

    /// 无编号的文件按名称排序，且排在有编号的之后
    #[test]
    fn sorts_unnumbered_after_numbered() {
        let mut names = vec![
            "closure.md".to_string(),
            "01-a.md".to_string(),
            "atomic.md".to_string(),
        ];
        names.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        assert_eq!(names, vec!["01-a.md", "atomic.md", "closure.md"]);
    }

    /// 正文深处的注释不应被当作元信息
    #[test]
    fn ignores_meta_far_into_body() {
        let mut content = String::from("# 标题\n");
        for _ in 0..20 {
            content.push_str("正文\n");
        }
        content.push_str("<!-- topic: 迟到的主题 -->\n");
        assert_eq!(read_meta_str(&content, "topic"), None);
    }
}
