//! 网页正文抽取。
//!
//! 目标：给一个 URL，抓下来、去掉导航/脚本/样式，抽出「人真正要读的正文」，
//! 切成可翻译的段落。刻意不做完整的阅读模式算法（那不是这个工具的职责），
//! 但把噪音挡在外面——导航栏、页脚、脚本、广告位翻出来只会干扰。
//!
//! 只支持静态 HTML。SPA（React/Vue 站点）拿到的往往是空壳，正文靠 JS 渲染，
//! 这里抓不到——那种情况会在结果里如实说明，而不是假装成功。

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 抽取结果的正文块。保留 `tag` 是为了前端能区分标题和段落，排版更自然。
///
/// 需要 `Deserialize`：前端会把 `web_fetch` 的结果原样回传给 `web_translate`，
/// 它得作为命令参数被反序列化。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebBlock {
    /// 粗粒度标签：h1/h2/h3/p/li/blockquote/pre。
    pub tag: String,
    pub text: String,
    /// 代码块等不该翻译的内容。
    pub translatable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebPage {
    /// 最终 URL（跟随跳转后的）。
    pub url: String,
    pub title: String,
    pub blocks: Vec<WebBlock>,
    /// 抓取到的原始 HTML 字节数，用于让用户判断是不是拿到了空壳。
    pub bytes: usize,
}

/// 命中这些标签就整块丢掉——它们几乎从不含正文。
const DROP_TAGS: [&str; 14] = [
    "script", "style", "noscript", "svg", "iframe", "nav", "footer", "header", "aside", "form",
    "button", "select", "template", "canvas",
];

/// 这些标签自带块级语义，逐个抽成独立段落。
const BLOCK_TAGS: [&str; 12] = [
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "blockquote", "pre", "td", "th",
];

/// 从 HTML 里抽出正文块。
pub fn extract(html: &str, url: &str) -> WebPage {
    let doc = scraper::Html::parse_document(html);

    let title = doc
        .select(&scraper::Selector::parse("title").unwrap())
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| url.to_string());

    // 先删掉噪音，避免它们的文字混进遍历结果。
    // scraper 不允许改动树，所以这里改成「遍历时跳过」。用一个已丢弃
    // 祖先集合来剪枝：
    let mut blocks: Vec<WebBlock> = Vec::new();
    let sel = scraper::Selector::parse("h1, h2, h3, h4, h5, h6, p, li, blockquote, pre, td, th")
        .expect("static selector");

    for el in doc.select(&sel) {
        if has_dropped_ancestor(&el) {
            continue;
        }
        // 嵌套块（li > p、blockquote > p）会重复：外层把内层的文字也包含进去，
        // 两个都抽就会输出两遍。规则是「谁最内层谁负责」——只要后代里还有块级
        // 元素，就把这一层让出去。
        if has_block_descendant(&el) {
            continue;
        }

        let tag = el.value().name().to_string();
        let text = normalize(&el.text().collect::<String>());
        if text.is_empty() {
            continue;
        }
        // 超短且没有标点的碎片多半是导航/标签，丢掉。
        if text.chars().count() < 2 {
            continue;
        }

        let translatable = tag != "pre";
        blocks.push(WebBlock {
            tag,
            text,
            translatable,
        });
    }

    WebPage {
        url: url.to_string(),
        title,
        blocks,
        bytes: html.len(),
    }
}

/// 祖先里有没有被丢弃的标签（script/nav 等）。
fn has_dropped_ancestor(el: &scraper::ElementRef) -> bool {
    el.ancestors().any(|a| {
        a.value()
            .as_element()
            .map(|e| DROP_TAGS.contains(&e.name()))
            .unwrap_or(false)
    })
}

/// 这个元素内部还有没有别的块级元素。有的话内层会自己输出，本层跳过，
/// 否则同一段文字会被抽两次。
fn has_block_descendant(el: &scraper::ElementRef) -> bool {
    el.descendants().skip(1).any(|d| {
        d.value()
            .as_element()
            .map(|e| BLOCK_TAGS.contains(&e.name()))
            .unwrap_or(false)
    })
}

