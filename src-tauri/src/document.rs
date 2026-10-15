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
    /// 表格类文档里这个单元格的位置；散文/代码段为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<(usize, usize)>,
}

impl DocumentSegment {
    fn plain(index: usize, kind: &str, text: String, translatable: bool) -> Self {
        Self {
            index,
            translatable,
            kind: kind.to_string(),
            text,
            cell: None,
        }
    }
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
    /// Table delimiter actually used ( sniffed or forced by the .tsv extension).
    /// Not serialized: it's an implementation detail of the write-back.
    pub delim: Option<u8>,
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
    let mut buf_kind = "prose";
    let mut in_fence = false;
    let mut fence_buf = String::new();

    fn push(segments: &mut Vec<DocumentSegment>, kind: &str, text: String, translatable: bool) {
        if text.trim().is_empty() {
            return;
        }
        segments.push(DocumentSegment::plain(0, kind, text, translatable));
    }

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();

        // Code fence: hold everything verbatim until the closing fence.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if in_fence {
                fence_buf.push_str(line);
                push(&mut segments, "code", std::mem::take(&mut fence_buf), false);
                in_fence = false;
            } else {
                push(&mut segments, buf_kind, std::mem::take(&mut buf), true);
                fence_buf.push_str(line);
                in_fence = true;
            }
            continue;
        }

        if in_fence {
            fence_buf.push_str(line);
            continue;
        }

        // Front-matter / YAML header.
        if trimmed.starts_with("---") && buf.trim().is_empty() && segments.is_empty() {
            push(&mut segments, "frontmatter", line.to_string(), false);
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
            || (trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) && trimmed.contains(". "))
        {
            "list"
        } else {
            "prose"
        };

        // A new kind closes the pending block, so headings/lists stay separate.
        if kind != buf_kind && !buf.trim().is_empty() {
            push(&mut segments, buf_kind, std::mem::take(&mut buf), true);
        }
        buf_kind = kind;
        buf.push_str(line);
    }

    if in_fence && !fence_buf.is_empty() {
        push(&mut segments, "code", fence_buf, false);
    }
    push(&mut segments, buf_kind, buf, true);

    for (i, s) in segments.iter_mut().enumerate() {
        s.index = i;
    }
    segments
}

/// 按文档类型回写：表格走单元格重建，其余按段落拼接。
pub fn write_back(doc: &LoadedDocument, translations: &[(usize, String)]) -> AppResult<String> {
    if doc.kind == "table" {
        reassemble_table(&doc.segments, translations, doc.delim.unwrap_or(b','))
    } else {
        Ok(reassemble(&doc.segments, translations))
    }
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
    let tabular = matches!(ext.as_str(), "csv" | "tsv");
    if !matches!(ext.as_str(), "txt" | "md" | "markdown") && !tabular {
        return Err(AppError::Io(format!(
            "暂只支持 .txt / .md / .csv / .tsv，收到 .{ext}"
        )));
    }
    let bytes = fs::read(p).map_err(|e| AppError::Io(format!("读取失败: {e}")))?;
    let (text, encoding) = decode(&bytes);

    if tabular {
        return Ok(load_tabular(path, &text, &encoding, ext == "tsv"));
    }

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
            .map(|(i, c)| DocumentSegment::plain(i, "prose", c.to_string(), true))
            .collect()
    };
    Ok(LoadedDocument {
        path: path.to_string(),
        encoding,
        kind: kind.to_string(),
        lines: text.lines().count(),
        raw: text.clone(),
        segments,
        delim: None,
    })
}

