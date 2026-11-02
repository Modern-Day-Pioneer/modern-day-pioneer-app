//! Dates typed as the family knows them.
//!
//! People type "about 1930", "1970s", "June 1956" or "1960 to 1965". The app
//! keeps the date as known in EDTF (Library of Congress Extended Date/Time
//! Format, for example `1930~`, `197X`, `1960/1965`) and the nearest exact
//! date, written in ISO 8601 to the precision known (`1930`, `1956-06`,
//! `1956-06-02`), so ordinary tools can still sort. Anything it cannot read
//! is refused with examples rather than guessed.

use serde::Serialize;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DateAsKnown {
    /// The date in EDTF.
    pub edtf: String,
    /// The nearest exact date, ISO 8601 to the precision known; empty for an
    /// interval with no known start or end.
    pub nearest: Option<String>,
    /// The date in plain words, to show back what was understood.
    pub words: String,
}

const MONTHS: [&str; 12] =
    ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

pub const EXAMPLES: &str = "Try a year (1956), a month and year (June 1956), a full date (June 2, 1956), \"about 1930\", \"maybe 1945\", \"1970s\", \"1960 to 1965\", \"before 1900\" or \"after 1950\".";

fn month_of(word: &str) -> Option<u32> {
    let w = word.trim_end_matches('.').to_lowercase();
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.starts_with(&w)).map(|i| i as u32 + 1)
}

fn days_in(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

fn year_of(s: &str) -> Option<i32> {
    (s.len() == 4 && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

fn ymd(y: i32, m: Option<u32>, d: Option<u32>) -> Option<String> {
    match (m, d) {
        (None, _) => Some(format!("{y:04}")),
        (Some(m), _) if !(1..=12).contains(&m) => None,
        (Some(m), None) => Some(format!("{y:04}-{m:02}")),
        (Some(m), Some(d)) if d >= 1 && d <= days_in(y, m) => Some(format!("{y:04}-{m:02}-{d:02}")),
        _ => None,
    }
}

/// One plain date, no qualifier: a year, a decade, a month and year, or a full date.
fn plain(s: &str) -> Option<String> {
    let s = s.trim().trim_end_matches(',').trim();
    if let Some(y) = year_of(s) {
        return Some(format!("{y:04}"));
    }
    // A decade: 1970s, 1970's.
    let dec = s.trim_end_matches('s').trim_end_matches('\'');
    if s.ends_with('s') && dec.len() == 4 && dec.ends_with('0') && year_of(dec).is_some() {
        return Some(format!("{}X", &dec[..3]));
    }
    // EDTF already: 196X, 1956-06, 1956-06-02.
    if s.len() == 4 && (s.ends_with('X') || s.ends_with('x')) && year_of(&format!("{}0", &s[..3])).is_some() {
        return Some(format!("{}X", &s[..3]));
    }
    let dash: Vec<&str> = s.split('-').collect();
    if dash.len() >= 2 && dash.len() <= 3 && dash.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        let y = year_of(dash[0])?;
        let m = dash[1].parse().ok()?;
        let d = dash.get(2).and_then(|d| d.parse().ok());
        if dash.len() == 3 && d.is_none() {
            return None;
        }
        return ymd(y, Some(m), d);
    }
    // US numeric: 6/2/1956.
    let slash: Vec<&str> = s.split('/').collect();
    if slash.len() == 3 && slash.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        let y = year_of(slash[2])?;
        return ymd(y, slash[0].parse().ok(), slash[1].parse().ok());
    }
    // Words: "June 1956", "June 2, 1956", "2 June 1956".
    let words: Vec<&str> = s.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let num = |w: &str| -> Option<u32> {
        let w = w.trim_end_matches("st").trim_end_matches("nd").trim_end_matches("rd").trim_end_matches("th");
        (w.len() <= 2).then(|| w.parse().ok()).flatten()
    };
    match words.as_slice() {
        [m, y] => ymd(year_of(y)?, Some(month_of(m)?), None),
        [m, d, y] if month_of(m).is_some() => ymd(year_of(y)?, month_of(m), Some(num(d)?)),
        [d, m, y] => ymd(year_of(y)?, Some(month_of(m)?), Some(num(d)?)),
        _ => None,
    }
}

/// The date with an optional "about" (~), "maybe" (?) or both (%).
fn qualified(s: &str) -> Option<String> {
    let mut s = s.trim().to_lowercase();
    let mut about = false;
    let mut maybe = false;
    loop {
        let before = s.clone();
        for p in ["about ", "around ", "circa ", "c. ", "ca. ", "ca ", "c.", "approximately ", "roughly "] {
            if let Some(rest) = s.strip_prefix(p) {
                about = true;
                s = rest.trim().to_string();
            }
        }
        for p in ["maybe ", "probably ", "perhaps ", "possibly "] {
            if let Some(rest) = s.strip_prefix(p) {
                maybe = true;
                s = rest.trim().to_string();
            }
        }
        if s == before {
            break;
        }
    }
    for (q, is_about) in [('?', false), ('~', true), ('%', true)] {
        if let Some(rest) = s.strip_suffix(q) {
            if q == '%' {
                maybe = true;
            }
            if is_about {
                about = true;
            } else {
                maybe = true;
            }
            s = rest.trim().to_string();
        }
    }
    let d = plain(&s)?;
    let mark = match (about, maybe) {
        (true, true) => "%",
        (true, false) => "~",
        (false, true) => "?",
        _ => "",
    };
    if !mark.is_empty() && d.ends_with('X') {
        // EDTF does not qualify a decade; "about the 1970s" stays the 1970s.
        return Some(d);
    }
    Some(format!("{d}{mark}"))
}

/// Read a date typed as known. Empty text gives `Ok(None)`.
pub fn parse(text: &str) -> Result<Option<DateAsKnown>> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let lower = t.to_lowercase().replace(['–', '—'], "-");
    let edtf = if let Some(rest) = lower.strip_prefix("before ") {
        qualified(rest).map(|d| format!("../{d}"))
    } else if let Some(rest) = lower.strip_prefix("after ") {
        qualified(rest).map(|d| format!("{d}/.."))
    } else if let Some(rest) = lower.strip_prefix("between ") {
        rest.split_once(" and ").and_then(|(a, b)| Some(format!("{}/{}", qualified(a)?, qualified(b)?)))
    } else if let Some(d) = qualified(&lower) {
        Some(d)
    } else {
        [" to ", " until ", " - ", "-", "/"].iter().find_map(|sep| {
            let (a, b) = lower.split_once(sep)?;
            let (a, b) = (a.trim(), b.trim());
            if a == ".." {
                return Some(format!("../{}", qualified(b)?));
            }
            if b == ".." {
                return Some(format!("{}/..", qualified(a)?));
            }
            Some(format!("{}/{}", qualified(a)?, qualified(b)?))
        })
    };
    let edtf = edtf.ok_or_else(|| Error::Invalid(format!("The date \"{t}\" isn't one the app can read. {EXAMPLES}")))?;
    if let Some((a, b)) = edtf.split_once('/') {
        if a != ".." && b != ".." && nearest_of(a) > nearest_of(b) {
            return Err(Error::Invalid(format!("In \"{t}\" the first date comes after the second.")));
        }
    }
    Ok(Some(DateAsKnown { nearest: nearest(&edtf), words: words(&edtf), edtf }))
}

