//! History, actions and undo.
//!
//! Every change to the catalog happens inside an [`Action`]. Each insert,
//! field change and delete writes one history row sharing the action's ID,
//! so undo can reverse a whole action even when it touched several records.
//! Undo is itself recorded as a new action, so history never loses anything.

use std::collections::HashMap;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{params, params_from_iter, Connection, Transaction};
use serde_json::{Map, Value};

use crate::journey::{now, Journey};
use crate::{ids, Error, Result};

/// Tables an action may write. `history` is written only by this module.
const WRITABLE: &[&str] = &[
    "item", "journey", "featured", "individual", "person", "pet", "individual_relationship", "family",
    "family_member", "family_relationship", "place", "event", "event_individual", "story", "story_memory",
    "memory", "memory_content", "memory_individual", "content", "picture", "voice_recording", "video",
    "text_content", "content_file", "content_relationship", "alias",
];

pub type Row = Map<String, Value>;

/// Column names and primary-key columns of a table, read from the schema.
#[derive(Clone)]
pub(crate) struct TableInfo {
    pub columns: Vec<String>,
    pub key: Vec<String>,
}

pub(crate) fn table_info(conn: &Connection, table: &str) -> Result<TableInfo> {
    if !WRITABLE.contains(&table) && table != "history" {
        return Err(Error::Invalid(format!("Unknown table {table}.")));
    }
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut cols: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let columns = cols.iter().map(|c| c.0.clone()).collect();
    cols.retain(|c| c.1 > 0);
    cols.sort_by_key(|c| c.1);
    Ok(TableInfo { columns, key: cols.into_iter().map(|c| c.0).collect() })
}

pub(crate) fn to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => SqlValue::Real(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

pub(crate) fn from_sql(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => Value::from(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::String(format!("{} bytes", b.len())),
    }
}

/// One user action in progress. Commit happens when the closure given to
/// [`Journey::act`] returns `Ok`; any error rolls everything back.
pub struct Action<'a> {
    tx: Transaction<'a>,
    action_id: String,
    author: String,
    at: String,
    tables: HashMap<String, TableInfo>,
    changes: usize,
}

impl<'a> Action<'a> {
    fn info(&mut self, table: &str) -> Result<TableInfo> {
        if !WRITABLE.contains(&table) {
            return Err(Error::Invalid(format!("Unknown table {table}.")));
        }
        if !self.tables.contains_key(table) {
            let info = table_info(&self.tx, table)?;
            self.tables.insert(table.to_string(), info);
        }
        Ok(self.tables[table].clone())
    }

    pub fn conn(&self) -> &Connection {
        &self.tx
    }

    pub fn action_id(&self) -> &str {
        &self.action_id
    }

    fn record(&mut self, table: &str, item_id: &str, field: Option<&str>, old: Option<&Value>, new: Option<&Value>) -> Result<()> {
        let text = |v: Option<&Value>| v.map(|v| v.to_string());
        self.tx.execute(
            "INSERT INTO history (at, author, action_id, table_name, item_id, field, old_value, new_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![self.at, self.author, self.action_id, table, item_id, field, text(old), text(new)],
        )?;
        self.changes += 1;
        Ok(())
    }

    fn key_of(info: &TableInfo, row: &Row) -> Result<Row> {
        let mut key = Row::new();
        for k in &info.key {
            let v = row.get(k).cloned().unwrap_or(Value::Null);
            if v.is_null() {
                return Err(Error::Invalid(format!("Missing key field {k}.")));
            }
            key.insert(k.clone(), v);
        }
        Ok(key)
    }

    /// The `item_id` written to history: the ID itself for item tables, or
    /// the key as JSON for link tables.
    fn item_label(key: &Row) -> String {
        match (key.len(), key.get("id")) {
            (1, Some(Value::String(id))) => id.clone(),
            _ => Value::Object(key.clone()).to_string(),
        }
    }

    /// Create a new item with a fresh permanent ID in `table`. Returns the ID.
    pub fn create(&mut self, table: &str, mut row: Row) -> Result<String> {
        let id = loop {
            let id = ids::new_id();
            let taken: bool =
                self.tx.query_row("SELECT EXISTS(SELECT 1 FROM item WHERE id = ?1)", [&id], |r| r.get(0))?;
            if !taken {
                break id;
            }
        };
        let mut reg = Row::new();
        reg.insert("id".into(), Value::from(id.clone()));
        reg.insert("table_name".into(), Value::from(table));
        self.insert("item", reg)?;
        row.insert("id".into(), Value::from(id.clone()));
        self.insert(table, row)?;
        Ok(id)
    }

