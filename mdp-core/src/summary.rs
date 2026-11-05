//! What the app shell shows: the home summary and simple lists (design
//! document 1.6). Read-only queries over the catalog.

use serde::Serialize;
use serde_json::Value;

use crate::{Journey, Result};

#[derive(Debug, Serialize, Default, PartialEq)]
pub struct Counts {
    pub stories: i64,
    pub memories: i64,
    pub people: i64,
    pub pets: i64,
    pub families: i64,
    pub places: i64,
    pub waiting: i64,
    /// Pictures anywhere in the Journey, and memories placed in a story:
    /// two of the getting-started steps (sprint 14).
    pub pictures: i64,
    pub in_stories: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// A short second line, such as a date or a story's name.
    pub note: String,
}

/// One raw picture, recording, video or text on the Waiting screen.
#[derive(Debug, Clone, Serialize)]
pub struct RawItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub date: Option<String>,
    /// The nearest exact date, for sorting by date (Waiting screen).
    pub date_sort: Option<String>,
    /// A title suggested from the file name, if it gives one.
    pub title: Option<String>,
    /// With no date of its own, the date its file name suggests.
    pub name_date: Option<String>,
    /// The last part of the folder it was brought in from ("" if not known),
    /// and the day it was brought in: the By box view boxes items per import.
    pub folder: String,
    pub added: String,
}

#[derive(Debug, Serialize)]
pub struct Home {
    pub id: String,
    pub name: String,
    pub introduction: String,
    pub folder: String,
    pub read_only: bool,
    pub counts: Counts,
    /// Placeholder memories waiting for a picture, recording or note.
    pub to_finish: Vec<Entry>,
    /// How many placeholders there are in all (`to_finish` shows the first 8).
    pub to_finish_total: i64,
    /// Memories with content that could still use more; the
    /// note lists what, such as "a date, a place". The first 8, by name.
    pub could_add: Vec<Entry>,
    pub could_add_total: i64,
    pub featured: Vec<Entry>,
    /// The stories the exported bookshelf shows, featured first, for the
    /// home's preview card (up to six); each note is its cover picture's id or empty.
    pub shelf: Vec<Entry>,
}

#[derive(Debug, Serialize)]
pub struct FamilyGroup {
    /// Empty for the people in no family.
    pub id: String,
    pub name: String,
    pub members: Vec<Entry>,
}

fn count(j: &Journey, sql: &str) -> Result<i64> {
    Ok(j.conn().query_row(sql, [], |r| r.get(0))?)
}

fn entries(j: &Journey, sql: &str, arg: Option<&str>) -> Result<Vec<Entry>> {
    let mut stmt = j.conn().prepare(sql)?;
    let map = |r: &rusqlite::Row| -> rusqlite::Result<Entry> {
        Ok(Entry { id: r.get(0)?, name: r.get(1)?, note: r.get::<_, Option<String>>(2)?.unwrap_or_default() })
    };
    let rows = match arg {
        Some(a) => stmt.query_map([a], map)?.collect::<std::result::Result<Vec<_>, _>>()?,
        None => stmt.query_map([], map)?.collect::<std::result::Result<Vec<_>, _>>()?,
    };
    Ok(rows)
}

