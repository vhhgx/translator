//! TXT / Markdown document translation.
//!
//! Decoding is charset-aware (UTF-8, GBK, GB18030, Big5, Shift-JIS...) because
//! the user works with Chinese files that are frequently not UTF-8.
//! Markdown is split into segments so that structure (headings, lists, code
//! fences, tables) survives translation.

use serde::Serialize;
use std::fs;
use std::path::Path;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
pub struct DocumentSegment {
    pub index: usize,
    /// Translated? `false` for code fences and other content we must not touch.
    pub translatable: bool,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadedDocument {
    pub path: String,
    pub encoding: String,
    pub kind: String,
    pub segments: Vec<DocumentSegment>,
    /// The whole file, reassembled from segments (untranslated).
    pub raw: String,
    /// Line count of the original.
    pub lines: usize,
}

/// Decode bytes, honouring a BOM, then sniffing for a Chinese legacy encoding.
pub fn decode(bytes: &[u8]) -> (String, String) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (
            String::from_utf8_lossy(&bytes[3..]).into_owned(),
            "UTF-8 (BOM)".into(),
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (
            utf16_decode(&bytes[2..], true),
            "UTF-16 LE".into(),
        );
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return (
            utf16_decode(&bytes[2..], false),
            "UTF-16 BE".into(),
        );
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), "UTF-8".into());
    }
    // Not valid UTF-8 — most likely a Chinese legacy encoding. Try GB18030
    // (a superset of GBK/GB2312) first, then Big5.
    for (enc, label) in [
        (&encoding_rs::GB18030, "GB18030/GBK"),
        (&encoding_rs::BIG5, "Big5"),
        (&encoding_rs::SHIFT_JIS, "Shift_JIS"),
        (&encoding_rs::WINDOWS_1252, "Windows-1252"),
    ] {
        let (text, _, _) = enc.decode(bytes);
        if !text.contains('\u{FFFD}') {
            return (text.into_owned(), label.into());
        }
    }
    (String::from_utf8_lossy(bytes).into_owned(), "未知".into())
}

fn utf16_decode(bytes: &[u8], little_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if little_endian {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// True when the content looks like Markdown worth segmenting.
fn looks_like_markdown(text: &str) -> bool {
    text.lines().take(200).any(|l| {
        let t = l.trim_start();
        t.starts_with("#")
            || t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with("> ")
            || t.starts_with("|")
            || t.starts_with("```")
            || l.contains("](")
    })
}

/// Split Markdown into translatable / protected segments.
pub fn segment_markdown(text: &str) -> Vec<DocumentSegment> {
    let mut segments: Vec<DocumentSegment> = Vec::new();
    let mut buf = String::new();
    let mut in_fence = false;
    let mut idx = 0usize;

    let flush = |segments: &mut Vec<DocumentSegment>, buf: &mut String, kind: &str| {
        if !buf.trim().is_empty() {
            segments.push(DocumentSegment {
                index: idx,
                translatable: true,
                kind: kind.to_string(),
                text: std::mem::take(buf),
            });
        }
    };

    let mut fence_buf = String::new();

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();

        // Code fence: hold everything verbatim until the closing fence.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if in_fence {
                fence_buf.push_str(line);
                segments.push(DocumentSegment {
                    index: idx,
                    translatable: false,
                    kind: "code".into(),
                    text: std::mem::take(&mut fence_buf),
                });
                in_fence = false;
            } else {
                if !buf.trim().is_empty() {
                    segments.push(DocumentSegment {
                        index: idx,
                        translatable: true,
                        kind: "prose".into(),
                        text: std::mem::take(&mut buf),
                    });
                }
                fence_buf.push_str(line);
                in_fence = true;
            }
            idx += 1;
            continue;
        }

        if in_fence {
            fence_buf.push_str(line);
            continue;
        }

        // Front-matter / YAML header.
        if trimmed.starts_with("---") && buf.is_empty() {
            segments.push(DocumentSegment {
                index: idx,
                translatable: false,
                kind: "frontmatter".into(),
                text: line.to_string(),
            });
            idx += 1;
            continue;
        }

        let kind = if trimmed.starts_with('#') {
            "heading"
        } else if trimmed.starts_with('|') {
            "table"
        } else if trimmed.starts_with('>') {
            "quote"
        } else if trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) && trimmed.contains(". ")
        {
            "list"
        } else {
            "prose"
        };

        if kind != "prose" && !buf.trim().is_empty() {
            segments.push(DocumentSegment {
                index: idx,
                translatable: true,
                kind: "prose".into(),
                text: std::mem::take(&mut buf),
            });
            idx += 1;
        }
        buf.push_str(line);
    }

    if in_fence && !fence_buf.is_empty() {
        segments.push(DocumentSegment {
            index: idx,
            translatable: false,
            kind: "code".into(),
            text: fence_buf,
        });
        idx += 1;
    }
    flush(&mut segments, &mut buf, "prose");
    for (i, s) in segments.iter_mut().enumerate() {
        s.index = i;
    }
    segments
}

