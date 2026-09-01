//! 知识库的结构化元信息（`book-meta.toml`）。
//!
//! 分区划分与主题分类属于**内容层**的信息，过去硬编码在 `main.rs` 的常量里，
//! 加一个语法主题就得改 Rust 代码并重新编译。移到配置文件后，
//! 调整分类体系不再需要动代码。

use std::fs;
use std::path::Path;

use crate::toml_lite::{self, Table};

pub const META_FILE: &str = "book-meta.toml";

/// 一个顶层分区（对应 `src/<dir>/` 与侧边栏的一个 `# Part`）
#[derive(Debug)]
pub struct Section {
    /// `src/` 下的目录名
    pub dir: String,
    /// 侧边栏 part 标题
    pub part: String,
    /// 章节首页标题（同时作为 `index.md` 的 H1）
    pub title: String,
    /// 章节首页导语，可为空
    pub intro: String,
}

/// 一个由子文件 `<!-- topic: xxx -->` 驱动、自动分组重建的索引页
#[derive(Debug)]
pub struct TopicIndex {
    /// 索引页相对 `src/` 的路径，如 `english/analysis.md`
    pub index: String,
    /// 子文件目录，如 `english/analysis`
    pub dir: String,
    pub title: String,
    pub intro: String,
    /// 主题展示顺序；子文件声明的主题必须在此列表中
    pub topics: Vec<String>,
}

#[derive(Debug)]
pub struct BookMeta {
    pub sections: Vec<Section>,
    pub topic_indexes: Vec<TopicIndex>,
}

impl BookMeta {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(META_FILE);
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{META_FILE} 配置有误 —— {e}"))
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        let tables = toml_lite::parse(input)?;
        let mut sections: Vec<Section> = Vec::new();
        let mut topic_indexes: Vec<TopicIndex> = Vec::new();

        for (name, table) in &tables {
            match name.as_str() {
                "section" => {
                    let s = Section::from_table(table, sections.len())?;
                    if sections.iter().any(|x| x.dir == s.dir) {
                        return Err(format!("分区 `{}` 重复定义", s.dir));
                    }
                    sections.push(s);
                }
                "topic_index" => {
                    let ti = TopicIndex::from_table(table, topic_indexes.len())?;
                    if topic_indexes.iter().any(|x| x.dir == ti.dir) {
                        return Err(format!("主题索引 `{}` 重复定义", ti.dir));
                    }
                    topic_indexes.push(ti);
                }
                other => return Err(format!("未知的表 `[[{other}]]`")),
            }
        }

        if sections.is_empty() {
            return Err("至少需要一个 `[[section]]`".into());
        }

        for ti in &topic_indexes {
            // scan() 靠「`foo.md` 与同名目录 `foo/`」的约定把子条目挂到父页下，
            // 配置若违反这个约定，生成出的目录会缺失层级，故提前拦下。
            let expected = format!("{}.md", ti.dir);
            if ti.index != expected {
                return Err(format!(
                    "主题索引 `{}` 的 index 应为 `{expected}`（索引页必须与子目录同名）",
                    ti.dir
                ));
            }
            let top = ti.dir.split('/').next().unwrap_or_default();
            if !sections.iter().any(|s| s.dir == top) {
                return Err(format!("主题索引 `{}` 所属分区 `{top}` 未定义", ti.dir));
            }
        }

        Ok(BookMeta {
            sections,
            topic_indexes,
        })
    }

    pub fn topic_index_for(&self, dir: &str) -> Option<&TopicIndex> {
        self.topic_indexes.iter().find(|ti| ti.dir == dir)
    }
}

impl Section {
    fn from_table(t: &Table, i: usize) -> Result<Self, String> {
        let ctx = format!("第 {} 个 [[section]]", i + 1);
        let dir = t.req_str("dir", &ctx)?;
        if dir.is_empty() || dir.contains('/') {
            return Err(format!("{ctx}：`dir` 应是 src/ 下的一级目录名"));
        }
        let ctx = format!("[[section]] `{dir}`");
        Ok(Section {
            part: t.req_str("part", &ctx)?,
            title: t.req_str("title", &ctx)?,
            intro: t.opt_str("intro").unwrap_or_default().to_string(),
            dir,
        })
    }
}