    /// Create an item under a given ID, as when an exported Journey is
    /// brought back in with its own IDs (sprint 11). Fails when it is taken.
    pub fn create_with_id(&mut self, table: &str, id: &str, mut row: Row) -> Result<()> {
        let mut reg = Row::new();
        reg.insert("id".into(), Value::from(id));
        reg.insert("table_name".into(), Value::from(table));
        self.insert("item", reg)?;
        row.insert("id".into(), Value::from(id));
        self.insert(table, row)
    }

    /// Create the extended row of an item (person, pet, picture, ...) under an existing ID.
    pub fn insert(&mut self, table: &str, mut row: Row) -> Result<()> {
        let info = self.info(table)?;
        if info.columns.iter().any(|c| c == "updated") {
            row.insert("updated".into(), Value::from(self.at.clone()));
        }
        for k in row.keys() {
            if !info.columns.contains(k) {
                return Err(Error::Invalid(format!("{table} has no field {k}.")));
            }
        }
        let key = Self::key_of(&info, &row)?;
        let cols: Vec<&String> = row.keys().collect();
        let sql = format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            cols.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "),
            (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ")
        );
        self.tx.execute(&sql, params_from_iter(row.values().map(to_sql)))?;
        let stored = self.read_row(table, &info, &key)?.unwrap_or(row);
        self.record(table, &Self::item_label(&key), None, None, Some(&Value::Object(stored)))
    }

    /// Change one field of one record.
    pub fn update(&mut self, table: &str, key: Row, field: &str, value: Value) -> Result<()> {
        let info = self.info(table)?;
        if !info.columns.iter().any(|c| c == field) || field == "updated" || info.key.iter().any(|k| k == field) {
            return Err(Error::Invalid(format!("{table}.{field} cannot be changed.")));
        }
        let key = Self::key_of(&info, &key)?;
        let old = self
            .read_row(table, &info, &key)?
            .ok_or_else(|| Error::Invalid(format!("No such record in {table}.")))?
            .remove(field)
            .unwrap_or(Value::Null);
        if old == value {
            return Ok(());
        }
        let mut args = vec![to_sql(&value)];
        let mut set = format!("{field} = ?1");
        if info.columns.iter().any(|c| c == "updated") {
            args.push(SqlValue::Text(self.at.clone()));
            set.push_str(", updated = ?2");
        }
        let (where_sql, key_args) = where_clause(&key, args.len() + 1);
        args.extend(key_args);
        self.tx.execute(&format!("UPDATE {table} SET {set} WHERE {where_sql}"), params_from_iter(args))?;
        self.record(table, &Self::item_label(&key), Some(field), Some(&old), Some(&value))
    }

    /// Delete one record. Deleting an item also needs its `item` registry row
    /// deleted afterwards, which [`Action::delete_item`] does.
    pub fn delete(&mut self, table: &str, key: Row) -> Result<()> {
        let info = self.info(table)?;
        let key = Self::key_of(&info, &key)?;
        let old = self
            .read_row(table, &info, &key)?
            .ok_or_else(|| Error::Invalid(format!("No such record in {table}.")))?;
        let (where_sql, args) = where_clause(&key, 1);
        self.tx.execute(&format!("DELETE FROM {table} WHERE {where_sql}"), params_from_iter(args))?;
        self.record(table, &Self::item_label(&key), None, Some(&Value::Object(old)), None)
    }

    /// Delete an item row (and its extended row, if any) and its ID registration.
    pub fn delete_item(&mut self, table: &str, id: &str) -> Result<()> {
        let key = id_key(id);
        if table == "content" {
            // A picture chosen as someone's portrait stops being it (sprint 8).
            let mut st = self.tx.prepare("SELECT id FROM individual WHERE image_id = ?1")?;
            let who: Vec<String> = st.query_map([id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
            drop(st);
            for w in who {
                self.update("individual", id_key(&w), "image_id", Value::Null)?;
            }
        }
        for ext in extended_tables(table) {
            let exists: bool = self.tx.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {ext} WHERE id = ?1)"),
                [id],
                |r| r.get(0),
            )?;
            if exists {
                self.delete(ext, key.clone())?;
            }
        }
        self.delete(table, key.clone())?;
        self.delete("item", key)
    }

    fn read_row(&self, table: &str, info: &TableInfo, key: &Row) -> Result<Option<Row>> {
        let (where_sql, args) = where_clause(key, 1);
        let sql = format!("SELECT {} FROM {table} WHERE {where_sql}", info.columns.join(", "));
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(args))?;
        Ok(match rows.next()? {
            Some(r) => {
                let mut m = Row::new();
                for (i, c) in info.columns.iter().enumerate() {
                    m.insert(c.clone(), from_sql(r.get_ref(i)?));
                }
                Some(m)
            }
            None => None,
        })
    }
}

