//! Translation prompt templates.
//!
//! Ported byte-for-byte from the official `hy-mt2-translator` skill
//! (`docs/upstream/hy_translate.py::build_prompt` and
//! `docs/upstream/translation-modes.md`). Do not "improve" the wording —
//! the model's behaviour is tuned to these exact strings.

use serde::{Deserialize, Serialize};

/// 38 languages the model supports, abbreviation -> Chinese name.
pub const SUPPORTED_LANGUAGES: [(&str, &str); 38] = [
    ("zh", "中文"),
    ("en", "英语"),
    ("fr", "法语"),
    ("pt", "葡萄牙语"),
    ("es", "西班牙语"),
    ("ja", "日语"),
    ("tr", "土耳其语"),
    ("ru", "俄语"),
    ("ar", "阿拉伯语"),
    ("ko", "韩语"),
    ("th", "泰语"),
    ("it", "意大利语"),
    ("de", "德语"),
    ("vi", "越南语"),
    ("ms", "马来语"),
    ("id", "印尼语"),
    ("tl", "菲律宾语"),
    ("hi", "印地语"),
    ("zh-Hant", "繁体中文"),
    ("pl", "波兰语"),
    ("cs", "捷克语"),
    ("nl", "荷兰语"),
    ("km", "高棉语"),
    ("my", "缅甸语"),
    ("fa", "波斯语"),
    ("gu", "古吉拉特语"),
    ("ur", "乌尔都语"),
    ("te", "泰卢固语"),
    ("mr", "马拉地语"),
    ("he", "希伯来语"),
    ("bn", "孟加拉语"),
    ("ta", "泰米尔语"),
    ("uk", "乌克兰语"),
    ("bo", "藏语"),
    ("kk", "哈萨克语"),
    ("mn", "蒙古语"),
    ("ug", "维吾尔语"),
    ("yue", "粤语"),
];

pub const STYLES: [&str; 12] = [
    "日常口语化风格",
    "学术论文严谨风格",
    "科普语气",
    "商业正式语气",
    "新闻报道风格",
    "宣传文案风格",
    "小说风格",
    "法律合同风格",
    "中国古代文言文",
    "技术文档风格",
    "字幕口语风格",
    "儿童易懂风格",
];

pub const FORMAT_TYPES: [&str; 5] = ["JSON", "HTML", "XML", "YAML", "Markdown"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Basic,
    Terminology,
    Style,
    Context,
    Delimiter,
    Structured,
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::Basic,
        Mode::Terminology,
        Mode::Style,
        Mode::Context,
        Mode::Delimiter,
        Mode::Structured,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Basic => "基础翻译",
            Mode::Terminology => "术语约束",
            Mode::Style => "风格控制",
            Mode::Context => "背景语境",
            Mode::Delimiter => "分隔符保留",
            Mode::Structured => "结构化数据",
        }
    }
}

/// Resolve an abbreviation ("en") or Chinese name ("英语") to the Chinese name.
///
/// Prompts always interpolate the **Chinese name**, so both input forms must
/// produce it — returning the abbreviation here would put "翻译为zh" into the
/// prompt and degrade the model's output.
pub fn resolve_language(lang: &str) -> Result<String, String> {
    let lang = lang.trim();
    if let Some((_, name)) = SUPPORTED_LANGUAGES.iter().find(|(abbr, _)| *abbr == lang) {
        return Ok(name.to_string());
    }
    if SUPPORTED_LANGUAGES.iter().any(|(_, name)| *name == lang) {
        return Ok(lang.to_string());
    }
    Err(format!(
        "不支持的语言: {lang:?}\n请使用缩写(如 en)或中文名(如 英语)之一。"
    ))
}