impl TopicIndex {
    fn from_table(t: &Table, i: usize) -> Result<Self, String> {
        let ctx = format!("第 {} 个 [[topic_index]]", i + 1);
        let dir = t.req_str("dir", &ctx)?;
        let ctx = format!("[[topic_index]] `{dir}`");
        let topics = t.req_arr("topics", &ctx)?;
        if topics.is_empty() {
            return Err(format!("{ctx}：`topics` 不能为空"));
        }
        for (n, topic) in topics.iter().enumerate() {
            if topics[..n].contains(topic) {
                return Err(format!("{ctx}：主题 `{topic}` 重复"));
            }
        }
        Ok(TopicIndex {
            index: t.req_str("index", &ctx)?,
            title: t.req_str("title", &ctx)?,
            intro: t.opt_str("intro").unwrap_or_default().to_string(),
            topics,
            dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[[section]]
dir = "english"
part = "English"
title = "English"

[[section]]
dir = "rust"
part = "Rust"
title = "Rust"
intro = "学习笔记。"

[[topic_index]]
index = "english/analysis.md"
dir = "english/analysis"
title = "句子分析"
intro = "导语。"
topics = ["倒装结构", "比较结构"]
"#;

    #[test]
    fn parses_sample_config() {
        let meta = BookMeta::parse(SAMPLE).unwrap();
        assert_eq!(meta.sections.len(), 2);
        assert_eq!(meta.sections[0].dir, "english");
        // intro 可选，缺省为空串
        assert_eq!(meta.sections[0].intro, "");
        assert_eq!(meta.sections[1].intro, "学习笔记。");

        let ti = meta.topic_index_for("english/analysis").unwrap();
        assert_eq!(ti.title, "句子分析");
        assert_eq!(ti.topics, vec!["倒装结构", "比较结构"]);
    }

    #[test]
    fn rejects_index_not_matching_dir() {
        let src = SAMPLE.replace(
            r#"index = "english/analysis.md""#,
            r#"index = "english/other.md""#,
        );
        let err = BookMeta::parse(&src).unwrap_err();
        assert!(err.contains("必须与子目录同名"), "{err}");
    }

    #[test]
    fn rejects_topic_index_in_unknown_section() {
        let src = SAMPLE.replace(r#"dir = "english/analysis""#, r#"dir = "ghost/analysis""#);
        let src = src.replace(
            r#"index = "english/analysis.md""#,
            r#"index = "ghost/analysis.md""#,
        );
        let err = BookMeta::parse(&src).unwrap_err();
        assert!(err.contains("未定义"), "{err}");
    }

    #[test]
    fn rejects_duplicates_and_empties() {
        let dup = format!("{SAMPLE}\n[[section]]\ndir = \"rust\"\npart = \"R\"\ntitle = \"R\"\n");
        assert!(BookMeta::parse(&dup).unwrap_err().contains("重复"));

        let empty_topics = SAMPLE.replace(r#"topics = ["倒装结构", "比较结构"]"#, "topics = []");
        assert!(BookMeta::parse(&empty_topics)
            .unwrap_err()
            .contains("不能为空"));

        let dup_topic = SAMPLE.replace(
            r#"topics = ["倒装结构", "比较结构"]"#,
            r#"topics = ["倒装结构", "倒装结构"]"#,
        );
        assert!(BookMeta::parse(&dup_topic).unwrap_err().contains("重复"));
    }

    #[test]
    fn rejects_missing_required_keys() {
        let err = BookMeta::parse("[[section]]\ndir = \"x\"\n").unwrap_err();
        assert!(err.contains("part"), "{err}");
    }
}