fn nearest_of(one: &str) -> String {
    one.trim_end_matches(['~', '?', '%']).replace('X', "0")
}

/// The nearest exact date of an EDTF date: its start, or its end when the
/// start is open.
pub fn nearest(edtf: &str) -> Option<String> {
    match edtf.split_once('/') {
        Some(("..", "..")) => None,
        Some(("..", b)) => Some(nearest_of(b)),
        Some((a, _)) => Some(nearest_of(a)),
        None => Some(nearest_of(edtf)),
    }
}

fn words_one(d: &str) -> String {
    let (core, about, maybe) = match d.chars().last() {
        Some('~') => (&d[..d.len() - 1], true, false),
        Some('?') => (&d[..d.len() - 1], false, true),
        Some('%') => (&d[..d.len() - 1], true, true),
        _ => (d, false, false),
    };
    let base = if core.ends_with('X') {
        format!("the {}0s", &core[..3])
    } else {
        let p: Vec<&str> = core.split('-').collect();
        let month = |m: &str| m.parse::<usize>().ok().and_then(|m| MONTHS.get(m.wrapping_sub(1))).map(|m| {
            let mut c = m.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        });
        match p.as_slice() {
            [y, m, d] => format!("{} {}, {y}", month(m).unwrap_or_default(), d.trim_start_matches('0')),
            [y, m] => format!("{} {y}", month(m).unwrap_or_default()),
            _ => core.to_string(),
        }
    };
    match (about, maybe) {
        (true, true) => format!("maybe about {base}"),
        (true, false) => format!("about {base}"),
        (false, true) => format!("maybe {base}"),
        _ => base,
    }
}

/// An EDTF date in plain American English words.
pub fn words(edtf: &str) -> String {
    match edtf.split_once('/') {
        Some(("..", b)) => format!("before {}", words_one(b)),
        Some((a, "..")) => format!("after {}", words_one(a)),
        Some((a, b)) => format!("{} to {}", words_one(a), words_one(b)),
        None => words_one(edtf),
    }
}

/// The first year in an EDTF date, for the living rule and short labels.
pub fn first_year(edtf: &str) -> Option<i32> {
    let n = nearest(edtf)?;
    n.get(..4)?.parse().ok()
}
