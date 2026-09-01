//! `cargo xtask new`：按约定创建新条目。
//!
//! 新增一条知识点原本要手工做四件事：翻目录找到最大编号、编一个英文 slug、
//! 写 `<!-- topic: -->` 注释、写 H1。这四步都是机械劳动，且漏掉任何一步
//! 都会被 `summary` 的严格校验拦下来。这里一次性生成。

use std::fs;
use std::path::Path;

use crate::md;
use crate::meta::BookMeta;

/// 编号前缀的最小宽度（现有条目形如 `01-xxx.md`）
const MIN_WIDTH: usize = 2;

/// slug 最多取几个单词
const SLUG_WORDS: usize = 5;

/// 这些词对定位条目没有帮助，生成 slug 时跳过。
///
/// 介词（of / to / in / as …）刻意保留：本书正是在讲它们的用法。
const SLUG_STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "is", "are", "was", "were", "be", "been", "being", "that", "this",
    "it", "its",
];

pub struct NewArgs {
    pub dir: String,
    pub title: String,
    /// 可指定多个：`--topic A --topic B` 或 `--topic 'A, B'`
    pub topics: Vec<String>,
    pub quote: Option<String>,
    pub label: Option<String>,
    pub desc: Option<String>,
    pub slug: Option<String>,
}

/// 创建新条目，返回其相对 `src/` 的路径
pub fn run_new(src: &Path, meta: &BookMeta, args: &[String]) -> Result<String, String> {
    let args = parse_args(args)?;
    let dir = src.join(&args.dir);
    if !dir.is_dir() {
        return Err(format!("目录不存在：src/{}", args.dir));
    }

    let topic_index = meta.topic_index_for(&args.dir);

    // 主题目录的条目必须带主题，否则生成目录时会被校验拦下
    match (topic_index, args.topics.is_empty()) {
        (Some(ti), true) => {
            return Err(format!(
                "src/{} 是主题索引目录，必须用 --topic 指定主题。可选主题：\n{}",
                args.dir,
                list_topics(&ti.topics)
            ))
        }
        (Some(ti), false) => {
            for t in &args.topics {
                if !ti.topics.contains(t) {
                    return Err(format!(
                        "主题「{t}」未在 book-meta.toml 中声明。可选主题：\n{}",
                        list_topics(&ti.topics)
                    ));
                }
            }
        }
        (None, false) => {
            return Err(format!("src/{} 不是主题索引目录，不支持 --topic", args.dir))
        }
        (None, true) => {}
    }

    let slug = resolve_slug(&args)?;
    let filename = if topic_index.is_some() {
        // 编号进位到三位也无妨：排序按数值而非字面，补零只是为了好看
        let (next, width) = next_number(&dir)?;
        format!("{next:0width$}-{slug}.md")
    } else {
        format!("{slug}.md")
    };

    let path = dir.join(&filename);
    if path.exists() {
        return Err(format!("文件已存在：src/{}/{filename}", args.dir));
    }

    fs::write(&path, render(&args))
        .map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;

    Ok(format!("{}/{filename}", args.dir))
}

