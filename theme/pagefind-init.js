// Pagefind 搜索集成
// 背景：mdBook 自带搜索使用 elasticlunr，按空白字符分词，中文内容无法建立索引
//      （实测搜索索引中 CJK token 数为 0）。此脚本改用 Pagefind 提供中文可用的全文检索。
(function () {
  "use strict";

  var PAGEFIND_JS = "_pagefind/pagefind.js";
  var pagefind = null;
  var initPromise = null;
  var debounceTimer = null;

  // 计算站点根路径，兼容 GitHub Pages 子路径部署（如 /markdown/）
  //
  // mdBook 在每页注入 `const path_to_root = "../"`（按页面深度生成）。
  // 注意它用 const 声明，不会挂到 window 上，需通过全局标识符直接访问，
  // 因此用 typeof 保护避免未定义时抛错。
  // 不要拿 css/general.css 当锚点：mdBook 0.5 起该文件名带哈希后缀，匹配会失败。
  function siteRoot() {
    var root;
    try {
      root = typeof path_to_root === "string" ? path_to_root : null;
    } catch (e) {
      root = null;
    }
    if (root === null) {
      // 兜底：从任一 css 资源反推层级
      var el = document.querySelector('link[rel="stylesheet"][href*="css/general"]');
      if (el) {
        var href = el.getAttribute("href");
        var idx = href.indexOf("css/general");
        if (idx >= 0) root = href.slice(0, idx);
      }
    }
    return new URL(root || "./", location.href).href;
  }

  function resolve(p) {
    return new URL(p.replace(/^\//, ""), siteRoot()).href;
  }

  function loadPagefind() {
    if (initPromise) return initPromise;
    initPromise = import(resolve(PAGEFIND_JS))
      .then(function (mod) {
        pagefind = mod;
        return pagefind.options({
          // 中文（zh-cn）无词干还原，分词后单字容易造成误命中；
          // 提高标题权重，让「所有权」这类查询优先匹配同名笔记
          ranking: {
            pageLength: 0.6,
            termFrequency: 0.8,
            termSaturation: 1.2,
            termSimilarity: 1.5
          }
        });
      })
      .then(function () {
        return pagefind.init();
      })
      .then(function () {
        return pagefind;
      })
      .catch(function (err) {
        console.warn("[pagefind] 索引未加载，请先执行 ./build.sh 生成索引", err);
        return null;
      });
    return initPromise;
  }

  function buildUI() {
    var wrapper = document.createElement("div");
    wrapper.id = "pagefind-wrapper";
    wrapper.className = "pagefind-hidden";
    wrapper.innerHTML =
      '<div id="pagefind-bar">' +
      '<input type="search" id="pagefind-input" placeholder="搜索（支持中文）…" ' +
      'autocomplete="off" autocapitalize="off" spellcheck="false" aria-label="搜索">' +
      "</div>" +
      '<div id="pagefind-hint">按 Esc 关闭，点击标签可筛选</div>' +
      '<div id="pagefind-filters"></div>' +
      '<div id="pagefind-results" role="listbox"></div>';
    return wrapper;
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

  // 渲染标签筛选条（数据来自 clipping 笔记中的 data-pagefind-filter）
  function renderFilters(container, filters, active, onToggle) {
    var names = Object.keys(filters).filter(function (n) { return n === "标签"; });
    if (!names.length) {
      container.innerHTML = "";
      return;
    }
    container.innerHTML = "";
    names.forEach(function (name) {
      var values = Object.keys(filters[name]).filter(function (v) { return filters[name][v] > 0; });
      if (!values.length) return;
      var row = document.createElement("div");
      row.className = "pagefind-filter-row";
      row.innerHTML = '<span class="pagefind-filter-label">' + escapeHtml(name) + "</span>";
      values.sort().forEach(function (v) {
        var chip = document.createElement("button");
        chip.type = "button";
        chip.className = "pagefind-chip" + (active[name] === v ? " pagefind-chip-active" : "");
        chip.textContent = v + " (" + filters[name][v] + ")";
        chip.addEventListener("click", function () { onToggle(name, v); });
        row.appendChild(chip);
      });
      container.appendChild(row);
    });
  }

  // 站内链接可能是「站点根相对」的形式（Pagefind 返回 /rust/xxx.html），
  // 在子路径部署下必须重新基于站点根解析，否则会指向域名根而 404。
  // 另外 r.url 中的中文未做百分号编码（sub_results 里的则已编码），统一处理。
  function pageUrl(u) {
    if (!u) return "#";
    var hash = "";
    var hashAt = u.indexOf("#");
    if (hashAt >= 0) {
      hash = u.slice(hashAt);
      u = u.slice(0, hashAt);
    }
    // 已编码的路径不要二次编码，先解码再编码保证幂等
    var path = u.replace(/^\//, "").split("/").map(function (seg) {
      return encodeURIComponent(decodeURIComponent(seg));
    }).join("/");
    return new URL(path + hash, siteRoot()).href;
  }

  function render(container, results, query) {
    if (!query) {
      container.innerHTML = "";
      return;
    }
    if (!results.length) {
      container.innerHTML = '<div class="pagefind-empty">未找到与「' + escapeHtml(query) + "」相关的内容</div>";
      return;
    }
    container.innerHTML = '<div class="pagefind-count">共 ' + results.length + " 条结果</div>";
    var frag = document.createDocumentFragment();
    results.slice(0, 30).forEach(function (r) {
      var title = r.meta && r.meta.title ? r.meta.title : r.url;
      var tags = (r.filters && r.filters["标签"]) || [];
      var categories = (r.filters && r.filters["分类"]) || [];
      // Pagefind 提取标题时会把紧随 H1 的徽标文字一并计入（如
      // "三体三部曲小说科幻"）。标签已单独渲染，这里从标题尾部剥离，
      // 避免重复。按长度降序剥离，防止短标签是长标签的前缀时截断出错。
      var noise = tags.concat(categories).sort(function (a, b) {
        return b.length - a.length;
      });
      var changed = true;
      while (changed) {
        changed = false;
        for (var i = 0; i < noise.length; i++) {
          if (noise[i] && title.length > noise[i].length && title.endsWith(noise[i])) {
            title = title.slice(0, -noise[i].length);
            changed = true;
          }
        }
      }
      title = title.trim();

      var tagHtml = "";
      if (tags.length) {
        tagHtml = '<span class="pagefind-result-tags">' +
          tags.map(function (t) {
            return "<em>" + escapeHtml(t) + "</em>";
          }).join("") + "</span>";
      }

      // 优先使用 sub_results 的锚点链接：长文档（如 237 行的智能指针.md）
      // 只跳到页首等于没跳，带 #anchor 才能直接定位到命中段落。
      //
      // 选取策略：每个 sub_result 自带 locations（该小节内命中词的位置），
      // 取命中次数最多的小节即最相关的一节；并列时取靠前的。
      //
      // 两个易错点：
      //   1. 不能取 sub_results[0]，它通常是 H1（页面标题），跳过去仍是页首；
      //   2. 不能用顶层 r.locations 与 anchor.location 比较——虽然数值看似
      //      可比，但顶层 locations 覆盖整篇（含 H1 段），会误选到靠前的小节。
      var subs = (r.sub_results || []).filter(function (s) {
        return s.url && s.url.indexOf("#") >= 0;
      });
      var deeper = subs.filter(function (s) {
        return !(s.anchor && s.anchor.element === "h1");
      });

      function hitCount(s) {
        return s.locations ? s.locations.length : 0;
      }
      var best = null;
      deeper.forEach(function (s) {
        if (!best || hitCount(s) > hitCount(best)) best = s;
      });
      if (!best || hitCount(best) === 0) best = deeper[0] || subs[0];
      var primaryUrl = best ? best.url : r.url;

      var item = document.createElement("a");
      item.className = "pagefind-result";
      item.href = pageUrl(primaryUrl);
      item.setAttribute("role", "option");
      item.innerHTML =
        '<div class="pagefind-result-title">' + escapeHtml(title) + tagHtml + "</div>" +
        '<div class="pagefind-result-excerpt">' + r.excerpt + "</div>";

      // 同一文档命中多个小节时列出，便于直接跳到需要的位置。
      // 排除 H1（跳过去就是页首，没有导航价值）；按命中次数降序，
      // 让最相关的小节排在前面，避免用户在长文里逐个试。
      if (deeper.length > 1) {
        var ordered = deeper.slice().sort(function (a, b) {
          return hitCount(b) - hitCount(a);
        });
        var list = document.createElement("div");
        list.className = "pagefind-subresults";
        ordered.slice(0, 5).forEach(function (s) {
          var sa = document.createElement("a");
          sa.className = "pagefind-subresult";
          sa.href = pageUrl(s.url);
          sa.textContent = s.title || "(片段)";
          if (s === best) sa.classList.add("pagefind-subresult-best");
          list.appendChild(sa);
        });
        var wrap = document.createElement("div");
        wrap.className = "pagefind-result-group";
        wrap.appendChild(item);
        wrap.appendChild(list);
        frag.appendChild(wrap);
        return;
      }

      frag.appendChild(item);
    });
    container.appendChild(frag);
  }

  var activeFilters = {};

  function doSearch(query, container, filterContainer) {
    if (!query || !query.trim()) {
      container.innerHTML = "";
      if (filterContainer) filterContainer.innerHTML = "";
      return;
    }
    loadPagefind().then(function (pf) {
      if (!pf) {
        container.innerHTML =
          '<div class="pagefind-empty">搜索索引不可用。请运行 <code>./build.sh</code> 生成 Pagefind 索引。</div>';
        return;
      }
      var opts = {};
      var used = Object.keys(activeFilters).filter(function (k) { return activeFilters[k]; });
      if (used.length) {
        opts.filters = {};
        used.forEach(function (k) { opts.filters[k] = activeFilters[k]; });
      }
      // 注意：search() 返回的 filters 在启用筛选时为空，需用全局 filters() 获取可选值
      Promise.all([pf.search(query, opts), pf.filters()]).then(function (both) {
        var search = both[0];
        var allFilters = both[1] || {};
        if (filterContainer) {
          renderFilters(filterContainer, allFilters, activeFilters, function (name, value) {
            activeFilters[name] = activeFilters[name] === value ? null : value;
            doSearch(query, container, filterContainer);
          });
        }
        return Promise.all(search.results.slice(0, 30).map(function (r) { return r.data(); }));
      }).then(function (data) {
        render(container, data, query);
      }).catch(function (err) {
        console.error("[pagefind] 搜索失败", err);
        container.innerHTML = '<div class="pagefind-empty">搜索出错，详见控制台。</div>';
      });
    });
  }

  function init() {
    var wrapper = buildUI();
    var content = document.querySelector("#content") || document.querySelector("main");
    if (!content) return;
    content.parentNode.insertBefore(wrapper, content);
    var input = wrapper.querySelector("#pagefind-input");
    var results = wrapper.querySelector("#pagefind-results");
    var filters = wrapper.querySelector("#pagefind-filters");

    function open() {
      wrapper.classList.remove("pagefind-hidden");
      input.focus();
      input.select();
    }
    function close() {
      wrapper.classList.add("pagefind-hidden");
      results.innerHTML = "";
      filters.innerHTML = "";
      input.value = "";
      activeFilters = {};
    }
    function toggle() {
      if (wrapper.classList.contains("pagefind-hidden")) open();
      else close();
    }

    // 点击结果后关闭搜索面板。
    // 关键：跳转到当前页的锚点时浏览器不会重新加载，若面板仍占据页面顶部，
    // 视觉上就像「点了没反应」。这里主动收起面板并让浏览器完成锚点滚动。
    results.addEventListener("click", function (e) {
      var link = e.target.closest("a.pagefind-result, a.pagefind-subresult");
      if (!link) return;
      var samePage = link.pathname === location.pathname;
      close();
      if (samePage && link.hash) {
        // 面板收起后布局已变化，重新触发一次锚点定位
        e.preventDefault();
        location.hash = link.hash;
        var el = document.getElementById(decodeURIComponent(link.hash.slice(1)));
        if (el) el.scrollIntoView({ block: "start" });
      }
    });

    // 复用 mdBook 原有的搜索按钮（默认搜索关闭后该按钮不会出现，故自行创建）
    var btn = document.createElement("button");
    btn.id = "pagefind-toggle";
    btn.className = "icon-button";
    btn.type = "button";
    btn.title = "搜索（按 s 快速唤起）";
    btn.setAttribute("aria-label", "搜索");
    btn.innerHTML = '<i class="fa fa-search"></i>';
    btn.addEventListener("click", toggle);
    var leftButtons = document.querySelector(".left-buttons");
    if (leftButtons) leftButtons.appendChild(btn);

    input.addEventListener("input", function () {
      clearTimeout(debounceTimer);
      var q = input.value;
      debounceTimer = setTimeout(function () { doSearch(q, results, filters); }, 180);
    });

    document.addEventListener("keydown", function (e) {
      var tag = (e.target.tagName || "").toLowerCase();
      var typing = tag === "input" || tag === "textarea" || e.target.isContentEditable;
      if (e.key === "Escape" && !wrapper.classList.contains("pagefind-hidden")) {
        close();
        e.preventDefault();
        return;
      }
      // 沿用 mdBook 习惯：按 s 唤起搜索
      if (!typing && (e.key === "s" || e.key === "S") && !e.ctrlKey && !e.metaKey && !e.altKey) {
        open();
        e.preventDefault();
      }
      // 兼容 Cmd/Ctrl+K
      if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
        open();
        e.preventDefault();
      }
    });

    // 支持 ?search=xxx 直接跳转搜索
    var params = new URLSearchParams(location.search);
    var initial = params.get("search");
    if (initial) {
      open();
      input.value = initial;
      doSearch(initial, results, filters);
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
