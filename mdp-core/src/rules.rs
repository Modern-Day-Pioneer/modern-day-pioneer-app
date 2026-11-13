//! Import rules: an ordered, editable list of patterns that fill in a bulk
//! import plan from folder and file names. Deterministic: the same folders
//! and the same rules always give the same plan, and every value says which
//! rule set it.
//!
//! The rules live in a plain Markdown file that people can read, share and
//! edit (see `data/import-rules/family-history.md` for the built-in set):
//!
//! ```text
//! # Import rules: Family history folders
//!
//! ## 3. Date folder
//! applies to: folder
//! pattern: ^(?<year>\d{4})[ ._-](?<month>\d{2})[ ._-](?<day>\d{2})$
//! kind: date
//! sets: date = {year}-{month}-{day}
//! then: stop
//! note: One-day folders written as "2003 07 19".
//!
//! # Names
//! - June
//!
//! # Initials
//! WJR = Walter J. Rowan
//!
//! # Spelling fixes
//! Vaccation = Vacation
//!
//! # Skip folders
//! - Edited
//!
//! # Settings
//! day 01 means month only: yes
//! later photo years: 10
//! ```
//!
//! The rules run in three stages: each folder from the top down (a folder
//! rule may give the folder a kind such as `date`, `era` or `event`), then
//! the file name (first match wins, unless a rule says `then: continue` or
//! `then: remove`), then a combine step (date order, the later-photo check).
//! Patterns use the `regex` crate, which always runs in linear time, so a
//! badly written pattern can't freeze an import.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::{Captures, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::{dates, Error, Result};

/// The built-in rule set for folders organized as family histories.
pub const BUILT_IN: &str = include_str!("../data/import-rules/family-history.md");

/// The file name that, at the top of a chosen folder, holds its rules.
pub const FOLDER_FILE: &str = "_import-rules.md";

/// Fields a rule can set.
pub const FIELDS: [&str; 12] =
    ["story", "family", "memory", "date", "place", "series", "photo_by", "title", "description", "note", "people", "maybe people"];

/// What a rule looks at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// The first folder under the chosen folder.
    TopFolder,
    /// Any folder.
    Folder,
    /// The folder at this level (1 = top).
    FolderLevel(usize),
    /// The file's name without its extension.
    FileName,
}

/// What happens after a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Then {
    /// No later rule looks at this name.
    Stop,
    /// Later rules look at the same name too.
    Continue,
    /// The matched text is taken out of the name and later rules look at the rest.
    Remove,
}

/// A condition on where the name is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cond {
    /// For a file: the folder it is in has (or hasn't) this kind. For a
    /// folder: the folder it is in.
    FolderIs { kind: String, not: bool },
    /// The folder above that one has (or hasn't) this kind.
    ParentIs { kind: String, not: bool },
    /// Some folder above has this kind.
    Inside { kind: String, not: bool },
    /// The named group (or the whole match) is on the names list, or a
    /// first name already in the Journey.
    KnownName { group: String },
}

/// One rule.
#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    /// As written in the heading: `3`, `13a`.
    pub number: String,
    pub title: String,
    pub target: Target,
    pub conditions: Vec<Cond>,
    pub pattern: String,
    #[serde(skip)]
    pub regex: Regex,
    /// The kind given to a folder it matches.
    pub kind: Option<String>,
    /// (field, template), in order.
    pub sets: Vec<(String, String)>,
    pub then: Then,
    /// The picture's own date is not used for files it matches.
    pub ignore_photo_date: bool,
    pub on: bool,
    pub note: String,
}

impl Rule {
    /// `13. Series under an event folder`.
    pub fn label(&self) -> String {
        format!("{}. {}", self.number, self.title)
    }
}

/// A rules file, read.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RuleSet {
    /// From the file's first heading.
    pub name: String,
    /// Where it came from: `built in`, or the file's path.
    pub source: String,
    pub rules: Vec<Rule>,
    /// First names (or full names) confirmed as people.
    pub names: Vec<String>,
    /// Initials and who they stand for.
    pub initials: Vec<(String, String)>,
    /// Misspellings and their fixes, applied to whole words in every name.
    pub fixes: Vec<(String, String)>,
    /// Folders whose files act as if they sat in the folder above.
    pub skip: Vec<String>,
    /// In 8-digit dates, day 01 means only the month is known and 0101
    /// only the year.
    pub day01_unknown: bool,
    /// A file dated this many years or more after its era folder ends is a
    /// later photo of that time (0: off).
    pub later_photo_years: i32,
    /// Lines that could not be read, and why. Those rules are left out.
    pub problems: Vec<String>,
}

fn strip_ticks(s: &str) -> &str {
    let t = s.trim();
    t.strip_prefix('`').and_then(|x| x.strip_suffix('`')).unwrap_or(t)
}

fn kind_word(s: &str) -> String {
    let s = s.trim().to_lowercase();
    let s = s.strip_prefix("a ").or_else(|| s.strip_prefix("an ")).unwrap_or(&s).to_string();
    s.strip_suffix(" folder").unwrap_or(&s).trim().to_string()
}

