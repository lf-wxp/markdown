# 多阶段构建：构建阶段生成静态站点，运行阶段仅携带产物 + nginx
#
# 构建:  docker build -t notes .
# 运行:  docker run -d -p 8080:80 --name notes notes
# 访问:  http://localhost:8080
#
# 多平台构建:
#   docker buildx build --platform linux/amd64,linux/arm64 -t notes .
#
# 设计说明：工具安装交给 cargo-binstall 处理——它会自动探测当前平台并
# 匹配对应的官方预编译发布物（各 crate 的 target triple 命名并不统一，
# 例如 mdbook 在 arm64 上只发布 musl 版），找不到时自动回退源码编译。
# 因此这里无需硬编码任何平台标识，amd64 / arm64 通用。

# ---------------------------------------------------------------- 构建阶段
FROM rust:1.98-slim AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 版本固定，保证镜像可复现。取自 tools.env——CI 与 cargo make setup 用的是同一份，
# 避免三处各自声明后悄悄漂移。
COPY tools.env /tmp/tools.env

# cargo-binstall 自身也优先取预编译版本，安装很快
RUN curl -fsSL https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall -V

# mdbook / mdbook-toc 用 binstall 取预编译版本；
# pagefind 必须启用 extended feature 才具备 CJK 分词（binstall 无法指定 feature，
# 故从源码编译）。标准版会使中文召回严重不足：实测「三体」「意义」返回 0 条结果。
RUN set -eux; \
    . /tmp/tools.env; \
    cargo binstall --no-confirm --locked \
        "mdbook@${MDBOOK_VERSION}" \
        "mdbook-toc@${MDBOOK_TOC_VERSION}"; \
    cargo install pagefind --version "${PAGEFIND_VERSION}" \
        --features extended --locked; \
    mdbook --version; \
    mdbook-toc --version; \
    printf '<html><body><main>probe</main></body></html>' > /tmp/i.html; \
    pagefind --site /tmp 2>&1 | grep -qi extended; \
    rm -rf /tmp/i.html /tmp/pagefind

WORKDIR /build

# 先只拷贝清单以缓存 xtask 编译层（xtask 零外部依赖，此层几乎不会失效）
COPY Cargo.toml Cargo.lock ./
COPY xtask/Cargo.toml ./xtask/
RUN mkdir -p xtask/src \
    && echo 'fn main() {}' > xtask/src/main.rs \
    && cargo build --release --package xtask

# 拷贝真实源码并重新编译
COPY xtask ./xtask
RUN touch xtask/src/main.rs && cargo build --release --package xtask

COPY src ./src
COPY theme ./theme
COPY book.toml book-meta.toml ./

# 构建流程与本地 cargo make build 保持一致：
#   1) 检查正文写作约定（列表标记、标题层级、内部死链）
#   2) 生成 SUMMARY.md、各章节首页与主题索引页
#   3) mdbook build（出现 WARN/ERROR 即失败，避免把内容丢失的产物打进镜像）
#   4) 移除 print.html、标记导航页为不索引
#   5) Pagefind 建索引（仅索引 <main> 正文）
RUN set -eux; \
    ./target/release/xtask lint; \
    ./target/release/xtask summary; \
    mdbook build 2>&1 | tee /tmp/build.log; \
    if grep -qE 'ERROR|WARN' /tmp/build.log; then \
      echo '构建存在 ERROR/WARN，中止：'; grep -E 'ERROR|WARN' /tmp/build.log; exit 1; \
    fi; \
    ./target/release/xtask prep-index; \
    pagefind --site book --output-subdir _pagefind --root-selector main; \
    test -f book/_pagefind/pagefind.js

# ---------------------------------------------------------------- 运行阶段
FROM nginx:1.27-alpine

# 移除默认站点配置，避免与自定义配置冲突
RUN rm -f /etc/nginx/conf.d/default.conf

COPY docker/nginx.conf /etc/nginx/conf.d/notes.conf
COPY --from=builder /build/book /usr/share/nginx/html

EXPOSE 80

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q --spider http://127.0.0.1/index.html || exit 1

STOPSIGNAL SIGQUIT

CMD ["nginx", "-g", "daemon off;"]