impl Journey {
    pub fn home(&self) -> Result<Home> {
        let (name, intro): (String, Option<String>) =
            self.conn().query_row("SELECT name, introduction FROM journey WHERE id = ?1", [self.id()], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut could = vec![];
        for m in entries(self, "SELECT id, name, '' FROM memory WHERE status <> 'needs content' AND finished IS NULL ORDER BY name COLLATE NOCASE", None)? {
            let missing = crate::memories::could_add(self.conn(), &m.id)?;
            if !missing.is_empty() {
                could.push(Entry { note: missing.join(", "), ..m });
            }
        }
        Ok(Home {
            id: self.id().to_string(),
            name,
            introduction: intro.unwrap_or_default(),
            folder: self.root().display().to_string(),
            read_only: self.mode() == crate::OpenMode::ReadOnly,
            counts: Counts {
                stories: count(self, "SELECT count(*) FROM story")?,
                memories: count(self, "SELECT count(*) FROM memory")?,
                people: count(self, "SELECT count(*) FROM individual WHERE kind = 'Person'")?,
                pets: count(self, "SELECT count(*) FROM individual WHERE kind = 'Pet'")?,
                families: count(self, "SELECT count(*) FROM family")?,
                places: count(self, "SELECT count(*) FROM place")?,
                waiting: count(self, &format!("SELECT count(*) FROM content WHERE state = 'waiting' AND {}", crate::voice::NOT_ATTACHED))?,
                pictures: count(self, "SELECT count(*) FROM content WHERE kind = 'picture'")?,
                in_stories: count(self, "SELECT count(DISTINCT memory_id) FROM story_memory")?,
            },
            to_finish: entries(
                self,
                "SELECT m.id, m.name, coalesce((SELECT s.name FROM story_memory sm JOIN story s ON s.id = sm.story_id
                   WHERE sm.memory_id = m.id ORDER BY s.name LIMIT 1), '') || CASE WHEN m.date_edtf IS NULL THEN '' ELSE ' · ' || m.date_edtf END
                 FROM memory m WHERE m.status = 'needs content' ORDER BY m.name LIMIT 8",
                None,
            )?
            .into_iter()
            .map(|mut e| {
                e.note = e.note.trim_start_matches(" · ").to_string();
                e
            })
            .collect(),
            to_finish_total: count(self, "SELECT count(*) FROM memory WHERE status = 'needs content'")?,
            could_add_total: could.len() as i64,
            could_add: could.into_iter().take(8).collect(),
            shelf: entries(
                self,
                "SELECT s.id, s.name, coalesce((SELECT coalesce(
                    (SELECT m.cover_id FROM memory m JOIN content c ON c.id = m.cover_id WHERE m.id = sm.memory_id AND c.kind = 'picture'),
                    (SELECT mc.content_id FROM memory_content mc JOIN content c ON c.id = mc.content_id
                     WHERE mc.memory_id = sm.memory_id AND c.kind = 'picture' ORDER BY mc.position LIMIT 1))
                   FROM story_memory sm WHERE sm.story_id = s.id
                     AND EXISTS (SELECT 1 FROM memory_content mc JOIN content c ON c.id = mc.content_id WHERE mc.memory_id = sm.memory_id AND c.kind = 'picture')
                   ORDER BY sm.position LIMIT 1), '')
                 FROM story s LEFT JOIN featured f ON f.item_id = s.id
                 WHERE EXISTS (SELECT 1 FROM story_memory sm JOIN memory m ON m.id = sm.memory_id WHERE sm.story_id = s.id AND m.status <> 'needs content')
                 ORDER BY f.position IS NULL, f.position, s.name COLLATE NOCASE LIMIT 6",
                None,
            )?,
            featured: entries(
                self,
                "SELECT f.item_id, coalesce(s.name, m.name), CASE WHEN s.id IS NULL THEN 'memory' ELSE 'story' END
                 FROM featured f LEFT JOIN story s ON s.id = f.item_id LEFT JOIN memory m ON m.id = f.item_id ORDER BY f.position",
                None,
            )?,
        })
    }

    pub fn stories(&self) -> Result<Vec<Entry>> {
        entries(
            self,
            "SELECT s.id, s.name, (SELECT count(*) FROM story_memory WHERE story_id = s.id) || ' memories' FROM story s ORDER BY s.name",
            None,
        )
    }

    /// People and pets grouped by family; those in no family come last.
    pub fn people_by_family(&self) -> Result<Vec<FamilyGroup>> {
        let mut out = vec![];
        for fam in entries(self, "SELECT id, name, '' FROM family ORDER BY name", None)? {
            let members = entries(
                self,
                "SELECT i.id, i.name, CASE WHEN i.kind = 'Pet' THEN coalesce(p.species, 'pet') ELSE trim(coalesce(substr(i.birth_date_edtf, 1, 4), '') ||
                   CASE WHEN i.death_date_edtf IS NULL THEN '' ELSE '–' || substr(i.death_date_edtf, 1, 4) END) END
                 FROM family_member fm JOIN individual i ON i.id = fm.individual_id LEFT JOIN pet p ON p.id = i.id
                 WHERE fm.family_id = ?1 ORDER BY i.kind DESC, i.name",
                Some(&fam.id),
            )?;
            out.push(FamilyGroup { id: fam.id, name: fam.name, members });
        }
        let loose = entries(
            self,
            "SELECT i.id, i.name, '' FROM individual i WHERE NOT EXISTS (SELECT 1 FROM family_member fm WHERE fm.individual_id = i.id) ORDER BY i.name",
            None,
        )?;
        if !loose.is_empty() {
            out.push(FamilyGroup { id: String::new(), name: "Not in a family yet".into(), members: loose });
        }
        Ok(out)
    }

    pub fn waiting(&self) -> Result<Vec<Entry>> {
        entries(
            self,
            &format!("SELECT id, name, kind FROM content WHERE state = 'waiting' AND {} ORDER BY date_added DESC, name LIMIT 500", crate::voice::NOT_ATTACHED),
            None,
        )
    }

    /// Raw content in one state ("waiting" or "set aside") for the Waiting screen, newest first.
    pub fn raw_content(&self, state: &str) -> Result<Vec<RawItem>> {
        let mut st = self.conn().prepare(&format!(
            "SELECT id, name, kind, date, coalesce(source_note, ''), date_added FROM content WHERE state = ?1 AND {} ORDER BY date_added DESC, date DESC, name LIMIT 2000",
            crate::voice::NOT_ATTACHED
        ))?;
        let rows = st.query_map([state], |r| {
            let date: Option<String> = r.get(3)?;
            let date_sort = date.as_deref().and_then(crate::dates::nearest);
            let name: String = r.get(1)?;
            let name_date = if date.is_none() { crate::sorting::file_name_date(&name) } else { None };
            let date_sort = date_sort.or_else(|| name_date.as_deref().and_then(|d| crate::dates::parse(d).ok().flatten()).and_then(|d| d.nearest));
            let folder = crate::sorting::last_folder(&r.get::<_, String>(4)?);
            Ok(RawItem { id: r.get(0)?, title: crate::sorting::file_name_title(&name), name, kind: r.get(2)?, date, date_sort, name_date, folder, added: r.get(5)? })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// One item as it appears in the plain-text copy.
    pub fn item(&self, id: &str) -> Result<Option<Value>> {
        crate::jsonl::line(self.conn(), id)
    }
}