fn parse_cond(text: &str) -> Option<Cond> {
    let t = text.trim().to_lowercase();
    let neg = |rest: &str| -> (String, bool) {
        match rest.strip_prefix("not ") {
            Some(r) => (kind_word(r), true),
            None => (kind_word(rest), false),
        }
    };
    if let Some(r) = t.strip_prefix("the folder is ") {
        let (kind, not) = neg(r);
        return Some(Cond::FolderIs { kind, not });
    }
    if let Some(r) = t.strip_prefix("the parent is ") {
        let (kind, not) = neg(r);
        return Some(Cond::ParentIs { kind, not });
    }
    if let Some(r) = t.strip_prefix("not inside ") {
        return Some(Cond::Inside { kind: kind_word(r), not: true });
    }
    if let Some(r) = t.strip_prefix("inside ") {
        return Some(Cond::Inside { kind: kind_word(r), not: false });
    }
    if let Some(r) = t.strip_suffix(" is a known name") {
        let g = r.trim().trim_start_matches('{').trim_end_matches('}').to_string();
        let g = if g == "the name" || g == "it" { String::new() } else { g };
        return Some(Cond::KnownName { group: g });
    }
    None
}

fn parse_target(text: &str) -> Option<(Target, Vec<Cond>)> {
    let t = text.trim();
    let lower = t.to_lowercase();
    let (head, rest) = match lower.find(" when ") {
        Some(i) => (lower[..i].trim_end_matches(',').trim().to_string(), Some(lower[i + 6..].to_string())),
        None => (lower.trim_end_matches(',').trim().to_string(), None),
    };
    let target = match head.as_str() {
        "top folder" | "the top folder" => Target::TopFolder,
        "folder" | "any folder" | "folders" => Target::Folder,
        "file name" | "file" | "file names" => Target::FileName,
        h => {
            let n = h.strip_prefix("folder at level ")?.trim().parse().ok().filter(|n| *n >= 1)?;
            Target::FolderLevel(n)
        }
    };
    let mut conds = Vec::new();
    if let Some(rest) = rest {
        for part in rest.split(" and ") {
            conds.push(parse_cond(part)?);
        }
    }
    Some((target, conds))
}

fn yes(s: &str) -> bool {
    matches!(s.trim().to_lowercase().as_str(), "yes" | "y" | "true" | "on" | "1")
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Rules,
    Names,
    Initials,
    Fixes,
    Skip,
    Settings,
}

impl RuleSet {
    /// The built-in set.
    pub fn built_in() -> RuleSet {
        let mut r = RuleSet::parse(BUILT_IN);
        r.source = "built in".into();
        r
    }

