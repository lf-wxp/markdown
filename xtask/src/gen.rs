//! 目录与索引页的生成。
//!
//! 一次 `plan()` 把**所有**由工具托管的文件算成目标内容，再由 `apply()`
//! 统一与磁盘比对。托管的文件有三类：
//!
//! - `src/SUMMARY.md`           侧边栏目录
//! - `src/<分区>/index.md`      章节首页（与 SUMMARY 同序，避免两处各写一份）
//! - `src/<主题索引>.md`        按 `<!-- topic: -->` 分组的索引页
//!
//! 设计原则：**增量、幂等、不破坏手工排序**
//!
//! - 已在 `SUMMARY.md` 中出现的条目保持原有顺序不变
//!   （`rust/` 按「由浅入深」、`philosophy/` 按哲学史时间线手工排列，不能被字母序覆盖）
//! - 磁盘上新增的文件追加到所属分区末尾并在输出中提示
//! - 磁盘上已删除的文件从目录中移除并提示
//! - 章节标题优先取文件内的一级标题（`# xxx`），回退到文件名
//! - 同名子目录（如 `english/analysis/`）内的文件作为父页的子条目挂载

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::md;
use crate::meta::{BookMeta, Section, TopicIndex};

#[derive(Debug)]
pub enum GenError {
    Io(io::Error),
    /// 内容不合规（缺主题、主题未声明等）。严格失败，不产出半成品目录。
    Invalid(Vec<String>),
}

impl From<io::Error> for GenError {
    fn from(e: io::Error) -> Self {
        GenError::Io(e)
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::Io(e) => write!(f, "{e}"),
            GenError::Invalid(problems) => {
                writeln!(f, "内容校验未通过，共 {} 处：", problems.len())?;
                for p in problems {
                    writeln!(f, "  × {p}")?;
                }
                write!(
                    f,
                    "\n补齐上述元信息后重新运行；主题需与 book-meta.toml 中声明的一致。"
                )
            }
        }
    }
}

/// 一个待收录的条目：相对路径 + 嵌套深度
///
/// - `depth == 0`：章节页下的直接条目（如 `english/analysis.md`）
/// - `depth == 1`：子目录内的孙级条目（如 `english/analysis/01-xxx.md`），
///   挂在同名父页 `english/analysis.md` 之下
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub rel: String,
    pub depth: usize,
}