pub fn id_key(id: &str) -> Row {
    let mut k = Row::new();
    k.insert("id".into(), Value::from(id));
    k
}

fn extended_tables(table: &str) -> &'static [&'static str] {
    match table {
        "individual" => &["person", "pet"],
        "content" => &["picture", "voice_recording", "video", "text_content"],
        _ => &[],
    }
}

fn where_clause(key: &Row, first: usize) -> (String, Vec<SqlValue>) {
    let sql = key.keys().enumerate().map(|(i, k)| format!("{k} = ?{}", first + i)).collect::<Vec<_>>().join(" AND ");
    (sql, key.values().map(to_sql).collect())
}

/// One history row, as read back.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub seq: i64,
    pub at: String,
    pub author: String,
    pub action_id: String,
    pub table_name: String,
    pub item_id: String,
    pub field: Option<String>,
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
}

impl Journey {
    /// Run one user action. All its changes commit together, are written to
    /// history under one action ID, and the plain-text copy is rewritten.
    pub fn act<T>(&mut self, f: impl FnOnce(&mut Action) -> Result<T>) -> Result<T> {
        if self.mode() == crate::OpenMode::ReadOnly {
            return Err(Error::Invalid("This Journey is open read-only.".into()));
        }
        self.before_change()?;
        let author = self.author().to_string();
        let action_id = ids::new_id();
        let (out, changed) = {
            let tx = self.conn_mut().transaction()?;
            let mut action = Action { tx, action_id: action_id.clone(), author, at: now(), tables: HashMap::new(), changes: 0 };
            let out = f(&mut action)?;
            let changed = action.changes > 0;
            action.tx.commit()?;
            (out, changed)
        };
        if changed {
            self.undo_stack.push(action_id.clone());
            self.sync_files(&action_id)?;
            self.after_change()?;
        }
        Ok(out)
    }

    /// Reverse the most recent action of this session. Returns false when
    /// there is nothing left to undo.
    pub fn undo(&mut self) -> Result<bool> {
        let Some(target) = self.undo_stack.pop() else { return Ok(false) };
        let entries = self.history_of(&target)?;
        self.before_change()?;
        let author = self.author().to_string();
        let action_id = ids::new_id();
        {
            let tx = self.conn_mut().transaction()?;
            let mut action = Action { tx, action_id: action_id.clone(), author, at: now(), tables: HashMap::new(), changes: 0 };
            for e in entries.iter().rev() {
                match (&e.field, &e.old_value, &e.new_value) {
                    (Some(field), old, _) => {
                        let info = action.info(&e.table_name)?;
                        let key = key_from_label(&info, &e.item_id);
                        action.update(&e.table_name, key, field, old.clone().unwrap_or(Value::Null))?;
                    }
                    (None, None, Some(Value::Object(row))) => {
                        let info = action.info(&e.table_name)?;
                        let key = Action::key_of(&info, row)?;
                        action.delete(&e.table_name, key)?;
                    }
                    (None, Some(Value::Object(row)), None) => {
                        let mut row = row.clone();
                        row.remove("updated");
                        action.insert(&e.table_name, row)?;
                    }
                    _ => return Err(Error::Invalid(format!("History entry {} cannot be undone.", e.seq))),
                }
            }
            action.tx.commit()?;
        }
        self.sync_files(&action_id)?;
        self.after_change()?;
        Ok(true)
    }

    /// How many actions of this session can still be undone.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn history_of(&self, action_id: &str) -> Result<Vec<HistoryEntry>> {
        self.history_where("action_id = ?1", [action_id])
    }

    pub fn history_all(&self) -> Result<Vec<HistoryEntry>> {
        self.history_where("1 = 1", [])
    }

    fn history_where<P: rusqlite::Params>(&self, cond: &str, p: P) -> Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT seq, at, author, action_id, table_name, item_id, field, old_value, new_value
             FROM history WHERE {cond} ORDER BY seq"
        ))?;
        let parse = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
        let rows = stmt.query_map(p, |r| {
            Ok(HistoryEntry {
                seq: r.get(0)?,
                at: r.get(1)?,
                author: r.get(2)?,
                action_id: r.get(3)?,
                table_name: r.get(4)?,
                item_id: r.get(5)?,
                field: r.get(6)?,
                old_value: parse(r.get(7)?),
                new_value: parse(r.get(8)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

fn key_from_label(info: &TableInfo, label: &str) -> Row {
    match serde_json::from_str::<Value>(label) {
        Ok(Value::Object(m)) if info.key.len() > 1 || !info.key.contains(&"id".to_string()) => m,
        _ => id_key(label),
    }
}