    /// Read a rules file. Rules that can't be read are left out and listed
    /// in `problems`; nothing here fails.
    pub fn parse(text: &str) -> RuleSet {
        let mut set = RuleSet { later_photo_years: 10, ..RuleSet::default() };
        let mut section = Section::Rules;
        // The rule being read: heading line number, number, title, and its lines.
        let mut current: Option<(usize, String, String, Vec<(usize, String)>)> = None;
        let mut in_code = false;
        let finish = |set: &mut RuleSet, cur: Option<(usize, String, String, Vec<(usize, String)>)>| {
            if let Some((line, number, title, body)) = cur {
                match build_rule(&number, &title, &body) {
                    Ok(r) => set.rules.push(r),
                    Err(e) => set.problems.push(format!("Line {line}, rule {number}: {e}")),
                }
            }
        };
        for (i, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line_no = i + 1;
            let line = raw.trim_end();
            if line.trim_start().starts_with("```") {
                in_code = !in_code;
                continue;
            }
            if in_code {
                continue;
            }
            if let Some(h) = line.strip_prefix("# ") {
                finish(&mut set, current.take());
                let h = h.trim();
                let lower = h.to_lowercase();
                section = if lower.starts_with("names") {
                    Section::Names
                } else if lower.starts_with("initials") {
                    Section::Initials
                } else if lower.starts_with("spelling") {
                    Section::Fixes
                } else if lower.starts_with("skip") {
                    Section::Skip
                } else if lower.starts_with("settings") {
                    Section::Settings
                } else {
                    if set.name.is_empty() {
                        set.name = h.strip_prefix("Import rules:").unwrap_or(h).trim().to_string();
                    }
                    Section::Rules
                };
                continue;
            }
            if let Some(h) = line.strip_prefix("## ") {
                finish(&mut set, current.take());
                if section != Section::Rules {
                    set.problems.push(format!("Line {line_no}: a rule heading outside the rules is ignored."));
                    continue;
                }
                let h = h.trim();
                let (number, title) = match h.split_once(". ") {
                    Some((n, t)) if !n.is_empty() && n.len() <= 6 && !n.contains(' ') => (n.to_string(), t.trim().to_string()),
                    _ => ((set.rules.len() + 1).to_string(), h.to_string()),
                };
                current = Some((line_no, number, title, Vec::new()));
                continue;
            }
            let t = line.trim();
            if t.is_empty() || t.starts_with('>') || t.starts_with("<!--") || t.starts_with("//") {
                continue;
            }
            let item = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t).trim();
            match section {
                Section::Rules => {
                    if let Some((_, _, _, body)) = current.as_mut() {
                        body.push((line_no, t.to_string()));
                    }
                }
                Section::Names => set.names.extend(item.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())),
                Section::Skip => set.skip.push(item.to_string()),
                Section::Initials | Section::Fixes => match item.split_once('=') {
                    Some((a, b)) if !a.trim().is_empty() && !b.trim().is_empty() => {
                        let pair = (a.trim().to_string(), b.trim().to_string());
                        if section == Section::Initials { set.initials.push(pair) } else { set.fixes.push(pair) }
                    }
                    _ => set.problems.push(format!("Line {line_no}: expected \"this = that\".")),
                },
                Section::Settings => match item.split_once(':') {
                    Some((k, v)) => match k.trim().to_lowercase().as_str() {
                        "day 01 means month only" | "day 01 means unknown" => set.day01_unknown = yes(v),
                        "later photo years" => match v.trim().parse() {
                            Ok(n) => set.later_photo_years = n,
                            Err(_) => set.problems.push(format!("Line {line_no}: \"later photo years\" needs a number.")),
                        },
                        other => set.problems.push(format!("Line {line_no}: unknown setting \"{other}\".")),
                    },
                    None => set.problems.push(format!("Line {line_no}: expected \"setting: value\".")),
                },
            }
        }
        finish(&mut set, current.take());
        set
    }

    /// Read a rules file from disk.
    pub fn read(path: &std::path::Path) -> Result<RuleSet> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Invalid(format!("The rules file {} could not be read: {e}", path.display())))?;
        let mut r = RuleSet::parse(&text);
        r.source = path.display().to_string();
        if r.name.is_empty() {
            r.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        }
        Ok(r)
    }

    /// The rules for a folder import: `none`, `built in`, a rules file's
    /// path, or blank for the folder's own `_import-rules.md` when it has one.
    pub fn choose(folder: &std::path::Path, choice: &str) -> Result<Option<RuleSet>> {
        match choice.trim() {
            "none" => Ok(None),
            "built in" | "builtin" => Ok(Some(RuleSet::built_in())),
            "" | "auto" => {
                let f = folder.join(FOLDER_FILE);
                if f.is_file() { RuleSet::read(&f).map(Some) } else { Ok(None) }
            }
            path => RuleSet::read(std::path::Path::new(path)).map(Some),
        }
    }

    fn fix(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (from, to) in &self.fixes {
            if let Ok(r) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(from))) {
                out = r.replace_all(&out, regex::NoExpand(to)).into_owned();
            }
        }
        out
    }

    fn skipped(&self, folder: &str) -> bool {
        self.skip.iter().any(|s| s.eq_ignore_ascii_case(folder.trim()))
    }

    /// Whether a name is confirmed: on the names list (whole name or first
    /// word) or known to the caller (people already in the Journey).
    pub fn confirmed(&self, name: &str, known: &dyn Fn(&str) -> bool) -> bool {
        let first = name.split_whitespace().next().unwrap_or("");
        self.names.iter().any(|n| n.eq_ignore_ascii_case(name.trim()) || n.eq_ignore_ascii_case(first)) || known(name) || known(first)
    }

    /// Run the rules on one file's path below the chosen folder (folders
    /// then the file name, with the extension), `known` telling whether a
    /// name is already in the Journey.
    pub fn apply(&self, segments: &[String], known: &dyn Fn(&str) -> bool) -> Outcome {
        let mut out = Outcome { photo_date: true, ..Outcome::default() };
        let Some((file, folders)) = segments.split_last() else { return out };
        // (name, kind) of each folder that isn't skipped, top first.
        let mut kept: Vec<(String, String)> = Vec::new();
        let mut top = String::new();
        for (i, raw) in folders.iter().enumerate() {
            let level = i + 1;
            let name = self.fix(raw);
            if level == 1 {
                top = name.clone();
            }
            if self.skipped(&name) {
                out.matched("skip folders", "Skip folders");
                continue;
            }
            let mut text = name.clone();
            let mut kind = String::from("folder");
            for rule in self.rules.iter().filter(|r| r.on) {
                let fits = match rule.target {
                    Target::TopFolder => level == 1,
                    Target::Folder => true,
                    Target::FolderLevel(n) => n == level,
                    Target::FileName => false,
                };
                if !fits {
                    continue;
                }
                let Some(c) = rule.regex.captures(&text) else { continue };
                // For a folder, "the folder" and "the parent" are both the folder it is in.
                let parent = kept.last().map(|k| k.1.as_str()).unwrap_or("none");
                if !conds_hold(rule, &c, parent, parent, &kept, self, known) {
                    continue;
                }
                if let Some(k) = &rule.kind {
                    kind = k.clone();
                }
                let memory_so_far = out.fields.get("memory").cloned().unwrap_or_default();
                let ctx = Places { folder: &name, parent: kept.last().map(|k| k.0.as_str()).unwrap_or(""), top: &top, file: "", folder_memory: &memory_so_far };
                out.set_all(rule, &c, &ctx, self);
                if kind == "era" {
                    out.era = rule_era(&out.fields);
                }
                match rule.then {
                    Then::Stop => break,
                    Then::Continue => {}
                    Then::Remove => text = tidy(&rule.regex.replace_all(&text, " ")),
                }
            }
            if kind == "skip" {
                continue;
            }
            kept.push((name, kind));
        }
        // The file name, without its extension.
        let stem = file.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(file);
        let mut text = self.fix(stem);
        let folder = kept.last().map(|k| k.0.clone()).unwrap_or_default();
        let parent_name = kept.len().checked_sub(2).and_then(|i| kept.get(i)).map(|k| k.0.clone()).unwrap_or_default();
        let fkind = kept.last().map(|k| k.1.clone()).unwrap_or_else(|| "none".into());
        let pkind = kept.len().checked_sub(2).and_then(|i| kept.get(i)).map(|k| k.1.clone()).unwrap_or_else(|| "none".into());
        let folder_rule_labels: Vec<String> = self.rules.iter().filter(|r| r.target != Target::FileName).map(Rule::label).collect();
        for rule in self.rules.iter().filter(|r| r.on && r.target == Target::FileName) {
            let Some(c) = rule.regex.captures(&text) else { continue };
            if !conds_hold(rule, &c, &fkind, &pkind, &kept, self, known) {
                continue;
            }
            let folder_memory = out.fields.get("memory").cloned().filter(|_| out.memory_rule.as_ref().is_some_and(|r| !folder_rule_labels.contains(r)) == false).unwrap_or_else(|| folder.clone());
            let ctx = Places { folder: &folder, parent: &parent_name, top: &top, file: &text, folder_memory: &folder_memory };
            let before = out.fields.contains_key("date");
            out.set_all(rule, &c, &ctx, self);
            if rule.sets.iter().any(|(f, _)| f == "date") && (out.fields.contains_key("date") || !before) {
                out.date_from_file = out.fields.contains_key("date");
            }
            if rule.ignore_photo_date {
                out.photo_date = false;
            }
            match rule.then {
                Then::Remove => {
                    if rule.sets.iter().any(|(f, _)| f == "note") || rule.title.to_lowercase().contains("edit") {
                        out.edition = true;
                    }
                    text = tidy(&rule.regex.replace_all(&text, " "));
                }
                Then::Continue => out.caught_by_file = true,
                Then::Stop => {
                    out.caught_by_file = true;
                    break;
                }
            }
        }
        out.clean_name = text;
        // Initials anywhere in the path name the people they stand for.
        if !self.initials.is_empty() {
            for seg in folders.iter().chain(std::iter::once(&stem.to_string())) {
                for (ini, who) in &self.initials {
                    let r = Regex::new(&format!(r"\b{}\b", regex::escape(ini))).ok();
                    if r.is_some_and(|r| r.is_match(seg)) && !out.people.contains(who) {
                        out.people.push(who.clone());
                        out.matched("initials", "Initials");
                    }
                }
            }
        }
        // Names from file names: confirmed ones join People, the rest are listed to confirm.
        for n in std::mem::take(&mut out.maybe_people) {
            if self.confirmed(&n, known) {
                if !out.people.contains(&n) {
                    out.people.push(n);
                }
            } else if !out.to_confirm.contains(&n) {
                out.to_confirm.push(n);
            }
        }
        // Whoever took or gave the pictures is credited, not a person in them.
        if let Some(by) = out.fields.get("photo_by").cloned() {
            out.people.retain(|p| !p.eq_ignore_ascii_case(&by) && p.split_whitespace().next().is_none_or(|f| !f.eq_ignore_ascii_case(&by)));
        }
        out
    }

    /// Stage C, once the picture's own date is known: a memory per day for
    /// files only that date can group, and the later-photo check.
    pub fn finish(&self, out: &mut Outcome, photo_date: Option<&str>) {
        if out.fields.get("memory").is_some_and(|m| m.contains("{photo day}")) {
            let own = if out.date_from_file { out.fields.get("date").map(String::as_str) } else { photo_date.filter(|_| out.photo_date) };
            let day = own.and_then(|d| dates::parse(d.get(..10).unwrap_or(d)).ok().flatten()).map(|d| d.words);
            match day {
                Some(w) => {
                    let m = out.fields["memory"].replace("{photo day}", &w);
                    out.fields.insert("memory".into(), tidy(&m));
                }
                None => {
                    out.fields.remove("memory");
                    out.memory_rule = None;
                }
            }
        }
        let Some((from, to)) = out.era else { return };
        let own = if out.date_from_file { out.fields.get("date").cloned() } else { photo_date.filter(|_| out.photo_date).map(str::to_string) };
        let Some(own) = own else { return };
        let Some(year) = dates::parse(&own).ok().flatten().and_then(|d| dates::first_year(&d.edtf)) else { return };
        if self.later_photo_years > 0 && year >= to + self.later_photo_years {
            out.fields.insert("date".into(), format!("{from} to {to}"));
            out.date_rule = Some("later photo".into());
            out.date_from_file = false;
            out.photo_date = false;
            out.add_note(&format!("Photographed {}, later than the time it shows.", dates::parse(&own).ok().flatten().map(|d| d.words).unwrap_or(own.clone())));
            out.matched("later photo", "Later photo of an earlier time");
        } else if year < from || year > to {
            out.add_note(&format!("Its date ({year}) is outside its folder's years ({from} to {to})."));
        }
    }
}

