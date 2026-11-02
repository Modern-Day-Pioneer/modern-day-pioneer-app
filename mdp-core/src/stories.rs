//! Stories and the Journey.
//!
//! A story has a title, one focus (a person, pet or family, or no one), an
//! introduction and ordered memories. A memory can sit in more than one
//! story. The Journey has a title, an introduction and up to six featured
//! stories or memories, in order. Removing a story leaves its memories.
//!
//! An "In memory of" page (sprint 13) is a story of the kind
//! "tribute": about one person or pet, with chosen memories in order and a
//! "why it matters" note on each. It is a celebration as much as a memorial,
//! so it works for the living too.

use std::collections::HashMap;

use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::{json, Value};

use crate::dates;
use crate::memories::{move_in_list, plural, renumber, strings};
use crate::people::{name_of, row, text_value};
use crate::summary::Entry;
use crate::{id_key, Action, Error, Journey, Result, Row};

/// Featured items on the home page.
pub const MAX_FEATURED: usize = 6;

#[derive(Debug, Clone, Serialize)]
pub struct StoryMemory {
    pub id: String,
    pub name: String,
    pub date: String,
    pub status: String,
    /// The memory's cover kind, for a small marker ("picture", "voice recording" ...).
    pub cover_kind: Option<String>,
    /// The picture or video shown small beside the memory on the story screen.
    pub cover_id: Option<String>,
    /// Why this memory matters, on an "In memory of" page.
    pub note: Option<String>,
}

/// One memory in the sortable, filterable list of all memories (sprint 8).
#[derive(Debug, Clone, Serialize)]
pub struct MemoryRow {
    pub id: String,
    pub name: String,
    /// The memory's date in words, or empty.
    pub date: String,
    /// The nearest full date, for sorting by memory date.
    pub date_sort: Option<String>,
    /// When the memory was made, from the history table (no format change).
    pub added: Option<String>,
    pub status: String,
    pub cover_kind: Option<String>,
    pub cover_id: Option<String>,
    /// IDs of the people and pets linked to it, and of the stories it is in.
    pub people: Vec<String>,
    pub stories: Vec<String>,
    /// A preview for lists (sprint 13): the caption, else the first words of
    /// its text or a transcript; and the names of the people in it.
    pub words: String,
    pub who: String,
    /// The memory's place and event, so adding it elsewhere can warn.
    pub place_id: Option<String>,
    pub event_id: Option<String>,
}

/// The first words of a text, for a preview: at most `n` characters, cut at a word.
pub(crate) fn first_words(t: &str, n: usize) -> String {
    let flat: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= n {
        return flat;
    }
    let cut: String = flat.chars().take(n).collect();
    let cut = cut.rsplit_once(' ').map(|(a, _)| a.to_string()).unwrap_or(cut);
    format!("{}…", cut.trim_end_matches(|c: char| c.is_ascii_punctuation()))
}

#[derive(Debug, Clone, Serialize)]
pub struct StoryView {
    pub id: String,
    pub name: String,
    pub introduction: Option<String>,
    /// The person, pet or family the story is about; `note` holds its kind.
    pub focus: Option<Entry>,
    pub memories: Vec<StoryMemory>,
    pub featured: bool,
    /// The story's cover: the cover picture of its first memory that has one.
    pub cover_id: Option<String>,
    /// "story", or "tribute" for an "In memory of" page.
    pub genre: String,
}

fn need_story(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    if crate::memories::exists(conn, "story", id)? {
        Ok(())
    } else {
        Err(Error::Invalid("That story is no longer in the Journey.".into()))
    }
}

fn kind_word(conn: &rusqlite::Connection, id: &str) -> Result<String> {
    let t: Option<String> = conn.query_row("SELECT table_name FROM item WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    Ok(match t.as_deref() {
        Some("family") => "family".into(),
        Some("individual") => {
            let k: String = conn.query_row("SELECT kind FROM individual WHERE id = ?1", [id], |r| r.get(0))?;
            if k == "Pet" { "pet".into() } else { "person".into() }
        }
        _ => return Err(Error::Invalid("A story's focus is a person, pet or family.".into())),
    })
}

