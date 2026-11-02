//! Sprint 8: turning a picture by quarter turns, and a portrait for each
//! person and pet.
//!
//! The original is never changed. A picture's turn is stored in the
//! catalog; the app's thumbnails and viewer follow it, and the export copy of
//! a JPEG gets the EXIF turn flag (Orientation) without being re-encoded.

use std::collections::HashMap;

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::summary::Entry;
use crate::{id_key, Error, Journey, Result};

/// The EXIF Orientation after turning a picture a further `turn` quarter
/// turns clockwise from how the camera's own flag shows it (1 to 8).
pub fn turned_orientation(camera: u16, turn: i64) -> u16 {
    // Each orientation as (mirrored, clockwise quarter turns): 2/4/5/7 mirror first.
    let (mirror, q) = match camera {
        2 => (true, 0),
        3 => (false, 2),
        4 => (true, 2),
        5 => (true, 3),
        6 => (false, 1),
        7 => (true, 1),
        8 => (false, 3),
        _ => (false, 0),
    };
    match (mirror, (q + turn).rem_euclid(4)) {
        (false, 0) => 1,
        (false, 1) => 6,
        (false, 2) => 3,
        (false, _) => 8,
        (true, 0) => 2,
        (true, 1) => 7,
        (true, 2) => 4,
        (true, _) => 5,
    }
}

impl Journey {
    /// A picture's turn, in quarter turns clockwise (0 to 3).
    pub fn picture_turn(&self, id: &str) -> Result<i64> {
        let t: Option<Option<i64>> = self.conn().query_row("SELECT turn FROM picture WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        Ok(t.flatten().unwrap_or(0).rem_euclid(4))
    }

    /// Turn a picture by `quarters` (1 clockwise, -1 counterclockwise).
    /// Returns its new turn. The original file is not touched.
    pub fn turn_picture(&mut self, id: &str, quarters: i64) -> Result<i64> {
        let is_picture: bool = self.conn().query_row("SELECT EXISTS(SELECT 1 FROM picture WHERE id = ?1)", [id], |r| r.get(0))?;
        if !is_picture {
            return Err(Error::Invalid("Only pictures can be turned.".into()));
        }
        let turn = (self.picture_turn(id)? + quarters).rem_euclid(4);
        let value = if turn == 0 { Value::Null } else { json!(turn) };
        let id = id.to_string();
        self.act(|a| a.update("picture", id_key(&id), "turn", value))?;
        Ok(turn)
    }

    /// Pictures in the memories this person or pet is linked to, for choosing a portrait.
    pub fn portrait_choices(&self, individual: &str) -> Result<Vec<Entry>> {
        let mut st = self.conn().prepare(
            "SELECT DISTINCT c.id, c.name, m.name FROM memory_individual mi
             JOIN memory m ON m.id = mi.memory_id
             JOIN memory_content mc ON mc.memory_id = m.id
             JOIN content c ON c.id = mc.content_id AND c.kind = 'picture'
             WHERE mi.individual_id = ?1 GROUP BY c.id ORDER BY m.name COLLATE NOCASE, mc.position",
        )?;
        let out = st.query_map([individual], |r| Ok(Entry { id: r.get(0)?, name: r.get(1)?, note: r.get(2)? }))?.collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Choose (or with `None`, clear) the portrait of a person or pet.
    pub fn set_portrait(&mut self, individual: &str, picture: Option<&str>) -> Result<()> {
        let known: bool = self.conn().query_row("SELECT EXISTS(SELECT 1 FROM individual WHERE id = ?1)", [individual], |r| r.get(0))?;
        if !known {
            return Err(Error::Invalid("That person or pet is no longer in the Journey.".into()));
        }
        if let Some(p) = picture {
            let ok: bool = self.conn().query_row("SELECT EXISTS(SELECT 1 FROM content WHERE id = ?1 AND kind = 'picture')", [p], |r| r.get(0))?;
            if !ok {
                return Err(Error::Invalid("A portrait has to be a picture in the Journey.".into()));
            }
        }
        let value = picture.map(|p| json!(p)).unwrap_or(Value::Null);
        let id = individual.to_string();
        self.act(|a| a.update("individual", id_key(&id), "image_id", value))
    }

    /// The portrait of everyone who has one: person or pet ID to picture ID.
    pub fn portraits(&self) -> Result<HashMap<String, String>> {
        let mut st = self.conn().prepare("SELECT id, image_id FROM individual WHERE image_id IS NOT NULL")?;
        let out = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }
}