/// What the rules made of one file.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Outcome {
    /// Field to value (people kept apart).
    pub fields: BTreeMap<String, String>,
    pub people: Vec<String>,
    /// Names from file names not yet confirmed (only during `apply`).
    pub maybe_people: Vec<String>,
    /// Names found that are not on the names list or in the Journey.
    pub to_confirm: Vec<String>,
    /// Labels of the rules that matched, in order.
    pub rules: Vec<String>,
    /// The rule that set the memory, for the coverage report.
    pub memory_rule: Option<String>,
    /// The rule that set the date.
    pub date_rule: Option<String>,
    /// The nearest era folder's years.
    pub era: Option<(i32, i32)>,
    /// The date came from the file name.
    pub date_from_file: bool,
    /// The picture's own date may be used.
    pub photo_date: bool,
    /// A file rule (other than `then: remove`) matched.
    pub caught_by_file: bool,
    /// An edition mark (an edited copy) was taken out of the name.
    pub edition: bool,
    /// The file name after `then: remove` rules.
    pub clean_name: String,
}

impl Outcome {
    fn matched(&mut self, number: &str, title: &str) {
        let label = if number.chars().next().is_some_and(|c| c.is_ascii_digit()) { format!("{number}. {title}") } else { title.to_string() };
        if !self.rules.contains(&label) {
            self.rules.push(label);
        }
    }

