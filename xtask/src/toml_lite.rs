//! 极简 TOML 子集解析器。
//!
//! 只覆盖 `book-meta.toml` 实际用到的语法：
//!
//! - `[[表名]]` 数组表
//! - `键 = "字符串"`（支持 `\n` `\t` `\"` `\\` 四种转义）
//! - `键 = ["a", "b"]` 字符串数组（可跨行书写）
//!
//! 之所以不引入 `toml` crate：整个 xtask 保持零依赖，配置面又完全由本仓库掌控。
//! 代价是必须**严格**——任何无法识别的写法一律报错，宁可构建失败，
//! 也不能让配置被静默误读成「少了一个主题」。

/// 配置项的值。TOML 的数字、布尔、嵌套表等类型本工具用不到，故不支持。
#[derive(Debug, PartialEq)]
pub enum Value {
    Str(String),
    Arr(Vec<String>),
}

/// 一个 `[[表]]` 的内容。保持声明顺序，便于错误信息定位。
#[derive(Debug, Default)]
pub struct Table {
    entries: Vec<(String, Value)>,
}

impl Table {
    fn insert(&mut self, key: String, value: Value) -> Result<(), String> {
        if self.entries.iter().any(|(k, _)| k == &key) {
            return Err(format!("键 `{key}` 重复"));
        }
        self.entries.push((key, value));
        Ok(())
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn opt_str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn req_str(&self, key: &str, ctx: &str) -> Result<String, String> {
        match self.get(key) {
            Some(Value::Str(s)) => Ok(s.clone()),
            Some(Value::Arr(_)) => Err(format!("{ctx}：`{key}` 应为字符串，实际是数组")),
            None => Err(format!("{ctx}：缺少必填键 `{key}`")),
        }
    }

    pub fn req_arr(&self, key: &str, ctx: &str) -> Result<Vec<String>, String> {
        match self.get(key) {
            Some(Value::Arr(a)) => Ok(a.clone()),
            Some(Value::Str(_)) => Err(format!("{ctx}：`{key}` 应为字符串数组，实际是字符串")),
            None => Err(format!("{ctx}：缺少必填键 `{key}`")),
        }
    }
}

/// 解析为 `(表名, 表内容)` 列表，保持文档中的出现顺序
pub fn parse(input: &str) -> Result<Vec<(String, Table)>, String> {
    let mut tables: Vec<(String, Table)> = Vec::new();
    let mut lines = input.lines().enumerate();

    while let Some((idx, raw)) = lines.next() {
        let no = idx + 1;
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }

        if let Some(inner) = line.strip_prefix("[[") {
            let name = inner
                .strip_suffix("]]")
                .ok_or_else(|| format!("第 {no} 行：数组表头未闭合"))?;
            tables.push((name.trim().to_string(), Table::default()));
            continue;
        }
        if line.starts_with('[') {
            return Err(format!("第 {no} 行：只支持 `[[表名]]` 形式的数组表"));
        }

        let (key, rest) = line
            .split_once('=')
            .ok_or_else(|| format!("第 {no} 行：无法解析 `{line}`"))?;
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(format!("第 {no} 行：键名为空"));
        }

        let value = parse_value(rest.trim(), no, &mut lines)?;
        let table = tables
            .last_mut()
            .ok_or_else(|| format!("第 {no} 行：键 `{key}` 出现在任何 `[[表]]` 之前"))?;
        table
            .1
            .insert(key, value)
            .map_err(|e| format!("第 {no} 行：{e}"))?;
    }

    Ok(tables)
}

fn parse_value<'a, I>(text: &str, no: usize, lines: &mut I) -> Result<Value, String>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    if !text.starts_with('[') {
        return Ok(Value::Str(parse_string(text, no)?));
    }

    // 数组可跨行：一直读到引号成对闭合且以 `]` 结尾为止
    let mut buf = text.to_string();
    while !array_complete(&buf) {
        let (_, more) = lines
            .next()
            .ok_or_else(|| format!("第 {no} 行：数组未闭合"))?;
        buf.push(' ');
        buf.push_str(strip_comment(more).trim());
    }
    Ok(Value::Arr(parse_array(&buf, no)?))
}

/// 数组是否已完整：不处于字符串内部，且最后一个有效字符是 `]`
fn array_complete(s: &str) -> bool {
    let mut in_str = false;
    let mut escaped = false;
    let mut last = None;
    for c in s.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
                last = Some('"');
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            c if c.is_whitespace() => {}
            c => last = Some(c),
        }
    }
    !in_str && last == Some(']')
}