/// 表格类文档：把每个有内容的单元格当成一个可译段落，行列坐标记在 `cell` 上，
/// 回写时按原样铺回网格。引号转义、内嵌换行交给 csv crate，手写容易出错。
fn load_tabular(path: &str, text: &str, encoding: &str, force_tab: bool) -> LoadedDocument {
    // .tsv 一律用制表符；.csv 的分隔符靠嗅探——欧洲locale 导出常是分号。
    let delim = if force_tab {
        b'\t'
    } else {
        sniff_delimiter(text)
    };

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        // 表头也是数据：csv crate 默认把第一行当表头吃掉，那样这一行就永远不会被翻译
        .has_headers(false)
        // 真实表格常有列数不齐的行，卡死在这里反而什么都读不到
        .flexible(true)
        .from_reader(text.as_bytes());

    let mut grid: Vec<Vec<String>> = Vec::new();
    for rec in rdr.records() {
        match rec {
            Ok(r) => grid.push(r.iter().map(|s| s.to_string()).collect()),
            Err(_) => break,
        }
    }

    let mut segments: Vec<DocumentSegment> = Vec::new();
    for (row, cells) in grid.iter().enumerate() {
        for (col, val) in cells.iter().enumerate() {
            // 每个格子都留下，纯数字/日期这类标 translatable=false：
            // 回写时网格才能完整，不会把整列数字悄悄抹成空。
            let t = val.trim();
            segments.push(DocumentSegment {
                index: 0,
                translatable: !t.is_empty() && !is_verbatim(val),
                kind: "cell".into(),
                text: val.clone(),
                cell: Some((row, col)),
            });
        }
    }
    for (i, s) in segments.iter_mut().enumerate() {
        s.index = i;
    }

    LoadedDocument {
        path: path.to_string(),
        encoding: encoding.to_string(),
        kind: "table".into(),
        lines: grid.len(),
        raw: text.to_string(),
        segments,
        delim: Some(delim),
    }
}

/// 单元格是否属于「翻了也没用、只会添乱」的内容。
fn is_verbatim(val: &str) -> bool {
    let t = val.trim();
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_digit() || "+-.,: %°/年月日时分秒¥$€#()*-_".contains(c))
}

