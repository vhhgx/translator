//! Named glossary sets, persisted as JSON in the app data dir.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TermSet {
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub terms: Vec<[String; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Glossary {
    pub version: u32,
    pub active: String,
    pub sets: std::collections::BTreeMap<String, TermSet>,
}

impl Default for Glossary {
    fn default() -> Self {
        let mut sets = std::collections::BTreeMap::new();
        sets.insert(
            "默认".to_string(),
            TermSet {
                note: "空词库".into(),
                terms: vec![],
            },
        );
        Self {
            version: 1,
            active: "默认".into(),
            sets,
        }
    }
}

fn store_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hy-mt2-translator")
        .join("glossary.json")
}

impl Glossary {
    pub fn load() -> Self {
        let path = store_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) -> AppResult<()> {
        let path = store_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| AppError::Io(e.to_string()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| AppError::Io(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| AppError::Io(e.to_string()))?;
        fs::rename(&tmp, &path).map_err(|e| AppError::Io(e.to_string()))
    }

    pub fn names(&self) -> Vec<String> {
        self.sets.keys().cloned().collect()
    }

    pub fn terms(&self, name: &str) -> Vec<[String; 2]> {
        self.sets
            .get(name)
            .map(|s| s.terms.clone())
            .unwrap_or_default()
    }

    pub fn set_terms(&mut self, name: &str, terms: Vec<[String; 2]>) {
        let cleaned: Vec<[String; 2]> = terms
            .into_iter()
            .map(|mut p| {
                p[0] = p[0].trim().to_string();
                p[1] = p[1].trim().to_string();
                p
            })
            .filter(|p| !p[0].is_empty() && !p[1].is_empty())
            .collect();
        self.sets
            .entry(name.to_string())
            .or_insert_with(|| TermSet::default())
            .terms = cleaned;
    }

    pub fn create_set(&mut self, name: &str) -> AppResult<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::Io("词库名不能为空".into()));
        }
        if self.sets.contains_key(name) {
            return Err(AppError::Io(format!("词库「{name}」已存在")));
        }
        self.sets
            .insert(name.to_string(), TermSet::default());
        self.active = name.to_string();
        Ok(())
    }

    pub fn rename_set(&mut self, old: &str, new: &str) -> AppResult<()> {
        let new = new.trim();
        if self.sets.contains_key(new) {
            return Err(AppError::Io(format!("词库「{new}」已存在")));
        }
        if let Some(v) = self.sets.remove(old) {
            self.sets.insert(new.to_string(), v);
            if self.active == old {
                self.active = new.to_string();
            }
            Ok(())
        } else {
            Err(AppError::Io(format!("词库「{old}」不存在")))
        }
    }

    pub fn delete_set(&mut self, name: &str) -> AppResult<()> {
        if self.sets.len() <= 1 {
            return Err(AppError::Io("至少保留一个词库".into()));
        }
        self.sets.remove(name);
        if self.active == name {
            if let Some(k) = self.sets.keys().next() {
                self.active = k.clone();
            }
        }
        Ok(())
    }

    pub fn set_active(&mut self, name: &str) {
        if self.sets.contains_key(name) {
            self.active = name.to_string();
        }
    }

    /// Merge terms from an exported payload, optionally into a new/other set.
    pub fn import(
        &mut self,
        name: &str,
        terms: Vec<[String; 2]>,
        replace: bool,
    ) -> usize {
        let entry = self.sets.entry(name.to_string()).or_insert_with(TermSet::default);
        if replace {
            entry.terms.clear();
        }
        for t in terms {
            if t[0].trim().is_empty() || t[1].trim().is_empty() {
                continue;
            }
            let mut p = t.clone();
            p[0] = p[0].trim().to_string();
            p[1] = p[1].trim().to_string();
            if !entry.terms.contains(&p) {
                entry.terms.push(p);
            }
        }
        entry.terms.len()
    }
}

/// Payload written by "导出词库".
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportPayload {
    pub name: String,
    pub terms: Vec<[String; 2]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_roundtrip() {
        let mut g = Glossary::default();
        g.create_set("产品").unwrap();
        assert_eq!(g.active, "产品");
        g.set_terms(
            "产品",
            vec![
                ["订单".to_string(), "ticket".to_string()],
                ["".to_string(), "x".to_string()],
                ["仓库".to_string(), "repo".to_string()],
            ],
        );
        assert_eq!(g.terms("产品").len(), 2); // blank row dropped
        g.rename_set("产品", "产品文档").unwrap();
        assert_eq!(g.active, "产品文档");
        assert!(g.names().contains(&"产品文档".to_string()));
        g.delete_set("产品文档").unwrap();
        assert_eq!(g.names(), vec!["默认".to_string()]);
        assert!(g.delete_set("默认").is_err(), "must keep one set");
    }

    #[test]
    fn duplicate_sets_rejected() {
        let mut g = Glossary::default();
        assert!(g.create_set("默认").is_err());
    }

    #[test]
    fn import_dedupes_and_appends() {
        let mut g = Glossary::default();
        g.import(
            "默认",
            vec![
                ["订单".to_string(), "ticket".to_string()],
                ["仓库".to_string(), "repo".to_string()],
            ],
            false,
        );
        g.import("默认", vec![["订单".to_string(), "ticket".to_string()]], false);
        assert_eq!(g.terms("默认").len(), 2);
        g.import("默认", vec![["发票".to_string(), "invoice".to_string()]], true);
        assert_eq!(
            g.terms("默认"),
            vec![["发票".to_string(), "invoice".to_string()]]
        );
    }
}