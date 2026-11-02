-- Catalog schema, format version 1.0.
-- One table per item type.
-- Every table has `updated` (time of the last change), not exported.

-- Registry of every permanent ID and the table that holds it (internal
-- only). It keeps IDs unique across all item types.
CREATE TABLE item (
  id         TEXT PRIMARY KEY,
  table_name TEXT NOT NULL
);

CREATE TABLE journey (
  id             TEXT PRIMARY KEY REFERENCES item(id),
  name           TEXT NOT NULL CHECK (name <> ''),
  introduction   TEXT,
  language       TEXT NOT NULL DEFAULT 'en',
  format_version TEXT NOT NULL,
  date_modified  TEXT NOT NULL,
  updated        TEXT NOT NULL
);

CREATE TABLE featured (
  position INTEGER NOT NULL,
  item_id  TEXT NOT NULL REFERENCES item(id),
  updated  TEXT NOT NULL,
  PRIMARY KEY (item_id)
);

CREATE TABLE individual (
  id              TEXT PRIMARY KEY REFERENCES item(id),
  kind            TEXT NOT NULL CHECK (kind IN ('Person', 'Pet')),
  name            TEXT NOT NULL CHECK (name <> ''),
  birth_date      TEXT,
  birth_date_edtf TEXT,
  death_date      TEXT,
  death_date_edtf TEXT,
  description     TEXT,
  image_id        TEXT REFERENCES item(id),
  updated         TEXT NOT NULL
);

CREATE TABLE person (
  id              TEXT PRIMARY KEY REFERENCES individual(id),
  given_name      TEXT,
  family_name     TEXT,
  other_names     TEXT,
  gender          TEXT,
  living_override TEXT CHECK (living_override IN ('living', 'not living')),
  updated         TEXT NOT NULL
);

CREATE TABLE pet (
  id               TEXT PRIMARY KEY REFERENCES individual(id),
  species          TEXT,
  breed            TEXT,
  joined_date_edtf TEXT,
  updated          TEXT NOT NULL
);

CREATE TABLE individual_relationship (
  id          TEXT PRIMARY KEY REFERENCES item(id),
  from_id     TEXT NOT NULL REFERENCES individual(id),
  to_id       TEXT NOT NULL REFERENCES individual(id),
  kind        TEXT NOT NULL CHECK (kind <> ''),
  qualifier   TEXT,
  certainty   TEXT CHECK (certainty IN ('certain', 'probably')),
  source_note TEXT,
  status      TEXT NOT NULL CHECK (status IN ('entered', 'suggested', 'accepted', 'rejected')),
  updated     TEXT NOT NULL
);

CREATE TABLE family (
  id          TEXT PRIMARY KEY REFERENCES item(id),
  name        TEXT NOT NULL CHECK (name <> ''),
  description TEXT,
  updated     TEXT NOT NULL
);

CREATE TABLE family_member (
  family_id     TEXT NOT NULL REFERENCES family(id),
  individual_id TEXT NOT NULL REFERENCES individual(id),
  updated       TEXT NOT NULL,
  PRIMARY KEY (family_id, individual_id)
);

CREATE TABLE family_relationship (
  from_id TEXT NOT NULL REFERENCES family(id),
  to_id   TEXT NOT NULL REFERENCES family(id),
  updated TEXT NOT NULL,
  PRIMARY KEY (from_id, to_id)
);

CREATE TABLE place (
  id          TEXT PRIMARY KEY REFERENCES item(id),
  name        TEXT NOT NULL CHECK (name <> ''),
  address     TEXT,
  latitude    REAL,
  longitude   REAL,
  exact       INTEGER NOT NULL DEFAULT 0 CHECK (exact IN (0, 1)),
  description TEXT,
  updated     TEXT NOT NULL
);

CREATE TABLE event (
  id              TEXT PRIMARY KEY REFERENCES item(id),
  kind            TEXT NOT NULL CHECK (kind IN ('birth', 'death', 'wedding', 'divorce', 'move',
                    'graduation', 'reunion', 'funeral', 'holiday', 'trip')),
  name            TEXT NOT NULL CHECK (name <> ''),
  start_date      TEXT,
  start_date_edtf TEXT,
  end_date        TEXT,
  end_date_edtf   TEXT,
  place_id        TEXT REFERENCES place(id),
  description     TEXT,
  updated         TEXT NOT NULL
);

-- individual_id holds an individual or a family ("an individual or a family").
CREATE TABLE event_individual (
  event_id      TEXT NOT NULL REFERENCES event(id),
  individual_id TEXT NOT NULL REFERENCES item(id),
  position      INTEGER NOT NULL DEFAULT 0,
  updated       TEXT NOT NULL,
  PRIMARY KEY (event_id, individual_id)
);

CREATE TABLE story (
  id           TEXT PRIMARY KEY REFERENCES item(id),
  name         TEXT NOT NULL CHECK (name <> ''),
  introduction TEXT,
  focus_id     TEXT REFERENCES item(id),
  updated      TEXT NOT NULL,
  -- 1.5: 'tribute' for an "In memory of" page about the focus person (sprint 13).
  genre        TEXT NOT NULL DEFAULT 'story' CHECK (genre IN ('story', 'tribute'))
);

