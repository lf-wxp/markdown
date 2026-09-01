//! 本地静态预览服务。
//!
//! 不使用 `mdbook serve`：它会重建 book/ 并清除 Pagefind 索引，导致搜索失效。

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;

pub fn run(root: &Path, port: u16) -> io::Result<()> {
    let root = root.join("book");
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
    fn decodes_percent_encoding() {
        assert_eq!(
            percent_decode("/rust/%E6%89%80%E6%9C%89%E6%9D%83.html"),
            "/rust/所有权.html"
        );
        assert_eq!(percent_decode("/plain.html"), "/plain.html");
    }

    #[test]
    fn maps_pagefind_content_types() {
        assert_eq!(
            content_type(Path::new("a.pf_index")),
            "application/octet-stream"
        );
        assert_eq!(content_type(Path::new("a.html")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("a.js")), "text/javascript; charset=utf-8");
    }
}