/// 从表头行猜分隔符：数一遍各候选字符的个数，最多的赢。
fn sniff_delimiter(text: &str) -> u8 {
    let header = text.lines().next().unwrap_or("");
    let mut best = (b',', 0usize);
    for d in [b',', b'\t', b';', b'|'] {
        let n = header.bytes().filter(|b| *b == d).count();
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

/// 表格回写：把译好的单元格铺回原网格，再按**原分隔符**写出去。
/// 行列数必须跟输入一致，否则下游按列读的脚本会错位。
pub fn reassemble_table(
    segments: &[DocumentSegment],
    translations: &[(usize, String)],
    delim: u8,
) -> AppResult<String> {
    let mut cells: Vec<Option<String>> = vec![None; segments.len()];
    for (i, t) in translations {
        if let Some(slot) = cells.get_mut(*i) {
            *slot = Some(t.clone());
        }
    }
    // 稀疏网格：没出现在 segments 里的格子（数字、空值）保持原样。
    let mut grid: std::collections::BTreeMap<(usize, usize), String> = Default::default();
    let mut max_row = 0usize;
    let mut max_col = 0usize;
    for (i, s) in segments.iter().enumerate() {
        let Some((r, c)) = s.cell else { continue };
        grid.insert(
            (r, c),
            cells[i].clone().unwrap_or_else(|| s.text.clone()),
        );
        max_row = max_row.max(r);
        max_col = max_col.max(c);
    }

    let mut wtr = csv::WriterBuilder::new()
        .delimiter(delim)
        .from_writer(Vec::new());
    for r in 0..=max_row {
        let row: Vec<String> = (0..=max_col)
            .map(|c| grid.get(&(r, c)).cloned().unwrap_or_default())
            .collect();
        wtr.write_record(&row)
            .map_err(|e| AppError::Io(format!("写 CSV 失败: {e}")))?;
    }
    let bytes = wtr
        .into_inner()
        .map_err(|e| AppError::Io(format!("写 CSV 失败: {e}")))?;
    String::from_utf8(bytes).map_err(|e| AppError::Io(format!("CSV 编码失败: {e}")))
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

        let (cow, _, _) = encoding_rs::GB18030.encode("中文编码测试");
        let gbk = cow.into_owned();
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
        // Non-ASCII language names become a safe ASCII tag.
        assert!(output_path("C:/a/b/readme.md", "英语").ends_with("readme.zh-CN.md"));
        // ASCII tags pass through unchanged.
        assert_eq!(output_path("notes.txt", "zh-CN"), "notes.zh-CN.txt");
        assert!(output_path("C:/a/b/readme.md", "en").ends_with("readme.en.md"));
    }

    /// 表格往返：单元格被译成译文，行列数与分隔符都不变。
    #[test]
    fn csv_translates_cells_and_keeps_shape() {
        let src = "源词,译词,数量\n苹果,apple,3\n香蕉,banana,12\n";
        let doc = load_tabular("x.csv", src, "UTF-8", false);
        assert_eq!(doc.kind, "table");
        assert_eq!(doc.delim, Some(b','));

        // 每个格子都进 segments（含纯数字那列），否则回写时整列会消失
        assert_eq!(doc.segments.len(), 9, "segments: {:?}", doc.segments);
        assert!(doc.segments.iter().all(|s| s.kind == "cell"));
        assert!(doc.segments.iter().all(|s| s.cell.is_some()));
        // 只有纯文本格子进翻译队列
        let queued: Vec<&DocumentSegment> =
            doc.segments.iter().filter(|s| s.translatable).collect();
        assert_eq!(queued.len(), 7, "queued: {:?}", queued);

        let trs: Vec<(usize, String)> = queued
            .iter()
            .map(|s| (s.index, format!("T{}", s.index)))
            .collect();
        let out = write_back(&doc, &trs).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "T0,T1,T2");
        assert_eq!(lines[1], "T3,T4,3");
        assert_eq!(lines[2], "T6,T7,12");
        assert_eq!(lines.len(), 3, "总行数应与输入一致");
    }

    /// 引号内的逗号不该被当成分隔符，含逗号与换行的格子也要能原样回来。
    #[test]
    fn csv_handles_quotes_and_embedded_newlines() {
        let src = "a,b\n\"x, y\",\"line1\nline2\"\n";
        let doc = load_tabular("x.csv", src, "UTF-8", false);
        assert_eq!(doc.segments.len(), 4);
        assert_eq!(doc.segments[2].text, "x, y");
        assert_eq!(doc.segments[3].text, "line1\nline2");

        let out = write_back(&doc, &[(0, "A".into()), (1, "B".into()), (2, "p, q".into()), (3, "l1\nl2".into())])
            .unwrap();
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(out.as_bytes());
        let rec = rdr.records().nth(1).unwrap().unwrap();
        assert_eq!(&rec[0], "p, q");
        assert_eq!(&rec[1], "l1\nl2");
    }

    /// 欧洲 locale 导出的 CSV 用分号；.tsv 一律按制表符读。
    #[test]
    fn csv_sniffs_delimiter() {
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n"), b';');
        assert_eq!(sniff_delimiter("a\tb\tc\n"), b'\t');
        assert_eq!(sniff_delimiter("a,b,c\n"), b',');
        let doc = load_tabular("x.tsv", "a\tb\n中文\t英文\n", "UTF-8", true);
        assert_eq!(doc.delim, Some(b'\t'));
        assert_eq!(doc.segments.len(), 4);
    }

    /// 纯数字/日期这类格子标成不译——翻了也不会变，还占 token。
    #[test]
    fn csv_marks_numeric_cells_verbatim() {
        let doc = load_tabular("x.csv", "a,2024-01-02,1,234,¥50\n", "UTF-8", false);
        assert_eq!(doc.segments.len(), 5, "格子一个都不能少，否则回写丢列");
        assert_eq!(
            doc.segments.iter().filter(|s| s.translatable).count(),
            1,
            "只有 'a' 该被翻译"
        );
        assert!(is_verbatim("2024-01-02") && is_verbatim("¥50") && is_verbatim("12.5%"));
        assert!(!is_verbatim("苹果"));
    }

    /// 空格子也要留在网格里，否则整行会被截短。
    #[test]
    fn csv_keeps_empty_cells_as_columns() {
        let doc = load_tabular("x.csv", "a,,c\n,,\n", "UTF-8", false);
        assert_eq!(doc.segments.len(), 6);
        let out = write_back(&doc, &[(0, "A".into())]).unwrap();
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(out.as_bytes());
        let rec = rdr.records().next().unwrap().unwrap();
        assert_eq!(rec.len(), 3, "空列不能被吃掉");
        assert_eq!(&rec[0], "A");
        assert_eq!(&rec[1], "");
        assert_eq!(&rec[2], "c");
    }
}