/// 一次生成的完整结果：所有托管文件的目标内容
#[derive(Debug)]
pub struct Plan {
    /// (相对 `src/` 的路径, 目标内容)
    pub files: Vec<(String, String)>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// 生成文件的 H1 来自配置而非磁盘，比对时需要用配置值
type Titles = HashMap<String, String>;

pub fn plan(src: &Path, meta: &BookMeta, current_summary: &str) -> Result<Plan, GenError> {
    let mut problems = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut titles: Titles = HashMap::new();

    // 主题索引页要先算：它的标题会成为 SUMMARY 中对应条目的文字
    for ti in &meta.topic_indexes {
        let content = build_topic_index(src, ti, &mut problems)?;
        titles.insert(ti.index.clone(), ti.title.clone());
        files.push((ti.index.clone(), content));
    }

    if !problems.is_empty() {
        return Err(GenError::Invalid(problems));
    }

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

    for section in &meta.sections {
        let on_disk = scan(src, &section.dir)?;
        let index_rel = format!("{}/index.md", section.dir);
        let index_exists = src.join(&index_rel).exists();
        if on_disk.is_empty() && !index_exists {
            continue;
        }

        let prior = existing_order(current_summary, &section.dir);

        // 父页（depth==0）按手工顺序重排，磁盘新增追加末尾。
        // 父页路径形如 `section/xxx.md`（恰好一个 `/`），据此过滤掉子目录条目。
        let disk_parents: Vec<String> = on_disk
            .iter()
            .filter(|i| i.depth == 0)
            .map(|i| i.rel.clone())
            .collect();
        let prior_parents: Vec<String> = prior
            .iter()
            .filter(|r| r.matches('/').count() == 1)
            .cloned()
            .collect();
        let (parents, removed_parents) = merge_order(&prior_parents, &disk_parents);
        removed.extend(removed_parents);

        lines.push(format!("# {}", section.part));
        lines.push(String::new());
        lines.push(format!("- [{}](./{index_rel})", section.title));

        for parent in &parents {
            let title = title_of(src, parent, &titles);
            lines.push(format!("  - [{title}](./{parent})"));
            added_if_new(&prior, parent, &mut added);

            // 该父页对应的子条目（depth==1）
            let child_prefix = format!("{}/", parent.trim_end_matches(".md"));
            let disk_children: Vec<String> = on_disk
                .iter()
                .filter(|i| i.depth == 1 && i.rel.starts_with(&child_prefix))
                .map(|i| i.rel.clone())
                .collect();
            if disk_children.is_empty() {
                continue;
            }
            let prior_children: Vec<String> = prior
                .iter()
                .filter(|r| r.starts_with(&child_prefix))
                .cloned()
                .collect();
            let (children, removed_children) = merge_order(&prior_children, &disk_children);
            removed.extend(removed_children);

            for child in &children {
                let ctitle = title_of(src, child, &titles);
                lines.push(format!("    - [{ctitle}](./{child})"));
                added_if_new(&prior, child, &mut added);
            }
        }
        lines.push(String::new());

        // 章节首页与侧边栏取同一份顺序，二者不可能再漂移
        files.push((index_rel, build_section_index(src, section, &parents, &titles)));
    }

    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.push(String::new()); // 以单个换行结尾

    files.push(("SUMMARY.md".into(), lines.join("\n")));

    Ok(Plan {
        files,
        added,
        removed,
    })
}

/// 把计划写入磁盘（或在 `check` 模式下只报告差异）
///
/// 返回 `true` 表示磁盘内容已是最新。
pub fn apply(src: &Path, plan: &Plan, check: bool, out: &mut impl Write) -> io::Result<bool> {
    let mut stale = Vec::new();
    for (rel, content) in &plan.files {
        let path = src.join(rel);
        let current = fs::read_to_string(&path).unwrap_or_default();
        if &current == content {
            continue;
        }
        stale.push(rel.clone());
        if !check {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, content)?;
        }
    }

    if stale.is_empty() {
        writeln!(out, "目录与索引页均为最新")?;
        return Ok(true);
    }

    if check {
        writeln!(out, "以下文件已过期，请运行: cargo make summary")?;
    } else {
        writeln!(out, "已更新 {} 个文件：", stale.len())?;
    }
    for rel in &stale {
        writeln!(out, "    {rel}")?;
    }
    for rel in &plan.added {
        writeln!(out, "  + 收录 {rel}")?;
    }
    for rel in &plan.removed {
        writeln!(out, "  - 移除 {rel}")?;
    }
    Ok(false)
}

fn title_of(src: &Path, rel: &str, titles: &Titles) -> String {
    titles
        .get(rel)
        .cloned()
        .unwrap_or_else(|| md::display_title(&src.join(rel)))
}

/// 生成章节首页正文
fn build_section_index(src: &Path, section: &Section, parents: &[String], titles: &Titles) -> String {
    let mut out = format!("# {}\n", section.title);
    if !section.intro.is_empty() {
        out.push('\n');
        out.push_str(&section.intro);
        out.push('\n');
    }
    out.push('\n');
    for rel in parents {
        let file = rel.rsplit_once('/').map(|(_, f)| f).unwrap_or(rel);
        let title = title_of(src, rel, titles);
        match md::read_desc(&src.join(rel)) {
            Some(desc) => out.push_str(&format!("- [{title}](./{file}) —— {desc}\n")),
            None => out.push_str(&format!("- [{title}](./{file})\n")),
        }
    }
    out
}

