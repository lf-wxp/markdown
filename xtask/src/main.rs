//! 知识库构建辅助工具。
//!
//! 子命令：
//! - `summary`      重建 SUMMARY.md、各章节首页与主题索引页
//! - `new`          按约定创建新条目（自动编号、slug 与元信息）
//! - `lint`         检查正文是否符合写作约定（渲染层面的坑）
//! - `prep-index`   建索引前处理构建产物（移除 print.html、标记导航页为不索引）
//! - `serve`        启动本地静态预览服务
//!
//! 分区与主题分类定义在根目录的 `book-meta.toml`，调整分类体系无需改代码。

mod gen;
mod lint;
mod md;
mod meta;
mod prep;
mod scaffold;
mod serve;
mod toml_lite;

#[cfg(test)]
mod testutil;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use meta::BookMeta;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let rest = args.get(1..).unwrap_or(&[]);
    let check = args.iter().any(|a| a == "--check");

    match cmd {
        "summary" => report(run_summary(check)),
        "new" => report(run_new(rest)),
        "lint" => report(run_lint(args.iter().any(|a| a == "--fix"))),
        "prep-index" => report(prep::run(&project_root()).map_err(|e| e.to_string()).map(|_| true)),
        "serve" => {
            let port = rest
                .iter()
                .position(|a| a == "--port")
                .and_then(|i| rest.get(i + 1))
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(8000);
            report(
                serve::run(&project_root(), port)
                    .map_err(|e| e.to_string())
                    .map(|_| true),
            )
        }
        _ => {
            print_help();
            ExitCode::SUCCESS
        }
    }
}

/// `Ok(true)` 成功；`Ok(false)` 校验发现差异；`Err` 执行出错
fn report(result: Result<bool, String>) -> ExitCode {
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!(
        "\
知识库构建辅助工具

用法:
    cargo xtask summary            重建目录、章节首页与主题索引页
    cargo xtask summary --check    校验是否最新，不写文件（CI 用）
    cargo xtask new <目录> ...     创建新条目
    cargo xtask lint               检查正文写作约定
    cargo xtask lint --fix         顺带修复可机械修复的问题
    cargo xtask prep-index         建索引前处理 book/ 产物
    cargo xtask serve [--port N]   启动本地预览（默认 8000）

new 的参数:
    --title  <标题>    必填，作为文件 H1
    --topic  <主题>    主题索引目录（如 english/analysis）必填
    --quote  <原句>    引用的原句，同时用作 slug 来源
    --label  <短标题>  索引页中的链接文字，默认用 H1
    --desc   <说明>    章节首页中跟在链接后的一句话
    --slug   <slug>    自定义文件名，默认从原句/标题的英文词生成

示例:
    cargo xtask new english/analysis \\
        --topic 倒装结构 \\
        --title '否定前置倒装：Nor' \\
        --quote 'Nor is the harm produced by creation'

分区与主题分类见根目录 book-meta.toml。
日常构建请用: cargo make build / serve / check
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

fn load(root: &Path) -> Result<(PathBuf, BookMeta), String> {
    let src = root.join("src");
    if !src.is_dir() {
        return Err(format!("找不到源目录: {}", src.display()));
    }
    let meta = BookMeta::load(root)?;
    Ok((src, meta))
}

fn run_summary(check: bool) -> Result<bool, String> {
    let root = project_root();
    let (src, meta) = load(&root)?;

    let current = fs::read_to_string(src.join("SUMMARY.md")).unwrap_or_default();
    let plan = gen::plan(&src, &meta, &current).map_err(|e| e.to_string())?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let up_to_date = gen::apply(&src, &plan, check, &mut out).map_err(|e| e.to_string())?;

    // 只有 --check 才把「有差异」视为失败；写入模式下发生改动是正常结果
    Ok(!check || up_to_date)
}

fn run_lint(fix: bool) -> Result<bool, String> {
    let root = project_root();
    let src = root.join("src");
    if !src.is_dir() {
        return Err(format!("找不到源目录: {}", src.display()));
    }
    lint::run(&src, fix).map_err(|e| e.to_string())
}

fn run_new(args: &[String]) -> Result<bool, String> {
    let root = project_root();
    let (src, meta) = load(&root)?;

    let rel = scaffold::run_new(&src, &meta, args)?;
    println!("已创建 src/{rel}");

    // 立刻同步目录，新条目即时出现在侧边栏与索引页中
    let current = fs::read_to_string(src.join("SUMMARY.md")).unwrap_or_default();
    let plan = gen::plan(&src, &meta, &current).map_err(|e| e.to_string())?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    gen::apply(&src, &plan, false, &mut out).map_err(|e| e.to_string())?;

    let _ = out.flush();
    Ok(true)
}
