//! 模板存储: 数据库里的一张表 (name, description, json_data, updated_at) + 一个放 JSON 文件的目录.
//!
//! IC 模板 (`ic_templates` / `ic-templates/`) 和拓扑模板 (`topology_templates` / `templates/`) 各用一个
//! [`TemplateStore`], 逻辑完全相同, 只有表名和目录名不同. 查找顺序都是先数据库后目录.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TemplateStore {
    /// 数据库表名
    table: &'static str,
    /// 放 JSON 模板的目录 (相对当前工作目录)
    dir: &'static str,
}

impl TemplateStore {
    pub(crate) const fn new(table: &'static str, dir: &'static str) -> Self {
        Self { table, dir }
    }

    /// 只读打开: 查找不写库; 库文件不存在 (或打不开) 就当库里没有模板, 不会顺手建出一个空库文件.
    fn open_db(&self) -> Option<Connection> {
        Connection::open_with_flags(
            crate::db_path::db_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()
    }

    /// 从数据库取一条模板的 JSON; 表不存在 / 没有这一条都当没有.
    fn load_from_db(&self, conn: &Connection, name: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT json_data FROM {} WHERE name = ?1", self.table),
            rusqlite::params![name],
            |row| row.get(0),
        )
        .ok()
    }

    fn list_from_db(&self, conn: &Connection) -> Vec<String> {
        let Ok(mut stmt) = conn.prepare(&format!("SELECT name FROM {} ORDER BY name", self.table))
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |row| row.get(0)) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    /// 模板的 JSON 文本: 先查数据库, 再读目录下的 `<name>.json`. 两处都没有返回 `None`;
    /// 目录里的文件存在但读不出来时返回错误.
    pub(crate) fn load_json(&self, name: &str) -> Result<Option<String>> {
        if let Some(db) = self.open_db() {
            if let Some(json) = self.load_from_db(&db, name) {
                return Ok(Some(json));
            }
        }
        let path = Path::new(self.dir).join(format!("{}.json", name));
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read template: {}", path.display()))
            .map(Some)
    }

    /// 全部模板名 (数据库 ∪ 目录), 已排序去重.
    pub(crate) fn list_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Some(db) = self.open_db() {
            names.extend(self.list_from_db(&db));
        }
        if let Ok(entries) = std::fs::read_dir(self.dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    let name = name.to_string();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }

    /// 把目录下所有 `.json` 模板写进数据库 (同名覆盖), 返回写入的个数.
    pub(crate) fn import_dir(&self, dir: &Path, conn: &Connection) -> Result<usize> {
        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("Failed to read template directory: {}", dir.display()))?;
        let sql = format!(
            "INSERT OR REPLACE INTO {} (name, description, json_data, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
            self.table
        );
        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let json = std::fs::read_to_string(&path)?;
            let desc: Option<String> = serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|v| {
                    v.get("description")
                        .and_then(|d| d.as_str().map(|s| s.to_string()))
                });
            conn.execute(&sql, rusqlite::params![name, desc, json])?;
            count += 1;
        }
        Ok(count)
    }
}