/// 生成一个主题索引页的正文，并把不合规的子文件记入 `problems`
fn build_topic_index(
    src: &Path,
    ti: &TopicIndex,
    problems: &mut Vec<String>,
) -> io::Result<String> {
    let dir = src.join(&ti.dir);
    let files = if dir.is_dir() {
        md::md_files_in(&dir)?
    } else {
        Vec::new()
    };
    let sub = ti
        .index
        .rsplit_once('/')
        .map(|(_, f)| f.trim_end_matches(".md"))
        .unwrap_or(&ti.dir);

    let mut grouped: Vec<(&str, Vec<String>)> =
        ti.topics.iter().map(|t| (t.as_str(), Vec::new())).collect();

    for fname in &files {
        let topics = md::read_topics(&dir.join(fname));
        if topics.is_empty() {
            problems.push(format!("{}/{fname} 缺少 <!-- topic: xxx --> 注释", ti.dir));
            continue;
        }
        // 一条可以归入多个主题，在每个相关分组下各出现一次
        for topic in topics {
            match grouped.iter_mut().find(|(t, _)| *t == topic) {
                Some(slot) => slot.1.push(fname.clone()),
                None => problems.push(format!(
                    "{}/{fname} 的主题「{topic}」未在 book-meta.toml 中声明",
                    ti.dir
                )),
            }
        }
    }

    let mut out = format!("# {}\n", ti.title);
    if !ti.intro.is_empty() {
        out.push('\n');
        out.push_str(&ti.intro);
        out.push('\n');
    }
    for (topic, items) in &grouped {
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {topic}\n\n"));
        for fname in items {
            let title = md::index_label(&dir.join(fname));
            out.push_str(&format!("- [{title}](./{sub}/{fname})\n"));
        }
    }
    Ok(out)
}

/// 从现有 SUMMARY.md 中提取某分区已有的相对路径顺序
///
/// 解析形如 `- [标题](./rust/xxx.md)` 的行，取出括号中的路径部分。
/// 返回的顺序保留手工排序（含子目录条目）。
pub fn existing_order(summary: &str, section: &str) -> Vec<String> {
    let prefix = format!("{section}/");
    let index_path = format!("{prefix}index.md");
    let mut order = Vec::new();

    for line in summary.lines() {
        let Some(rel) = md::extract_link_path(line) else {
            continue;
        };
        if rel.starts_with(&prefix) && rel != index_path && !order.contains(&rel) {
            order.push(rel);
        }
    }
    order
}

/// 扫描磁盘上某分区的所有条目（不含 index.md）
///
/// 除了 `src/<section>/*.md`，若某个 `foo.md` 存在同名子目录
/// `src/<section>/foo/`，则把该目录内的 `*.md` 作为孙级条目（`depth == 1`）
/// 紧随 `foo.md` 之后收录，用于「一个知识点一个文件」的展开式章节。
pub fn scan(src: &Path, section: &str) -> io::Result<Vec<Item>> {
    let dir = src.join(section);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for name in md::md_files_in(&dir)? {
        items.push(Item {
            rel: format!("{section}/{name}"),
            depth: 0,
        });

        // 同名子目录：foo.md ↔ foo/
        let stem = name.trim_end_matches(".md");
        let subdir = dir.join(stem);
        if subdir.is_dir() {
            for child in md::md_files_in(&subdir)? {
                items.push(Item {
                    rel: format!("{section}/{stem}/{child}"),
                    depth: 1,
                });
            }
        }
    }
    Ok(items)
}

/// 若某路径不在历史顺序中，则记为「新增」（用于输出提示）
fn added_if_new(prior: &[String], rel: &str, added: &mut Vec<String>) {
    if !prior.iter().any(|r| r == rel) {
        added.push(rel.to_string());
    }
}