    fn add_note(&mut self, n: &str) {
        let v = self.fields.entry("note".into()).or_default();
        if !v.contains(n) {
            if !v.is_empty() {
                v.push(' ');
            }
            v.push_str(n);
        }
    }

    fn set_all(&mut self, rule: &Rule, c: &Captures, ctx: &Places, set: &RuleSet) {
        self.matched(&rule.number, &rule.title);
        for (field, template) in &rule.sets {
            let value = render(template, c, ctx, set);
            match field.as_str() {
                "people" => {
                    for p in split_names(&value) {
                        if !self.people.contains(&p) {
                            self.people.push(p);
                        }
                    }
                }
                "maybe people" => {
                    for p in split_names(&value) {
                        if !self.maybe_people.contains(&p) {
                            self.maybe_people.push(p);
                        }
                    }
                }
                "note" => {
                    if !value.trim().is_empty() {
                        self.add_note(value.trim());
                    }
                }
                "date" => {
                    let v = value.trim();
                    if v.eq_ignore_ascii_case("none") {
                        self.fields.remove("date");
                        self.date_rule = Some(rule.label());
                        continue;
                    }
                    if v.is_empty() {
                        continue;
                    }
                    match read_date(v, set.day01_unknown) {
                        Some(d) => {
                            self.fields.insert("date".into(), d);
                            self.date_rule = Some(rule.label());
                        }
                        None => self.add_note(&format!("Rule {} found the date \u{201c}{v}\u{201d} but it isn't one the app can read.", rule.number)),
                    }
                }
                f => {
                    let v = tidy(&value);
                    if v.is_empty() {
                        continue;
                    }
                    if f == "memory" {
                        self.memory_rule = Some(rule.label());
                    }
                    self.fields.insert(f.to_string(), v);
                }
            }
        }
    }
}

struct Places<'a> {
    /// The memory folder rules gave, or else the folder's name.
    folder_memory: &'a str,
    folder: &'a str,
    parent: &'a str,
    top: &'a str,
    file: &'a str,
}

fn conds_hold(rule: &Rule, c: &Captures, folder_kind: &str, parent_kind: &str, kept: &[(String, String)], set: &RuleSet, known: &dyn Fn(&str) -> bool) -> bool {
    rule.conditions.iter().all(|cond| match cond {
        Cond::FolderIs { kind, not } => one_of(kind, folder_kind) != *not,
        Cond::ParentIs { kind, not } => one_of(kind, parent_kind) != *not,
        Cond::Inside { kind, not } => kept.iter().any(|k| one_of(kind, &k.1)) != *not,
        Cond::KnownName { group } => {
            let v = if group.is_empty() { c.get(0) } else { c.name(group) }.map(|m| m.as_str()).unwrap_or("");
            !v.trim().is_empty() && set.confirmed(v.trim(), known)
        }
    })
}

/// `event or contributor` holds for either kind.
fn one_of(kinds: &str, kind: &str) -> bool {
    kinds.split(" or ").any(|k| kind_word(k) == kind)
}