/// Reassemble a document from translated segments, keeping structure markers.
pub fn reassemble(segments: &[DocumentSegment], translations: &[(usize, String)]) -> String {
    let mut out = String::new();
    for s in segments {
        match translations.iter().find(|(i, _)| *i == s.index) {
            Some((_, t)) => {
                let mut t = t.clone();
                if !t.ends_with('\n') && s.text.ends_with('\n') {
                    t.push('\n');
                }
                out.push_str(&t);
            }
            None => out.push_str(&s.text),
        }
    }
    out
}

pub fn load(path: &str) -> AppResult<LoadedDocument> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "txt" | "md" | "markdown") {
        return Err(AppError::Io(format!(
            "暂只支持 .txt / .md，收到 .{ext}"
        )));
    }
    let bytes = fs::read(p).map_err(|e| AppError::Io(format!("读取失败: {e}")))?;
    let (text, encoding) = decode(&bytes);
    let kind = if matches!(ext.as_str(), "md" | "markdown") {
        "markdown"
    } else {
        "txt"
    };
    let segments = if kind == "markdown" || looks_like_markdown(&text) {
        segment_markdown(&text)
    } else {
        // Plain text: split into paragraph-ish chunks so long files don't
        // blow the context window and translation stays resumable.
        text.split("\n\n")
            .enumerate()
            .filter(|(_, c)| !c.trim().is_empty())
            .map(|(i, c)| DocumentSegment {
                index: i,
                translatable: true,
                kind: "prose".into(),
                text: c.to_string(),
            })
            .collect()
    };
    Ok(LoadedDocument {
        path: path.to_string(),
        encoding,
        kind: kind.to_string(),
        lines: text.lines().count(),
        raw: text.clone(),
        segments,
    })
}

/// Guess an output path next to the input, with a `.translated` marker.
pub fn output_path(input: &str, target_lang: &str) -> String {
    let p = Path::new(input);
    let parent = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("document");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("txt");
    let tag = if target_lang.chars().all(|c| c.is_ascii_alphanumeric()) {
        target_lang.to_string()
    } else {
        "zh-CN".into()
    };
    parent.join(format!("{stem}.{tag}.{ext}")).to_string_lossy().into_owned()
}

pub fn save(path: &str, content: &str) -> AppResult<()> {
    fs::write(path, content).map_err(|e| AppError::Io(format!("写入失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_and_gbk() {
        let (s, enc) = decode("你好".as_bytes());
        assert_eq!(s, "你好");
        assert_eq!(enc, "UTF-8");

        let mut gbk = Vec::new();
        let (cow, _, _) = encoding_rs::GB18030.encode("中文编码测试");
        gbk.extend_from_slice(cow);
        let (s2, enc2) = decode(&gbk);
        assert_eq!(s2, "中文编码测试");
        assert!(enc2.contains("GB"), "got {enc2}");
    }

    #[test]
    fn markdown_keeps_code_fence_verbatim() {
        let md = "# 标题\n\n正文一段。\n\n```rust\nlet x = 1; // 不要翻译\n```\n\n结尾。\n";
        let segs = segment_markdown(md);
        assert!(segs.iter().any(|s| s.kind == "code" && !s.translatable));
        assert!(segs.iter().any(|s| s.kind == "heading"));
        // code text survives round-trip untouched
        let out = reassemble(&segs, &[]);
        assert_eq!(out, md);
    }

    #[test]
    fn reassemble_substitutes_only_translated() {
        let md = "# 标题\n\n正文。\n";
        let segs = segment_markdown(md);
        let head = segs.iter().find(|s| s.kind == "heading").unwrap();
        let out = reassemble(&segs, &[(head.index, "# Title\n".to_string())]);
        assert!(out.starts_with("# Title"));
        assert!(out.contains("正文。"));
    }

    #[test]
    fn output_path_gets_suffix() {
        assert_eq!(
            output_path("C:/a/b/readme.md", "英语"),
            "C:\\a\\b\\readme.英语.md".replace('\\', "/")
        );
        assert_eq!(
            output_path("notes.txt", "zh-CN"),
            "notes.zh-CN.txt"
        );
    }
}