/// 按「手工顺序优先、磁盘新增追加末尾」重排一组路径
///
/// - `manual`：来自现有 SUMMARY 的顺序（可能含已删除项）
/// - `disk`：磁盘上真实存在的路径集合（决定去留）
///
/// 返回重排后的路径，以及被移除的路径（在 SUMMARY 中但磁盘已无）。
pub fn merge_order(manual: &[String], disk: &[String]) -> (Vec<String>, Vec<String>) {
    let kept: Vec<String> = manual.iter().filter(|r| disk.contains(r)).cloned().collect();
    let removed: Vec<String> = manual
        .iter()
        .filter(|r| !disk.contains(r))
        .cloned()
        .collect();
    let mut ordered = kept;
    for r in disk {
        if !ordered.contains(r) {
            ordered.push(r.clone());
        }
    }
    (ordered, removed)
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

[[topic_index]]
index = "english/analysis.md"
dir = "english/analysis"
title = "句子分析"
intro = "导语。"
topics = ["倒装结构", "比较结构"]
"#,
        )
        .unwrap()
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

    /// 同名子目录内的文件被作为 depth==1 的孙级条目，紧随父页之后
    #[test]
    fn scan_nests_subdir_files_under_parent() {
        let tmp = TmpDir::new("scan-nest");
        let src = tmp.path();
        write(&src.join("english/analysis.md"), "# 句子分析\n");
        write(&src.join("english/analysis/01-a.md"), "# A\n");
        write(&src.join("english/analysis/02-b.md"), "# B\n");
        write(&src.join("english/phonetics.md"), "# 发音\n");

        let items = scan(src, "english").unwrap();
        assert_eq!(
            items,
            vec![
                Item { rel: "english/analysis.md".into(), depth: 0 },
                Item { rel: "english/analysis/01-a.md".into(), depth: 1 },
                Item { rel: "english/analysis/02-b.md".into(), depth: 1 },
                Item { rel: "english/phonetics.md".into(), depth: 0 },
            ]
        );
    }

    #[test]
    fn builds_nested_summary() {
        let tmp = TmpDir::new("plan-nest");
        let src = tmp.path();
        write(&src.join("english/index.md"), "# English\n");
        write(&src.join("english/analysis.md"), "# 旧标题\n");
        write(&src.join("english/analysis/01-a.md"), "<!-- topic: 倒装结构 -->\n# 倒装 A\n");
        write(&src.join("english/analysis/02-b.md"), "<!-- topic: 比较结构 -->\n# 比较 B\n");

        let plan = plan(src, &meta(), "").unwrap();
        let summary = &plan.files.iter().find(|(r, _)| r == "SUMMARY.md").unwrap().1;
        assert!(summary.contains("- [English](./english/index.md)"));
        // analysis.md 的标题取配置里的 title，而非磁盘上的旧 H1
        assert!(summary.contains("  - [句子分析](./english/analysis.md)"));
        assert!(summary.contains("    - [倒装 A](./english/analysis/01-a.md)"));
        assert!(!summary.contains("旧标题"));
        assert!(plan.added.contains(&"english/analysis/01-a.md".to_string()));
    }

    /// 磁盘新增子条目追加末尾、删除的从输出移除，且保留手工排序
    #[test]
    fn preserves_child_order_and_syncs() {
        let tmp = TmpDir::new("plan-sync");
        let src = tmp.path();
        write(&src.join("english/index.md"), "# English\n");
        write(&src.join("english/analysis.md"), "# 句子分析\n");
        write(&src.join("english/analysis/01-a.md"), "<!-- topic: 倒装结构 -->\n# A\n");
        write(&src.join("english/analysis/03-c.md"), "<!-- topic: 倒装结构 -->\n# C\n");

        // 现有 SUMMARY 里手工顺序是 03 在前、01 在后，且含已删除的 02
        let current = "\
# English

- [English](./english/index.md)
  - [句子分析](./english/analysis.md)
    - [C](./english/analysis/03-c.md)
    - [B](./english/analysis/02-b.md)
    - [A](./english/analysis/01-a.md)
";
        let plan = plan(src, &meta(), current).unwrap();
        let summary = &plan.files.iter().find(|(r, _)| r == "SUMMARY.md").unwrap().1;
        let pos_c = summary.find("03-c.md").unwrap();
        let pos_a = summary.find("01-a.md").unwrap();
        assert!(pos_c < pos_a, "应保留手工顺序");
        assert!(!summary.contains("02-b.md"));
        assert!(plan.removed.contains(&"english/analysis/02-b.md".to_string()));
    }

    #[test]
    fn topic_index_groups_in_declared_order_and_prefers_label() {
        let tmp = TmpDir::new("topic-index");
        let src = tmp.path();
        write(&src.join("english/index.md"), "# English\n");
        write(&src.join("english/analysis.md"), "# 句子分析\n");
        // 文件名顺序 01,02,03，但主题交错
        write(&src.join("english/analysis/01-x.md"), "<!-- topic: 倒装结构 -->\n# 倒装 X\n");
        write(&src.join("english/analysis/02-y.md"), "<!-- topic: 比较结构 -->\n# 比较 Y\n");
        write(
            &src.join("english/analysis/03-z.md"),
            "<!-- topic: 倒装结构 -->\n<!-- label: 短标题 -->\n# 一个很长很长的原句作为 H1\n",
        );

        let plan = plan(src, &meta(), "").unwrap();
        let idx = &plan
            .files
            .iter()
            .find(|(r, _)| r == "english/analysis.md")
            .unwrap()
            .1;

        assert!(idx.starts_with("# 句子分析\n\n导语。\n"));
        let p_inv = idx.find("## 倒装结构").unwrap();
        let p_cmp = idx.find("## 比较结构").unwrap();
        assert!(p_inv < p_cmp, "应按 topics 声明序分组");
        let p01 = idx.find("[倒装 X](./analysis/01-x.md)").unwrap();
        let p03 = idx.find("[短标题](./analysis/03-z.md)").unwrap();
        assert!(p01 < p03 && p03 < p_cmp, "组内应按文件名序");
        // 索引页用 label，而非长 H1
        assert!(!idx.contains("一个很长很长的原句"));
    }

    /// 跨知识点的条目在每个相关主题下各出现一次，但侧边栏里仍只有一条
    #[test]
    fn multi_topic_entry_appears_in_each_group() {
        let tmp = TmpDir::new("multi-topic");
        let src = tmp.path();
        write(&src.join("english/index.md"), "# English\n");
        write(&src.join("english/analysis.md"), "# 句子分析\n");
        write(
            &src.join("english/analysis/01-x.md"),
            "<!-- topic: 倒装结构, 比较结构 -->\n# 让步倒装\n",
        );

        let plan = plan(src, &meta(), "").unwrap();
        let idx = &plan
            .files
            .iter()
            .find(|(r, _)| r == "english/analysis.md")
            .unwrap()
            .1;
        assert_eq!(
            idx.matches("[让步倒装](./analysis/01-x.md)").count(),
            2,
            "两个主题下都应出现"
        );

        let summary = &plan.files.iter().find(|(r, _)| r == "SUMMARY.md").unwrap().1;
        assert_eq!(
            summary.matches("english/analysis/01-x.md").count(),
            1,
            "侧边栏不应重复"
        );
    }

    /// 编号跨过 99 后，侧边栏顺序仍按数值而非字面
    #[test]
    fn orders_entries_numerically_past_ninety_nine() {
        let tmp = TmpDir::new("numeric-order");
        let src = tmp.path();
        write(&src.join("english/index.md"), "# English\n");
        write(&src.join("english/analysis.md"), "# 句子分析\n");
        for n in ["09", "11", "100"] {
            write(
                &src.join(format!("english/analysis/{n}-x.md")),
                "<!-- topic: 倒装结构 -->\n# T\n",
            );
        }

        let plan = plan(src, &meta(), "").unwrap();
        let summary = &plan.files.iter().find(|(r, _)| r == "SUMMARY.md").unwrap().1;
        let p09 = summary.find("09-x.md").unwrap();
        let p11 = summary.find("11-x.md").unwrap();
        let p100 = summary.find("100-x.md").unwrap();
        assert!(p09 < p11 && p11 < p100, "100 应排在 11 之后");
    }

    /// 章节首页由工具生成，条目顺序与 SUMMARY 一致，并带上 desc 说明
    #[test]
    fn generates_section_index_with_desc() {
        let tmp = TmpDir::new("section-index");
        let src = tmp.path();
        write(&src.join("english/index.md"), "# 过期的手写内容\n");
        write(&src.join("english/analysis.md"), "# 句子分析\n");
        write(
            &src.join("english/phonetics.md"),
            "<!-- desc: 元音与辅音 -->\n# 英语发音规则\n",
        );

        let plan = plan(src, &meta(), "").unwrap();
        let idx = &plan
            .files
            .iter()
            .find(|(r, _)| r == "english/index.md")
            .unwrap()
            .1;
        assert_eq!(
            idx,
            "# English\n\n- [句子分析](./analysis.md)\n- [英语发音规则](./phonetics.md) —— 元音与辅音\n"
        );
    }

    #[test]
    fn section_index_renders_intro() {
        let tmp = TmpDir::new("section-intro");
        let src = tmp.path();
        write(&src.join("philosophy/index.md"), "");
        write(&src.join("philosophy/导论.md"), "# 导论\n");
        let m = BookMeta::parse(
            "[[section]]\ndir = \"philosophy\"\npart = \"Philosophy\"\ntitle = \"Philosophy\"\nintro = \"按时间线组织。\"\n",
        )
        .unwrap();

        let plan = plan(src, &m, "").unwrap();
        let idx = &plan
            .files
            .iter()
            .find(|(r, _)| r == "philosophy/index.md")
            .unwrap()
            .1;
        assert_eq!(idx, "# Philosophy\n\n按时间线组织。\n\n- [导论](./导论.md)\n");
    }

    /// 缺主题 / 主题未声明都必须失败，而不是静默漏掉条目
    #[test]
    fn rejects_missing_and_undeclared_topics() {
        let tmp = TmpDir::new("topic-strict");
        let src = tmp.path();
        write(&src.join("english/index.md"), "# English\n");
        write(&src.join("english/analysis.md"), "# 句子分析\n");
        write(&src.join("english/analysis/01-x.md"), "# 没有主题\n");
        write(&src.join("english/analysis/02-y.md"), "<!-- topic: 未声明的主题 -->\n# Y\n");

        let err = plan(src, &meta(), "").unwrap_err();
        let GenError::Invalid(problems) = err else {
            panic!("应为内容校验错误");
        };
        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("01-x.md") && problems[0].contains("缺少"));
        assert!(problems[1].contains("02-y.md") && problems[1].contains("未在 book-meta.toml"));
    }

    /// 同一份磁盘状态重复生成结果一致；apply 后再 check 应报告最新
    #[test]
    fn apply_is_idempotent() {
        let tmp = TmpDir::new("apply-idem");
        let src = tmp.path();
        write(&src.join("english/index.md"), "");
        write(&src.join("english/analysis.md"), "");
        write(&src.join("english/analysis/01-a.md"), "<!-- topic: 倒装结构 -->\n# A\n");

        let m = meta();
        let mut sink = Vec::new();
        let p1 = plan(src, &m, "").unwrap();
        assert!(!apply(src, &p1, false, &mut sink).unwrap(), "首次应有写入");

        let current = fs::read_to_string(src.join("SUMMARY.md")).unwrap();
        let p2 = plan(src, &m, &current).unwrap();
        assert!(apply(src, &p2, false, &mut sink).unwrap(), "二次应无变化");
    }
}