fn rule_era(fields: &BTreeMap<String, String>) -> Option<(i32, i32)> {
    let d = dates::parse(fields.get("date")?).ok().flatten()?;
    let (a, b) = d.edtf.split_once('/')?;
    Some((a.get(..4)?.parse().ok()?, b.get(..4)?.parse().ok()?))
}

/// Single spaces, no stray separators at the ends.
fn tidy(s: &str) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    t.trim_matches(|c: char| c == '-' || c == '_' || c == ',' || c.is_whitespace()).to_string()
}

fn build_rule(number: &str, title: &str, body: &[(usize, String)]) -> std::result::Result<Rule, String> {
    let (mut target, mut pattern, mut kind, mut sets, mut then, mut on, mut note, mut ignore, mut case) =
        (None, None, None, Vec::new(), Then::Stop, true, String::new(), false, false);
    for (line, text) in body {
        if text.eq_ignore_ascii_case("off") {
            on = false;
            continue;
        }
        let Some((k, v)) = text.split_once(':') else {
            // A wrapped note line.
            if !note.is_empty() {
                note.push(' ');
                note.push_str(text.trim());
                continue;
            }
            return Err(format!("line {line} should be \"key: value\""));
        };
        let v = v.trim();
        match k.trim().to_lowercase().as_str() {
            "applies to" => target = Some(parse_target(v).ok_or_else(|| format!("line {line}: \"applies to\" should be top folder, folder, folder at level N or file name, optionally with \"when …\""))?),
            "pattern" => pattern = Some(strip_ticks(v).to_string()),
            "kind" => kind = Some(kind_word(v)),
            "sets" => {
                for part in v.split(';') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let (f, t) = part.split_once('=').ok_or_else(|| format!("line {line}: \"{part}\" should be \"field = value\""))?;
                    let f = f.trim().to_lowercase().replace("photo by", "photo_by");
                    if !FIELDS.contains(&f.as_str()) {
                        return Err(format!("line {line}: unknown field \"{f}\" (fields: {})", FIELDS.join(", ")));
                    }
                    sets.push((f, t.trim().to_string()));
                }
            }
            "then" => {
                then = match v.to_lowercase().as_str() {
                    "stop" => Then::Stop,
                    "continue" | "keep going" => Then::Continue,
                    "remove" | "remove and continue" => Then::Remove,
                    _ => return Err(format!("line {line}: \"then\" should be stop, continue or remove")),
                }
            }
            "on" => on = yes(v),
            "case" => case = v.eq_ignore_ascii_case("ignore") || v.eq_ignore_ascii_case("ignored"),
            "photo date" => ignore = v.eq_ignore_ascii_case("ignore") || v.eq_ignore_ascii_case("ignored"),
            "note" => note = v.to_string(),
            other => return Err(format!("line {line}: unknown key \"{other}\"")),
        }
    }
    let (target, conditions) = target.ok_or("it needs \"applies to\"")?;
    let pattern = pattern.ok_or("it needs \"pattern\"")?;
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(case)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("the pattern can't be read: {}", e.to_string().lines().last().unwrap_or("")))?;
    Ok(Rule { number: number.to_string(), title: title.to_string(), target, conditions, pattern, regex, kind, sets, then, ignore_photo_date: ignore, on, note })
}

// ------------------------------------------------------------ templates

/// Fill in `{group}`, `{folder}`, `{parent}`, `{top}` and `{file}`, each
/// optionally through filters: `{subject|leading names|names}`.
fn render(template: &str, c: &Captures, ctx: &Places, set: &RuleSet) -> String {
    static VAR: OnceLock<Regex> = OnceLock::new();
    let var = VAR.get_or_init(|| Regex::new(r"\{([^{}|]+)((?:\|[^{}|]+)*)\}").expect("valid"));
    var.replace_all(template, |m: &Captures| {
        let name = m[1].trim();
        let mut v = match name {
            "folder" => ctx.folder.to_string(),
            "folder memory" => ctx.folder_memory.to_string(),
            "parent" => ctx.parent.to_string(),
            "top" => ctx.top.to_string(),
            "file" => ctx.file.to_string(),
            "photo day" => return "{photo day}".to_string(),
            g => c.name(g).map(|x| x.as_str().to_string()).unwrap_or_default(),
        };
        for f in m[2].split('|').map(str::trim).filter(|f| !f.is_empty()) {
            v = filter(f, &v, set);
        }
        v
    })
    .into_owned()
}

fn filter(name: &str, v: &str, set: &RuleSet) -> String {
    match name {
        "names" => split_names(v).join("; "),
        "leading names" => leading_names(v),
        "family" => {
            let n = split_names(v);
            match n.as_slice() {
                [] => String::new(),
                [one] => format!("{one}'s family"),
                _ => format!("{} family", tidy(v)),
            }
        }
        "date" => text_date(v, set.day01_unknown).unwrap_or_default(),
        "clean" => clean_text(v),
        "number" => {
            let t = v.trim().trim_start_matches('0');
            if t.is_empty() && !v.trim().is_empty() { "0".into() } else { t.to_string() }
        }
        "words" => tidy(&v.replace(['_', '-'], " ")),
        "lower" => v.to_lowercase(),
        "upper" => v.to_uppercase(),
        _ => v.to_string(),
    }
}