/// Render terminology pairs as `src翻译成dst` lines, exactly as the skill specifies.
///
/// When `reverse` is set the pair is flipped, which is what you want when the
/// glossary is stored zh->en but the translation direction is en->zh.
pub fn terminology_string(pairs: &[[String; 2]], reverse: bool) -> String {
    pairs
        .iter()
        .filter(|pair| !pair[0].trim().is_empty() && !pair[1].trim().is_empty())
        .map(|pair| {
            let (a, b) = (&pair[0], &pair[1]);
            if reverse {
                format!("{}翻译成{}", b.trim(), a.trim())
            } else {
                format!("{}翻译成{}", a.trim(), b.trim())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the prompt for a translation request.
pub fn build_prompt(
    source_text: &str,
    target_lang: &str,
    mode: Mode,
    terminology: &str,
    style: &str,
    format_type: &str,
    context: &str,
) -> String {
    match mode {
        Mode::Basic => format!(
            "将以下文本翻译为{target_lang}，注意只需要输出翻译后的结果，不要额外解释：\n\n{source_text}"
        ),
        Mode::Terminology => format!(
            "参考下面的翻译：\n{terminology}\n将以下文本翻译为{target_lang}，注意只需要输出翻译后的结果，不要额外解释：\n\n{source_text}"
        ),
        Mode::Style => format!(
            "请将以下文本翻译为{target_lang}。\n注意翻译的风格要严格符合【{style}】\n\n{source_text}"
        ),
        Mode::Context => format!(
            "【背景信息】\n{context}\n\n请结合背景信息将以下文本翻译为{target_lang}。\n\n【待翻译文本】\n{source_text}"
        ),
        Mode::Delimiter => format!(
            "请将以下文本准确翻译为{target_lang}。\n你必须在译文中保留等量的分隔符，绝对不可遗漏、转义或翻译该符号，并注意分隔符的位置。\n\n{source_text}"
        ),
        Mode::Structured => format!(
            "请将以下{format_type} 结构化数据翻译为{target_lang}。\n绝对保持原有的 {format_type} 数据结构、缩进和层级完全不变。仅翻译面向用户展示的可见文本内容。禁止翻译代码标签、键名（key）、变量占位符或任何代码属性。\n\n\n{source_text}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden values captured from the official `build_prompt`. If a template
    /// drifts, these fail — that is the whole point.
    #[test]
    fn prompts_match_official_skill() {
        let t = "英语";
        assert_eq!(
            build_prompt("X", t, Mode::Basic, "", "", "", ""),
            "将以下文本翻译为英语，注意只需要输出翻译后的结果，不要额外解释：\n\nX"
        );
        assert_eq!(
            build_prompt("X", t, Mode::Terminology, "A翻译成B", "", "", ""),
            "参考下面的翻译：\nA翻译成B\n将以下文本翻译为英语，注意只需要输出翻译后的结果，不要额外解释：\n\nX"
        );
        assert_eq!(
            build_prompt("X", t, Mode::Style, "", "学术论文严谨风格", "", ""),
            "请将以下文本翻译为英语。\n注意翻译的风格要严格符合【学术论文严谨风格】\n\nX"
        );
        assert_eq!(
            build_prompt("X", t, Mode::Context, "", "", "", "医学场景"),
            "【背景信息】\n医学场景\n\n请结合背景信息将以下文本翻译为英语。\n\n【待翻译文本】\nX"
        );
        assert_eq!(
            build_prompt("X", t, Mode::Delimiter, "", "", "", ""),
            "请将以下文本准确翻译为英语。\n你必须在译文中保留等量的分隔符，绝对不可遗漏、转义或翻译该符号，并注意分隔符的位置。\n\nX"
        );
        assert_eq!(
            build_prompt("X", t, Mode::Structured, "", "", "JSON", ""),
            "请将以下JSON 结构化数据翻译为英语。\n绝对保持原有的 JSON 数据结构、缩进和层级完全不变。仅翻译面向用户展示的可见文本内容。禁止翻译代码标签、键名（key）、变量占位符或任何代码属性。\n\n\nX"
        );
    }

    #[test]
    fn language_resolution() {
        // Both forms must resolve to the Chinese name, because prompts
        // interpolate it directly.
        assert_eq!(resolve_language("en").unwrap(), "英语");
        assert_eq!(resolve_language("英语").unwrap(), "英语");
        assert_eq!(resolve_language(" zh ").unwrap(), "中文");
        assert_eq!(resolve_language("中文").unwrap(), "中文");
        assert!(resolve_language("klingon").is_err());
    }

    #[test]
    fn terminology_reversal() {
        let pairs = vec![["订单".to_string(), "order".to_string()]];
        assert_eq!(terminology_string(&pairs, false), "订单翻译成order");
        assert_eq!(terminology_string(&pairs, true), "order翻译成订单");
    }
}