/// 折叠空白：HTML 源码里的换行和缩进不是内容的换行。
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// 抓取一个 URL 并解出正文。
///
/// `client` 由调用方传入，复用主客户端的代理/证书配置。
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
) -> AppResult<WebPage> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::Io(
            "URL 需要以 http:// 或 https:// 开头".into(),
        ));
    }

    let resp = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/124.0 Safari/537.36 ZephyrTranslator/0.1",
        )
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .send()
        .await
        .map_err(|e| AppError::Io(format!("抓取失败: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Io(format!("抓取失败: HTTP {}", resp.status())));
    }

    let final_url = resp.url().to_string();

    // 按响应头里的 charset 解码；没有就交给 charset feature 猜。
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Io(format!("读取响应失败: {e}")))?;

    let page = extract(&body, &final_url);
    if page.blocks.is_empty() {
        return Err(AppError::Io(
            "没抽到正文。该页面可能是靠 JS 渲染的（单页应用），或需要登录。".into(),
        ));
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实网络冒烟测试。默认忽略：CI 无网时不该红。
    /// 手动跑：cargo test -- --ignored live_fetch_smoke
    #[tokio::test]
    #[ignore]
    async fn live_fetch_smoke() {
        let c = crate::client::web_client(30).expect("client");
        let page = fetch(&c, "https://example.com").await.expect("fetch");
        println!("title={:?} blocks={}", page.title, page.blocks.len());
        for b in &page.blocks {
            println!("  [{}] {}", b.tag, b.text);
        }
        assert!(!page.blocks.is_empty());
    }

    #[test]
    fn drops_scripts_nav_and_styles() {
        let html = r#"
            <html><head><title>标题</title><style>p{color:red}</style></head>
            <body>
              <nav><a href="/">首页</a><a href="/a">栏目</a></nav>
              <h1>真正的主标题</h1>
              <p>第一段正文。</p>
              <script>var x = '不该出现';</script>
              <footer>版权信息</footer>
            </body></html>"#;
        let page = extract(html, "https://example.com");
        let texts: Vec<&str> = page.blocks.iter().map(|b| b.text.as_str()).collect();
        assert!(texts.contains(&"真正的主标题"), "got {texts:?}");
        assert!(texts.contains(&"第一段正文。"), "got {texts:?}");
        assert!(!texts.iter().any(|t| t.contains("不该出现")), "script 泄漏");
        assert!(!texts.iter().any(|t| t.contains("首页")), "nav 泄漏");
        assert!(!texts.iter().any(|t| t.contains("版权")), "footer 泄漏");
        assert!(!texts.iter().any(|t| t.contains("color:red")), "style 泄漏");
    }

    #[test]
    fn title_falls_back_to_url() {
        let page = extract("<html><body><p>正文内容</p></body></html>", "https://x.com/p");
        assert_eq!(page.title, "https://x.com/p");
    }

    #[test]
    fn pre_is_not_translatable() {
        let html = "<html><body><pre>let x = 1;</pre><p>说明文字</p></body></html>";
        let page = extract(html, "https://example.com");
        let pre = page.blocks.iter().find(|b| b.tag == "pre").expect("pre");
        assert!(!pre.translatable);
        let p = page.blocks.iter().find(|b| b.tag == "p").expect("p");
        assert!(p.translatable);
    }

    #[test]
    fn nested_blocks_are_not_duplicated() {
        // li 里套 p：只应抽到内层的 p，否则同一句话出现两次。
        let html = "<html><body><ul><li><p>列表项正文</p></li></ul></body></html>";
        let page = extract(html, "https://example.com");
        let hits = page
            .blocks
            .iter()
            .filter(|b| b.text.contains("列表项正文"))
            .count();
        assert_eq!(hits, 1, "嵌套块重复: {:?}", page.blocks);
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize("a\n\n   b\t c "), "a b c");
        assert_eq!(normalize("   "), "");
    }
}