/// "June, Ellen and Walter Rowan" → June Rowan; Ellen Rowan; Walter Rowan:
/// a closing surname (the last name has two or more words and the others
/// one) is given to every name.
pub fn split_names(s: &str) -> Vec<String> {
    static SEP: OnceLock<Regex> = OnceLock::new();
    let sep = SEP.get_or_init(|| Regex::new(r"(?i)\s*(?:,|;|&|\+|\band\b)\s*").expect("valid"));
    let parts: Vec<String> = sep.split(s).map(tidy).filter(|p| !p.is_empty()).collect();
    if parts.len() > 1 {
        let last: Vec<&str> = parts[parts.len() - 1].split_whitespace().collect();
        if last.len() >= 2 && parts[..parts.len() - 1].iter().all(|p| p.split_whitespace().count() == 1) {
            let surname = last[last.len() - 1];
            return parts.iter().enumerate().map(|(i, p)| if i == parts.len() - 1 { p.clone() } else { format!("{p} {surname}") }).collect();
        }
    }
    parts
}

/// The list of names a description starts with: capitalized words joined
/// by `,`, `and` or `&` ("June Rowan 11th birthday" → "June Rowan").
pub fn leading_names(s: &str) -> String {
    static LEAD: OnceLock<Regex> = OnceLock::new();
    let lead = LEAD.get_or_init(|| {
        Regex::new(r"^\s*((?:[A-Z](?:[^\W\d_]|['’])+\.?)(?:(?:\s+|\s*,\s*|\s*&\s*|\s+and\s+|,\s*and\s+)(?:[A-Z](?:[^\W\d_]|['’])*\.?))*)").expect("valid")
    });
    let found = lead.captures(s).map(|c| tidy(&c[1])).unwrap_or_default();
    // Stop at a month ("Christmas Oct1976" names nobody called Oct).
    let words: Vec<&str> = found.split_whitespace().take_while(|w| !MONTHS.iter().any(|m| w.to_lowercase().trim_end_matches('.').starts_with(m) && w.len() <= 9 && crate::dates::parse(&format!("{w} 2000")).ok().flatten().is_some())).collect();
    tidy(&words.join(" "))
}

/// Text without dates, series numbers, brackets and underscores, for a title.
pub fn clean_text(s: &str) -> String {
    static DATES: OnceLock<Regex> = OnceLock::new();
    let r = DATES.get_or_init(|| {
        Regex::new(r"(?i)\[[^\]]*\]|\b\d{1,2}[a-z]{3}\d{4}\b|\b[a-z]{3}\d{4}\b|\b(?:18|19|20)(?:\d\d|\d0'?s|__)\b|_\d{2,}$|\(\d+(?:of\d+)?\)|\s-\s*\d+$").expect("valid")
    });
    tidy(&r.replace_all(s, " ").replace('_', " "))
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// A date as found in a rule: `20140202`, `199203`, `2014-02-02`,
/// `1961 to 1970`, `8JUL1991`, `Oct1976`, `1960's`, `19__` (blank: year
/// unknown on purpose), or anything [`dates::parse`] reads. With
/// `day01_unknown`, `19920301` means March 1992 and `19920101` means 1992.
pub fn read_date(v: &str, day01_unknown: bool) -> Option<String> {
    let v = v.trim().trim_matches(['[', ']']);
    let ok = |d: String| dates::parse(&d).ok().flatten().map(|_| d);
    static EIGHT: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = EIGHT.get_or_init(|| Regex::new(r"^(\d{4})-?(\d{2})-?(\d{2})$").expect("valid")).captures(v) {
        let (y, m, d) = (&c[1], &c[2], &c[3]);
        if day01_unknown && d == "01" {
            return if m == "01" { ok(y.to_string()) } else { ok(format!("{y}-{m}")) };
        }
        if m == "00" {
            return ok(y.to_string());
        }
        if d == "00" {
            return ok(format!("{y}-{m}"));
        }
        return ok(format!("{y}-{m}-{d}"));
    }
    static SIX: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = SIX.get_or_init(|| Regex::new(r"^(\d{4})-?(\d{2})$").expect("valid")).captures(v) {
        return ok(format!("{}-{}", &c[1], &c[2]));
    }
    if v.contains("__") {
        return None;
    }
    ok(v.to_string()).or_else(|| text_date(v, day01_unknown))
}

/// The first date written in some text: `[30JAN1957]`, `8JUL1991`,
/// `Oct1976`, `[1957]`, a decade (`1960's`), or a year. `19__` means the
/// year is unknown on purpose and gives nothing.
pub fn text_date(s: &str, day01_unknown: bool) -> Option<String> {
    let _ = day01_unknown;
    let ok = |d: String| dates::parse(&d).ok().flatten().map(|_| d);
    let month = |m: &str| MONTHS.iter().position(|x| m.to_lowercase().starts_with(x)).map(|i| i + 1);
    static DMY: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = DMY.get_or_init(|| Regex::new(r"(?i)\b(\d{1,2})\s*([a-z]{3})[a-z]*\.?\s*((?:18|19|20)\d\d)\b").expect("valid")).captures(s) {
        if let Some(m) = month(&c[2]) {
            let d: u32 = c[1].parse().ok()?;
            if let Some(x) = ok(format!("{}-{m:02}-{d:02}", &c[3])) {
                return Some(x);
            }
        }
    }
    static MY: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = MY.get_or_init(|| Regex::new(r"(?i)\b([a-z]{3,9})\.?\s*((?:18|19|20)\d\d)\b").expect("valid")).captures(s) {
        if let Some(m) = month(&c[1]).filter(|_| c[1].len() == 3 || crate::dates::parse(&format!("{} 2000", &c[1])).ok().flatten().is_some()) {
            return ok(format!("{}-{m:02}", &c[2]));
        }
    }
    static DECADE: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = DECADE.get_or_init(|| Regex::new(r"\b((?:18|19|20)\d0)'?s\b").expect("valid")).captures(s) {
        return ok(format!("{}s", &c[1]));
    }
    static UNKNOWN: OnceLock<Regex> = OnceLock::new();
    if UNKNOWN.get_or_init(|| Regex::new(r"\b(?:18|19|20)__").expect("valid")).is_match(s) {
        return None;
    }
    static YEAR: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = YEAR.get_or_init(|| Regex::new(r"(?:^|[^\d])((?:18|19|20)\d\d)(?:[^\d]|$)").expect("valid")).captures(s) {
        return ok(c[1].to_string());
    }
    None
}

// ------------------------------------------------------------ coverage

/// How many files each rule caught, and the files no rule caught.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Coverage {
    /// The rule set's name and where it came from.
    pub name: String,
    pub source: String,
    pub files: usize,
    /// (rule label, files where it named the memory, files it matched at all).
    pub rules: Vec<(String, usize, usize)>,
    /// Files no rule gave a memory, with their paths.
    pub uncaught: Vec<String>,
    /// Names found in file names that are not confirmed yet, with how many files.
    pub to_confirm: Vec<(String, usize)>,
    /// Lines of the rules file that couldn't be read.
    pub problems: Vec<String>,
}