fn list_topics(topics: &[String]) -> String {
    topics
        .iter()
        .map(|x| format!("    {x}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(args: &NewArgs) -> String {
    let mut out = String::new();
    if !args.topics.is_empty() {
        out.push_str(&format!("<!-- topic: {} -->\n", args.topics.join(", ")));
    }
    if let Some(l) = &args.label {
        out.push_str(&format!("<!-- label: {l} -->\n"));
    }
    if let Some(d) = &args.desc {
        out.push_str(&format!("<!-- desc: {d} -->\n"));
    }
    out.push_str(&format!("# {}\n\n", args.title));

    // 主题条目的固定骨架是「原句引用 + 解析要点」，先摆好占位
    match &args.quote {
        Some(q) => out.push_str(&format!("> {q}\n\n- \n")),
        None if !args.topics.is_empty() => out.push_str("> \n\n- \n"),
        None => {}
    }
    out
}

/// 目录中现有编号的下一个值，以及应使用的补零宽度
fn next_number(dir: &Path) -> Result<(usize, usize), String> {
    let files = md::md_files_in(dir).map_err(|e| format!("读取 {} 失败：{e}", dir.display()))?;
    let mut max = 0;
    let mut width = MIN_WIDTH;
    for name in &files {
        let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        width = width.max(digits.len());
        if let Ok(n) = digits.parse::<usize>() {
            max = max.max(n);
        }
    }
    Ok((max + 1, width))
}

fn resolve_slug(args: &NewArgs) -> Result<String, String> {
    if let Some(s) = &args.slug {
        let s = slugify(s, usize::MAX);
        if s.is_empty() {
            return Err("--slug 需要至少包含一个 ASCII 字母或数字".into());
        }
        return Ok(s);
    }
    // 原句里的英文比中文标题更适合做文件名
    for source in [&args.quote, &Some(args.title.clone())].into_iter().flatten() {
        let s = slugify(source, SLUG_WORDS);
        if !s.is_empty() {
            return Ok(s);
        }
    }
    Err("无法从标题或原句中提取英文 slug，请用 --slug 指定".into())
}

/// 取前若干个英文单词，小写后用 `-` 连接
fn slugify(text: &str, max_words: usize) -> String {
    let mut words = Vec::new();
    for raw in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if words.len() >= max_words {
            break;
        }
        let w = raw.to_ascii_lowercase();
        if w.is_empty() || SLUG_STOPWORDS.contains(&w.as_str()) {
            continue;
        }
        words.push(w);
    }
    words.join("-")
}

fn parse_args(args: &[String]) -> Result<NewArgs, String> {
    let mut positional = None;
    let mut out = NewArgs {
        dir: String::new(),
        title: String::new(),
        topics: Vec::new(),
        quote: None,
        label: None,
        desc: None,
        slug: None,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(rest) = arg.strip_prefix("--") {
            let (name, inline) = match rest.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (rest, None),
            };
            let value = match inline {
                Some(v) => v,
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| format!("--{name} 缺少取值"))?
                }
            };
            let slot = match name {
                "title" => &mut out.title,
                "topic" => {
                    // 可重复出现，也可在一个取值里用逗号分隔多个
                    for t in md::split_topics(&value) {
                        if !out.topics.contains(&t) {
                            out.topics.push(t);
                        }
                    }
                    i += 1;
                    continue;
                }
                "quote" => {
                    out.quote = Some(value);
                    i += 1;
                    continue;
                }
                "label" => {
                    out.label = Some(value);
                    i += 1;
                    continue;
                }
                "desc" => {
                    out.desc = Some(value);
                    i += 1;
                    continue;
                }
                "slug" => {
                    out.slug = Some(value);
                    i += 1;
                    continue;
                }
                other => return Err(format!("未知参数 --{other}")),
            };
            *slot = value;
        } else if positional.is_none() {
            positional = Some(arg.clone());
        } else {
            return Err(format!("多余的参数 `{arg}`"));
        }
        i += 1;
    }

    out.dir = positional.ok_or("缺少目标目录，例如: cargo xtask new english/analysis")?;
    out.dir = out.dir.trim_matches('/').to_string();
    if out.title.is_empty() {
        return Err("缺少 --title".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{write, TmpDir};

    fn meta() -> BookMeta {
        BookMeta::parse(
            r#"
[[section]]
dir = "english"
part = "English"
title = "English"

[[section]]
dir = "rust"
part = "Rust"
title = "Rust"

[[topic_index]]
index = "english/analysis.md"
dir = "english/analysis"
title = "句子分析"
topics = ["倒装结构", "比较结构"]
"#,
        )
        .unwrap()
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn slugify_takes_english_words() {
        assert_eq!(slugify("Had one not come into existence", 5), "had-one-not-come-into");
        // 停用词被跳过
        assert_eq!(slugify("The harm that is produced", 5), "harm-produced");
        // 中文取不出 slug
        assert_eq!(slugify("虚拟语气倒装", 5), "");
    }

    #[test]
    fn creates_numbered_entry_in_topic_dir() {
        let tmp = TmpDir::new("new-topic");
        let src = tmp.path();
        write(&src.join("english/analysis/01-a.md"), "<!-- topic: 倒装结构 -->\n# A\n");
        write(&src.join("english/analysis/07-b.md"), "<!-- topic: 倒装结构 -->\n# B\n");

        let rel = run_new(
            src,
            &meta(),
            &args(&[
                "english/analysis",
                "--topic",
                "倒装结构",
                "--title",
                "否定前置倒装",
                "--quote",
                "Nor is the harm produced",
            ]),
        )
        .unwrap();

        // 编号取 max+1 而非文件数+1
        assert_eq!(rel, "english/analysis/08-nor-harm-produced.md");
        let body = fs::read_to_string(src.join(&rel)).unwrap();
        assert_eq!(
            body,
            "<!-- topic: 倒装结构 -->\n# 否定前置倒装\n\n> Nor is the harm produced\n\n- \n"
        );
    }

    #[test]
    fn creates_plain_entry_with_desc() {
        let tmp = TmpDir::new("new-plain");
        let src = tmp.path();
        fs::create_dir_all(src.join("rust")).unwrap();

        let rel = run_new(
            src,
            &meta(),
            &args(&["rust", "--title", "闭包", "--slug", "closure", "--desc", "捕获环境"]),
        )
        .unwrap();

        assert_eq!(rel, "rust/closure.md");
        let body = fs::read_to_string(src.join(&rel)).unwrap();
        assert_eq!(body, "<!-- desc: 捕获环境 -->\n# 闭包\n\n");
    }

    /// 多主题可以重复传 --topic，也可以在一个取值里用逗号分隔
    #[test]
    fn creates_entry_with_multiple_topics() {
        let tmp = TmpDir::new("new-multitopic");
        let src = tmp.path();
        fs::create_dir_all(src.join("english/analysis")).unwrap();

        let rel = run_new(
            src,
            &meta(),
            &args(&[
                "english/analysis",
                "--topic",
                "倒装结构",
                "--topic",
                "比较结构",
                "--title",
                "X",
                "--slug",
                "x",
            ]),
        )
        .unwrap();
        let body = fs::read_to_string(src.join(&rel)).unwrap();
        assert!(body.starts_with("<!-- topic: 倒装结构, 比较结构 -->\n"), "{body}");

        let a = parse_args(&args(&["english/analysis", "--title", "X", "--topic", "倒装结构, 比较结构"]))
            .unwrap();
        assert_eq!(a.topics, vec!["倒装结构", "比较结构"]);
    }

    #[test]
    fn rejects_undeclared_or_missing_topic() {
        let tmp = TmpDir::new("new-badtopic");
        let src = tmp.path();
        fs::create_dir_all(src.join("english/analysis")).unwrap();

        let err = run_new(
            src,
            &meta(),
            &args(&["english/analysis", "--title", "X", "--slug", "x", "--topic", "不存在"]),
        )
        .unwrap_err();
        assert!(err.contains("未在 book-meta.toml 中声明"), "{err}");

        // 主题目录下不给 --topic 直接失败，并列出可选主题
        let err = run_new(
            src,
            &meta(),
            &args(&["english/analysis", "--title", "X", "--slug", "x"]),
        )
        .unwrap_err();
        assert!(err.contains("必须用 --topic") && err.contains("倒装结构"), "{err}");
    }

    #[test]
    fn refuses_to_overwrite() {
        let tmp = TmpDir::new("new-clobber");
        let src = tmp.path();
        write(&src.join("rust/closure.md"), "# 已存在\n");

        let err = run_new(
            src,
            &meta(),
            &args(&["rust", "--title", "闭包", "--slug", "closure"]),
        )
        .unwrap_err();
        assert!(err.contains("已存在"), "{err}");
    }

    #[test]
    fn requires_slug_when_title_has_no_ascii() {
        let tmp = TmpDir::new("new-noslug");
        let src = tmp.path();
        fs::create_dir_all(src.join("rust")).unwrap();

        let err = run_new(src, &meta(), &args(&["rust", "--title", "所有权"])).unwrap_err();
        assert!(err.contains("--slug"), "{err}");
    }

    #[test]
    fn parses_flag_forms_and_rejects_unknown() {
        let a = parse_args(&args(&["rust", "--title=闭包", "--slug", "closure"])).unwrap();
        assert_eq!(a.dir, "rust");
        assert_eq!(a.title, "闭包");
        assert_eq!(a.slug.as_deref(), Some("closure"));

        assert!(parse_args(&args(&["rust", "--title", "x", "--bogus", "1"])).is_err());
        assert!(parse_args(&args(&["rust"])).is_err(), "缺 --title 应报错");
        assert!(parse_args(&args(&["--title", "x"])).is_err(), "缺目录应报错");
        assert!(parse_args(&args(&["rust", "--title"])).is_err(), "flag 缺值应报错");
    }
}