/// Set or clear the "why it matters" note on a memory in a story.
pub(crate) fn set_note(a: &mut Action, story: &str, memory: &str, note: &str) -> Result<()> {
    let v = text_value(Some(&Value::from(note)));
    a.update("story_memory", row(json!({"story_id": story, "memory_id": memory})), "note", v)
}

/// Put a memory at the end of a story (nothing happens when it is already there).
pub(crate) fn append_memory(a: &mut Action, story: &str, memory: &str) -> Result<()> {
    let there: bool = a.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM story_memory WHERE story_id = ?1 AND memory_id = ?2)",
        [story, memory],
        |r| r.get(0),
    )?;
    if there {
        return Ok(());
    }
    let next: i64 = a.conn().query_row("SELECT coalesce(max(position) + 1, 0) FROM story_memory WHERE story_id = ?1", [story], |r| r.get(0))?;
    a.insert("story_memory", row(json!({"story_id": story, "memory_id": memory, "position": next})))
}

/// Take a story or memory off the home page, and close the gap.
pub(crate) fn unfeature(a: &mut Action, id: &str) -> Result<()> {
    let on: bool = a.conn().query_row("SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1)", [id], |r| r.get(0))?;
    if on {
        a.delete("featured", row(json!({"item_id": id})))?;
        renumber_featured(a)?;
    }
    Ok(())
}

fn renumber_featured(a: &mut Action) -> Result<()> {
    let items: Vec<String> = {
        let mut st = a.conn().prepare("SELECT item_id FROM featured ORDER BY position, rowid")?;
        st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
    };
    for (i, it) in items.iter().enumerate() {
        a.update("featured", row(json!({"item_id": it})), "position", json!(i as i64))?;
    }
    Ok(())
}

/// What removing a story unlinks.
pub(crate) fn story_warning(c: &rusqlite::Connection, id: &str) -> Result<Vec<String>> {
    let mut out = vec![];
    let n: i64 = c.query_row("SELECT count(*) FROM story_memory WHERE story_id = ?1", [id], |r| r.get(0))?;
    if n > 0 {
        out.push(format!("Its {} (they stay in the Journey)", plural(n, "memory", "memories")));
    }
    let focus: Option<String> = c.query_row("SELECT focus_id FROM story WHERE id = ?1", [id], |r| r.get(0))?;
    if let Some(f) = focus {
        out.push(format!("Its focus on {} (they stay in the Journey)", name_of(c, &f)?));
    }
    if c.query_row("SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1)", [id], |r| r.get::<_, bool>(0))? {
        out.push("Its place among the featured items on the home page".into());
    }
    Ok(out)
}

pub(crate) fn remove_story(a: &mut Action, id: &str) -> Result<()> {
    for m in strings(a.conn(), "SELECT memory_id FROM story_memory WHERE story_id = ?1", id)? {
        a.delete("story_memory", row(json!({"story_id": id, "memory_id": m})))?;
    }
    unfeature(a, id)?;
    a.delete_item("story", id)
}