fn parse_array(text: &str, no: usize) -> Result<Vec<String>, String> {
    let inner = text
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.trim_end().strip_suffix(']'))
        .ok_or_else(|| format!("第 {no} 行：数组格式不正确"))?;

    let mut out = Vec::new();
    for part in split_items(inner) {
        let part = part.trim();
        if part.is_empty() {
            continue; // 允许尾随逗号
        }
        out.push(parse_string(part, no)?);
    }
    Ok(out)
}

/// 按逗号切分数组元素，忽略字符串内部的逗号
fn split_items(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut escaped = false;

    for c in s.chars() {
        if in_str {
            cur.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                cur.push(c);
            }
            ',' => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    parts
}

fn parse_string(text: &str, no: usize) -> Result<String, String> {
    let body = text
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| format!("第 {no} 行：值 `{text}` 必须是双引号字符串"))?;
    unescape(body, no)
}

fn unescape(s: &str, no: usize) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => return Err(format!("第 {no} 行：不支持的转义 `\\{other}`")),
            None => return Err(format!("第 {no} 行：字符串以孤立的反斜杠结尾")),
        }
    }
    Ok(out)
}

/// 去掉行尾注释，但不误伤字符串内部的 `#`
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        } else if c == '#' {
            return &line[..i];
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tables_and_scalars() {
        let src = r#"
# 注释
[[section]]
dir = "english"
title = "English"

[[section]]
dir = "rust"
title = "Rust"
"#;
        let tables = parse(src).unwrap();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].0, "section");
        assert_eq!(tables[0].1.opt_str("dir"), Some("english"));
        assert_eq!(tables[1].1.opt_str("title"), Some("Rust"));
    }

    #[test]
    fn parses_inline_and_multiline_arrays() {
        let src = r#"
[[topic_index]]
a = ["x", "y"]
b = [
  "第一",
  "第二",
]
"#;
        let tables = parse(src).unwrap();
        let t = &tables[0].1;
        assert_eq!(t.req_arr("a", "ctx").unwrap(), vec!["x", "y"]);
        assert_eq!(t.req_arr("b", "ctx").unwrap(), vec!["第一", "第二"]);
    }

    #[test]
    fn handles_escapes() {
        let tables = parse("[[t]]\nintro = \"第一段\\n\\n第二段\"\n").unwrap();
        assert_eq!(tables[0].1.opt_str("intro"), Some("第一段\n\n第二段"));
    }

    /// 字符串里的 `#` 与逗号不应被当作注释或分隔符
    #[test]
    fn does_not_break_on_hash_or_comma_inside_string() {
        let tables = parse("[[t]]\na = \"C# 与 Rust, 以及别的\"  # 真注释\n").unwrap();
        assert_eq!(tables[0].1.opt_str("a"), Some("C# 与 Rust, 以及别的"));

        let tables = parse("[[t]]\na = [\"x, y\", \"z\"]\n").unwrap();
        assert_eq!(tables[0].1.req_arr("a", "c").unwrap(), vec!["x, y", "z"]);
    }

    #[test]
    fn rejects_unsupported_syntax() {
        // 普通表
        assert!(parse("[section]\na = \"b\"\n").is_err());
        // 裸值（非字符串）
        assert!(parse("[[t]]\na = 1\n").is_err());
        // 键出现在任何表之前
        assert!(parse("a = \"b\"\n").is_err());
        // 未闭合的表头
        assert!(parse("[[t]\n").is_err());
        // 未闭合的数组
        assert!(parse("[[t]]\na = [\"x\",\n").is_err());
        // 不支持的转义
        assert!(parse("[[t]]\na = \"\\q\"\n").is_err());
        // 重复键
        assert!(parse("[[t]]\na = \"1\"\na = \"2\"\n").is_err());
    }

    #[test]
    fn required_accessors_report_context() {
        let tables = parse("[[t]]\na = \"1\"\n").unwrap();
        let t = &tables[0].1;
        assert_eq!(t.req_str("a", "ctx").unwrap(), "1");
        let err = t.req_str("missing", "ctx").unwrap_err();
        assert!(err.contains("ctx") && err.contains("missing"));
        // 类型不匹配也要报错，而不是静默返回默认值
        assert!(t.req_arr("a", "ctx").is_err());
    }
}