impl Coverage {
    pub fn new(set: &RuleSet) -> Coverage {
        let mut rules: Vec<(String, usize, usize)> = set.rules.iter().filter(|r| r.on).map(|r| (r.label(), 0, 0)).collect();
        for extra in ["Skip folders", "Initials", "Later photo of an earlier time"] {
            rules.push((extra.to_string(), 0, 0));
        }
        Coverage { name: set.name.clone(), source: set.source.clone(), problems: set.problems.clone(), rules, ..Coverage::default() }
    }

    /// Count one file's outcome.
    pub fn add(&mut self, file: &str, out: &Outcome) {
        self.files += 1;
        for label in &out.rules {
            match self.rules.iter_mut().find(|r| &r.0 == label) {
                Some(r) => r.2 += 1,
                None => self.rules.push((label.clone(), 0, 1)),
            }
        }
        match &out.memory_rule {
            Some(m) if out.fields.contains_key("memory") => {
                if let Some(r) = self.rules.iter_mut().find(|r| &r.0 == m) {
                    r.1 += 1;
                }
            }
            _ => self.uncaught.push(file.to_string()),
        }
        for n in &out.to_confirm {
            match self.to_confirm.iter_mut().find(|t| &t.0 == n) {
                Some(t) => t.1 += 1,
                None => self.to_confirm.push((n.clone(), 1)),
            }
        }
    }

    /// Sorted for showing: names most used first.
    pub fn done(&mut self) {
        self.to_confirm.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }

    /// The report as plain text, to save beside a plan.
    pub fn text(&self) -> String {
        let mut s = format!("IMPORT RULES COVERAGE\r\n\r\nRules: {} ({})\r\nFiles: {}\r\n\r\n", self.name, self.source, self.files);
        s.push_str("Rule                                              Named the memory   Matched\r\n");
        for (label, mem, all) in &self.rules {
            s.push_str(&format!("{label:<50}{mem:>16}{all:>10}\r\n"));
        }
        s.push_str(&format!("\r\nFILES NO RULE CAUGHT ({})\r\n", self.uncaught.len()));
        for f in &self.uncaught {
            s.push_str(&format!("{f}\r\n"));
        }
        s.push_str(&format!("\r\nNAMES TO CONFIRM ({})\r\nAdd the ones that are people to the rules file's Names list.\r\n", self.to_confirm.len()));
        for (n, c) in &self.to_confirm {
            s.push_str(&format!("{n} ({c})\r\n"));
        }
        if !self.problems.is_empty() {
            s.push_str("\r\nLINES OF THE RULES FILE THAT WERE LEFT OUT\r\n");
            for p in &self.problems {
                s.push_str(&format!("{p}\r\n"));
            }
        }
        s
    }
}