CREATE TABLE memory (
  id        TEXT PRIMARY KEY REFERENCES item(id),
  name      TEXT NOT NULL CHECK (name <> ''),
  date      TEXT,
  date_edtf TEXT,
  caption   TEXT,
  status    TEXT NOT NULL DEFAULT 'needs content'
              CHECK (status IN ('complete', 'needs content', 'needs info')),
  place_id  TEXT REFERENCES place(id),
  event_id  TEXT REFERENCES event(id),
  cover_id  TEXT REFERENCES item(id),
  finished  TEXT,
  updated   TEXT NOT NULL
);

CREATE TABLE story_memory (
  story_id  TEXT NOT NULL REFERENCES story(id),
  memory_id TEXT NOT NULL REFERENCES memory(id),
  position  INTEGER NOT NULL,
  updated   TEXT NOT NULL,
  note      TEXT, -- 1.5: why this memory matters, on an "In memory of" page
  PRIMARY KEY (story_id, memory_id)
);

CREATE TABLE memory_individual (
  memory_id     TEXT NOT NULL REFERENCES memory(id),
  individual_id TEXT NOT NULL REFERENCES item(id),
  role          TEXT NOT NULL CHECK (role <> ''),
  role_note     TEXT,
  certainty     TEXT CHECK (certainty IN ('certain', 'probably')),
  updated       TEXT NOT NULL,
  PRIMARY KEY (memory_id, individual_id, role),
  CHECK (role <> 'other' OR (role_note IS NOT NULL AND role_note <> ''))
);

CREATE TABLE content (
  id          TEXT PRIMARY KEY REFERENCES item(id),
  kind        TEXT NOT NULL CHECK (kind IN ('picture', 'voice recording', 'video', 'text')),
  state       TEXT NOT NULL DEFAULT 'waiting' CHECK (state IN ('waiting', 'set aside', 'in a memory')),
  name        TEXT NOT NULL CHECK (name <> ''),
  date        TEXT,
  date_edtf   TEXT,
  description TEXT,
  author      TEXT,
  copyright   TEXT,
  source_note TEXT,
  date_added  TEXT NOT NULL DEFAULT (date('now')),
  added_by    TEXT,
  updated     TEXT NOT NULL
);

-- Content enters the Journey only through a memory, and only this table links it.
CREATE TABLE memory_content (
  memory_id  TEXT NOT NULL REFERENCES memory(id),
  content_id TEXT NOT NULL REFERENCES content(id),
  position   INTEGER NOT NULL,
  updated    TEXT NOT NULL,
  PRIMARY KEY (memory_id, content_id)
);

CREATE TABLE picture (
  id          TEXT PRIMARY KEY REFERENCES content(id),
  width       INTEGER,
  height      INTEGER,
  people_note TEXT,
  latitude    REAL,
  longitude   REAL,
  camera      TEXT,
  turn        INTEGER,
  updated     TEXT NOT NULL
);

CREATE TABLE voice_recording (
  id           TEXT PRIMARY KEY REFERENCES content(id),
  duration     TEXT,
  language     TEXT,
  speaker_note TEXT,
  quiet_start  REAL,
  play_full    INTEGER NOT NULL DEFAULT 0 CHECK (play_full IN (0, 1)),
  updated      TEXT NOT NULL
);

CREATE TABLE video (
  id           TEXT PRIMARY KEY REFERENCES content(id),
  duration     TEXT,
  width        INTEGER,
  height       INTEGER,
  language     TEXT,
  people_note  TEXT,
  speaker_note TEXT,
  latitude     REAL,
  longitude    REAL,
  camera       TEXT,
  updated      TEXT NOT NULL
);

CREATE TABLE text_content (
  id             TEXT PRIMARY KEY REFERENCES content(id),
  language       TEXT,
  summary        TEXT,
  transcribed_by TEXT,
  review_status  TEXT CHECK (review_status IN ('automatic', 'checked')),
  updated        TEXT NOT NULL
);

CREATE TABLE content_file (
  content_id    TEXT NOT NULL REFERENCES content(id),
  is_copy       INTEGER NOT NULL CHECK (is_copy IN (0, 1)),
  path          TEXT NOT NULL UNIQUE,
  format        TEXT NOT NULL,
  size          INTEGER NOT NULL,
  sha256        TEXT NOT NULL,
  original_name TEXT,
  updated       TEXT NOT NULL,
  PRIMARY KEY (content_id, path)
);

CREATE TABLE content_relationship (
  from_id TEXT NOT NULL REFERENCES content(id),
  to_id   TEXT NOT NULL REFERENCES content(id),
  kind    TEXT NOT NULL CHECK (kind IN ('transcript of', 'captions of', 'translation of',
            'typed copy of', 'back or page of', 'still frame of', 'clip of', 'edited version of')),
  updated TEXT NOT NULL,
  PRIMARY KEY (from_id, to_id, kind)
);

-- An ID that was merged into another item (format 1.4, sprint 10): links
-- written with the old ID still find the item it became.
CREATE TABLE alias (
  old_id  TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES item(id),
  updated TEXT NOT NULL
);

CREATE TABLE history (
  seq        INTEGER PRIMARY KEY AUTOINCREMENT,
  at         TEXT NOT NULL,
  author     TEXT NOT NULL,
  action_id  TEXT NOT NULL,
  table_name TEXT NOT NULL,
  item_id    TEXT NOT NULL,
  field      TEXT,
  old_value  TEXT,
  new_value  TEXT
);
CREATE INDEX history_action ON history(action_id);