impl Journey {
    /// Every memory with what the Stories & Memories screen sorts and
    /// filters by: name, memory date, date added, people, stories and status.
    pub fn memory_list(&self) -> Result<Vec<MemoryRow>> {
        let c = self.conn();
        let mut added: HashMap<String, String> = HashMap::new();
        let mut st = c.prepare("SELECT item_id, min(at) FROM history WHERE table_name = 'memory' GROUP BY item_id")?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, at) = r?;
            added.insert(id, at);
        }
        let links = |sql: &str| -> Result<HashMap<String, Vec<String>>> {
            let mut out: HashMap<String, Vec<String>> = HashMap::new();
            let mut st = c.prepare(sql)?;
            for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (m, x) = r?;
                let v = out.entry(m).or_default();
                if !v.contains(&x) {
                    v.push(x);
                }
            }
            Ok(out)
        };
        let mut people = links("SELECT memory_id, individual_id FROM memory_individual")?;
        let mut stories = links("SELECT memory_id, story_id FROM story_memory")?;
        let mut st = c.prepare(
            "SELECT m.id, m.name, m.date_edtf, m.status,
                    coalesce((SELECT c.kind FROM content c WHERE c.id = m.cover_id),
                             (SELECT c.kind FROM memory_content mc JOIN content c ON c.id = mc.content_id
                              WHERE mc.memory_id = m.id ORDER BY mc.position LIMIT 1)),
                    coalesce((SELECT c.id FROM content c WHERE c.id = m.cover_id AND c.kind IN ('picture', 'video')),
                             (SELECT c.id FROM memory_content mc JOIN content c ON c.id = mc.content_id
                              WHERE mc.memory_id = m.id AND c.kind IN ('picture', 'video') ORDER BY mc.position LIMIT 1)),
                    m.date, m.caption, m.place_id, m.event_id
             FROM memory m ORDER BY m.name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            let d: Option<String> = r.get(2)?;
            Ok(MemoryRow {
                id: r.get(0)?,
                name: r.get(1)?,
                date_sort: r.get::<_, Option<String>>(6)?.or_else(|| d.as_deref().and_then(dates::nearest)),
                date: d.map(|d| dates::words(&d)).unwrap_or_default(),
                added: None,
                status: r.get(3)?,
                cover_kind: r.get(4)?,
                cover_id: r.get(5)?,
                people: vec![],
                stories: vec![],
                words: r.get::<_, Option<String>>(7)?.map(|c| first_words(&c, 140)).unwrap_or_default(),
                who: String::new(),
                place_id: r.get(8)?,
                event_id: r.get(9)?,
            })
        })?;
        // The first text file in each memory, else a transcript of something in it.
        let mut text_file: HashMap<String, String> = HashMap::new();
        let mut st = c.prepare(
            "SELECT mc.memory_id, cf.path FROM memory_content mc JOIN content_file cf ON cf.content_id = mc.content_id
              WHERE cf.format = 'text/plain' ORDER BY mc.memory_id, mc.position, cf.is_copy",
        )?;
        let mut st2 = c.prepare(
            "SELECT mc.memory_id, cf.path FROM memory_content mc
               JOIN content_relationship cr ON cr.to_id = mc.content_id AND cr.kind = 'transcript of'
               JOIN content_file cf ON cf.content_id = cr.from_id
              WHERE cf.format = 'text/plain' ORDER BY mc.memory_id, mc.position, cf.is_copy",
        )?;
        for q in [&mut st, &mut st2] {
            for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (m, p) = r?;
                text_file.entry(m).or_insert(p);
            }
        }
        let names: HashMap<String, String> = {
            let mut st = c.prepare("SELECT id, name FROM individual")?;
            let x = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<std::result::Result<_, _>>()?;
            x
        };
        let root = self.root().to_path_buf();
        let mut out = Vec::new();
        for r in rows {
            let mut m = r?;
            if m.words.is_empty() {
                if let Some(p) = text_file.get(&m.id) {
                    let full = crate::folder::absolute(&root, p);
                    if let Ok(f) = std::fs::File::open(&full) {
                        use std::io::Read;
                        let mut b = Vec::new();
                        let _ = f.take(800).read_to_end(&mut b);
                        m.words = first_words(String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').trim_end_matches('\u{fffd}'), 140);
                    }
                }
            }
            m.added = added.remove(&m.id);
            m.people = people.remove(&m.id).unwrap_or_default();
            m.stories = stories.remove(&m.id).unwrap_or_default();
            m.who = m.people.iter().filter_map(|p| names.get(p).cloned()).collect::<Vec<_>>().join(", ");
            out.push(m);
        }
        Ok(out)
    }

    pub fn add_story(&mut self, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("A story needs a title.".into()));
        }
        self.act(|a| a.create("story", row(json!({"name": name}))))
    }

    /// Start an "In memory of" page about a person or pet, with a title such
    /// as "In memory of Ellen" or "Celebrating Ellen". Returns its ID.
    pub fn add_tribute(&mut self, about: &str, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("The page needs a title.".into()));
        }
        if kind_word(self.conn(), about)? == "family" {
            return Err(Error::Invalid("An In memory of page is about one person or pet.".into()));
        }
        self.act(|a| a.create("story", row(json!({"name": name, "focus_id": about, "genre": "tribute"}))))
    }

    /// The "In memory of" pages about a person or pet, as (ID, title).
    pub fn tributes_about(&self, about: &str) -> Result<Vec<Entry>> {
        let mut st = self.conn().prepare("SELECT id, name FROM story WHERE genre = 'tribute' AND focus_id = ?1 ORDER BY name, id")?;
        let out = st
            .query_map([about], |r| Ok(Entry { id: r.get(0)?, name: r.get(1)?, note: "In memory of page".into() }))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Say why a memory matters on an "In memory of" page (empty clears it).
    pub fn set_memory_note(&mut self, story: &str, memory: &str, note: &str) -> Result<()> {
        need_story(self.conn(), story)?;
        let there: bool = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM story_memory WHERE story_id = ?1 AND memory_id = ?2)",
            [story, memory],
            |r| r.get(0),
        )?;
        if !there {
            return Err(Error::Invalid("That memory is no longer on this page.".into()));
        }
        self.act(|a| set_note(a, story, memory, note))
    }

    /// Change a story: `name`, `introduction`, `focus_id` (a person, pet or
    /// family; empty for no one).
    pub fn update_story(&mut self, id: &str, changes: &Row) -> Result<()> {
        need_story(self.conn(), id)?;
        for (k, v) in changes {
            match k.as_str() {
                "name" if text_value(Some(v)).is_null() => return Err(Error::Invalid("A story needs a title.".into())),
                "name" | "introduction" => {}
                "focus_id" => {
                    if let Some(f) = v.as_str().filter(|f| !f.is_empty()) {
                        kind_word(self.conn(), f)?;
                    }
                }
                _ => return Err(Error::Invalid(format!("{k} can't be changed on a story."))),
            }
        }
        self.act(|a| {
            for (k, v) in changes {
                a.update("story", id_key(id), k, text_value(Some(v)))?;
            }
            Ok(())
        })
    }

    pub fn story_view(&self, id: &str) -> Result<StoryView> {
        let c = self.conn();
        let base: Option<(String, Option<String>, Option<String>, String)> = c
            .query_row("SELECT name, introduction, focus_id, genre FROM story WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?;
        let (name, introduction, focus, genre) = base.ok_or_else(|| Error::Invalid("That story is no longer in the Journey.".into()))?;
        let focus = match focus {
            Some(f) => Some(Entry { name: name_of(c, &f)?, note: kind_word(c, &f)?, id: f }),
            None => None,
        };
        let mut st = c.prepare(
            "SELECT m.id, m.name, m.date_edtf, m.status,
                    coalesce((SELECT c.kind FROM content c WHERE c.id = m.cover_id),
                             (SELECT c.kind FROM memory_content mc JOIN content c ON c.id = mc.content_id
                              WHERE mc.memory_id = m.id ORDER BY mc.position LIMIT 1)),
                    coalesce((SELECT c.id FROM content c WHERE c.id = m.cover_id AND c.kind IN ('picture', 'video')),
                             (SELECT c.id FROM memory_content mc JOIN content c ON c.id = mc.content_id
                              WHERE mc.memory_id = m.id AND c.kind IN ('picture', 'video') ORDER BY mc.position LIMIT 1)),
                    sm.note
             FROM story_memory sm JOIN memory m ON m.id = sm.memory_id WHERE sm.story_id = ?1 ORDER BY sm.position, sm.rowid",
        )?;
        let memories: Vec<StoryMemory> = st
            .query_map([id], |r| {
                let d: Option<String> = r.get(2)?;
                Ok(StoryMemory {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    date: d.map(|d| dates::words(&d)).unwrap_or_default(),
                    status: r.get(3)?,
                    cover_kind: r.get(4)?,
                    cover_id: r.get(5)?,
                    note: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        let cover_id: Option<String> = c
            .query_row(
                "SELECT coalesce(
                    (SELECT m.cover_id FROM memory m JOIN content c ON c.id = m.cover_id WHERE m.id = sm.memory_id AND c.kind = 'picture'),
                    (SELECT mc.content_id FROM memory_content mc JOIN content c ON c.id = mc.content_id
                     WHERE mc.memory_id = sm.memory_id AND c.kind = 'picture' ORDER BY mc.position LIMIT 1))
                 FROM story_memory sm WHERE sm.story_id = ?1
                   AND EXISTS (SELECT 1 FROM memory_content mc JOIN content c ON c.id = mc.content_id WHERE mc.memory_id = sm.memory_id AND c.kind = 'picture')
                 ORDER BY sm.position LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(StoryView {
            id: id.to_string(),
            name,
            introduction,
            focus,
            memories,
            featured: c.query_row("SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1)", [id], |r| r.get(0))?,
            cover_id,
            genre,
        })
    }

    /// Put existing memories at the end of a story, in the order given.
    pub fn add_memories_to_story(&mut self, story: &str, memories: &[String]) -> Result<()> {
        need_story(self.conn(), story)?;
        for m in memories {
            if !crate::memories::exists(self.conn(), "memory", m)? {
                return Err(Error::Invalid("That memory is no longer in the Journey.".into()));
            }
        }
        self.act(|a| {
            for m in memories {
                append_memory(a, story, m)?;
            }
            Ok(())
        })
    }

    /// Take a memory out of a story; the memory stays in the Journey.
    pub fn remove_memory_from_story(&mut self, story: &str, memory: &str) -> Result<()> {
        self.act(|a| {
            a.delete("story_memory", row(json!({"story_id": story, "memory_id": memory})))?;
            renumber(a, "story_memory", "story_id", story, "memory_id")
        })
    }

    pub fn move_in_story(&mut self, story: &str, memory: &str, delta: i64) -> Result<()> {
        need_story(self.conn(), story)?;
        self.act(|a| move_in_list(a, "story_memory", "story_id", story, "memory_id", memory, delta))
    }

    /// Change the Journey's `name` (its title) or `introduction`.
    pub fn update_journey(&mut self, changes: &Row) -> Result<()> {
        let id = self.id().to_string();
        for (k, v) in changes {
            match k.as_str() {
                "name" if text_value(Some(v)).is_null() => return Err(Error::Invalid("The Journey needs a title.".into())),
                "name" | "introduction" => {}
                _ => return Err(Error::Invalid(format!("{k} can't be changed on the Journey."))),
            }
        }
        self.act(|a| {
            for (k, v) in changes {
                a.update("journey", id_key(&id), k, text_value(Some(v)))?;
            }
            Ok(())
        })
    }

    /// Feature a story or memory on the home page (at the end), or take it off.
    pub fn set_featured(&mut self, id: &str, on: bool) -> Result<()> {
        let t: Option<String> = self.conn().query_row("SELECT table_name FROM item WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        if !matches!(t.as_deref(), Some("story") | Some("memory")) {
            return Err(Error::Invalid("Only stories and memories can be featured.".into()));
        }
        let (now_on, count): (bool, i64) = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1), (SELECT count(*) FROM featured)",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if on && !now_on && count as usize >= MAX_FEATURED {
            return Err(Error::Invalid(format!("Up to {MAX_FEATURED} items can be featured. Take one off the home page first.")));
        }
        self.act(|a| {
            if on && !now_on {
                a.insert("featured", row(json!({"item_id": id, "position": count})))?;
            } else if !on {
                unfeature(a, id)?;
            }
            Ok(())
        })
    }

    pub fn move_featured(&mut self, id: &str, delta: i64) -> Result<()> {
        self.act(|a| {
            renumber_featured(a)?;
            let items: Vec<String> = {
                let mut st = a.conn().prepare("SELECT item_id FROM featured ORDER BY position")?;
                st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
            };
            let from = items.iter().position(|i| i == id).ok_or_else(|| Error::Invalid("That item isn't featured.".into()))? as i64;
            let to = (from + delta).clamp(0, items.len() as i64 - 1);
            let mut order = items.clone();
            let it = order.remove(from as usize);
            order.insert(to as usize, it);
            for (i, it) in order.iter().enumerate() {
                a.update("featured", row(json!({"item_id": it})), "position", Value::from(i as i64))?;
            }
            Ok(())
        })
    }
}
