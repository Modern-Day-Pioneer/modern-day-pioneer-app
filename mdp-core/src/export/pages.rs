//! The exported pages. Every page is made from the catalog by the fixed templates
//! below; nothing on a page is written by hand or by an AI service.
//!
//! Which pages exist, and where each memory is shown in full, follow fixed
//! rules: one page per story, person and place,
//! people grouped by Family; a memory is shown in full in its story, else on
//! its first person's page, else on its place's page, else under "Assorted
//! memories". The timeline lists events and dated memories in date order.
//! Pages are plain HTML and CSS with no script, so they read with
//! JavaScript off and open from a folder with no internet. The one
//! exception is the search page (sprint 13): its small script reads
//! `pages/search.js`, and without scripts it points to the plain lists.
//!
//! The front door is the bookshelf or, when the Journey's setting
//! says so, the coffee table (sprint 13). "In memory of" pages
//! (stories of the kind "tribute") have a page each under
//! `pages/tributes/`, linked from the person's page and the front door.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde_json::Value;

use super::{index, slug, Privacy, ABOUT_FILE, FRONT};
use crate::{dates, Result};

const CSS: &str = include_str!("style.css");

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn st<'a>(o: &'a Value, k: &str) -> &'a str {
    o.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

fn ids(o: &Value, k: &str) -> Vec<String> {
    o.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The ID part of `mdpid:k7f3m2qa`.
fn pid(o: &Value) -> String {
    st(o, "@id").trim_start_matches("mdpid:").to_string()
}

/// A link from the page at `from` to `to`, both relative to the export's top.
pub fn rel(from: &str, to: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').collect::<Vec<_>>().split_last().map(|(_, d)| d.to_vec()).unwrap_or_default();
    let (to_path, frag) = match to.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (to, None),
    };
    let to_parts: Vec<&str> = to_path.split('/').collect();
    let common = from_dir.iter().zip(&to_parts).take_while(|(a, b)| a == b).count().min(to_parts.len().saturating_sub(1));
    let mut out: Vec<String> = std::iter::repeat("..".to_string()).take(from_dir.len() - common).collect();
    out.extend(to_parts[common..].iter().map(|s| s.to_string()));
    let mut r = out.join("/").replace(' ', "%20");
    if let Some(f) = frag {
        r.push('#');
        r.push_str(f);
    }
    r
}

/// A date as known, in words (American English).
fn when(o: &Value, edtf_key: &str, exact_key: &str) -> String {
    let e = st(o, edtf_key);
    if !e.is_empty() {
        return dates::words(e);
    }
    let x = st(o, exact_key);
    if x.is_empty() { String::new() } else { dates::words(x) }
}

/// The nearest exact date, for sorting.
fn sort_date(o: &Value, edtf_key: &str, exact_key: &str) -> Option<String> {
    let x = st(o, exact_key);
    if !x.is_empty() {
        return Some(x.to_string());
    }
    dates::nearest(st(o, edtf_key)).filter(|d| !d.is_empty())
}

fn life(p: &Value) -> String {
    if p.get("living").is_some() {
        return String::new();
    }
    let b = when(p, "birthDateEdtf", "birthDate");
    let d = when(p, "deathDateEdtf", "deathDate");
    match (b.is_empty(), d.is_empty()) {
        (false, false) => format!("{b} to {d}"),
        (true, false) => format!("died {d}"),
        (false, true) => format!("born {b}"),
        _ => String::new(),
    }
}

fn paras(t: &str) -> String {
    t.replace("\r\n", "\n")
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty() && !p.chars().all(|c| c == '=' || c == '-'))
        .map(|p| format!("<p>{}</p>", esc(p)))
        .collect()
}

struct Site<'a> {
    cat: &'a Value,
    g: BTreeMap<String, Value>,
    media: &'a dyn Fn(&str) -> String,
    text: &'a dyn Fn(&str) -> Option<String>,
    privacy: Privacy,
    note: Option<&'a str>,
    fam_dir: HashMap<String, String>,
    fams_of: HashMap<String, Vec<String>>,
    in_story: HashMap<String, String>,
    linked: HashMap<(String, String), String>,
    out: &'a Path,
    pages: usize,
}

const NAV: &[(&str, &str)] = &[
    (FRONT, "Bookshelf"),
    ("pages/search.html", "Search"),
    ("pages/people.html", "People"),
    ("pages/places.html", "Places"),
    ("pages/timeline.html", "Timeline"),
    ("pages/assorted-memories.html", "Assorted memories"),
    ("pages/about.html", "About"),
];

impl<'a> Site<'a> {
    fn of(&self, ty: &str) -> Vec<&'a Value> {
        let cat: &'a Value = self.cat;
        cat["@graph"].as_array().map(|g| g.iter().filter(|o| st(o, "@type") == ty).collect()).unwrap_or_default()
    }
    fn stories(&self) -> Vec<&'a Value> {
        self.of("CreativeWork").into_iter().filter(|o| st(o, "genre") == "story").collect()
    }
    /// "In memory of" pages.
    fn tributes(&self) -> Vec<&'a Value> {
        self.of("CreativeWork").into_iter().filter(|o| st(o, "genre") == "tribute").collect()
    }
    fn tribute_url(&self, s: &Value) -> String {
        format!("pages/tributes/{}-{}.html", slug(st(s, "name"), 40), pid(s))
    }
    /// The first picture in a story's memories, as an `<img>` for `from`.
    fn cover_of(&self, from: &str, s: &Value, alt: &str) -> String {
        ids(s, "memories")
            .iter()
            .filter_map(|m| self.get(m))
            .flat_map(|m| {
                let cover = st(m, "cover").to_string();
                std::iter::once(cover).chain(ids(m, "contents"))
            })
            .filter_map(|c| self.get(&c))
            .find(|c| st(c, "@type") == "ImageObject")
            .and_then(|c| self.shown(c))
            .map(|e| format!("<img src=\"{}\" alt=\"{}\">", self.src(from, st(e, "contentUrl")), esc(alt)))
            .unwrap_or_default()
    }
    fn get(&self, id: &str) -> Option<&Value> {
        self.g.get(id)
    }

    fn person_url(&self, p: &Value, fam: Option<&str>) -> String {
        let fam = fam.map(str::to_string).or_else(|| self.fams_of.get(st(p, "@id")).and_then(|f| f.first().cloned()));
        let dir = fam.and_then(|f| self.fam_dir.get(&f).cloned()).unwrap_or_else(|| "pages/people".into());
        format!("{dir}/{}-{}.html", slug(st(p, "name"), 40), pid(p))
    }
    fn story_url(&self, s: &Value) -> String {
        format!("pages/stories/{}-{}.html", slug(st(s, "name"), 40), pid(s))
    }
    fn place_url(&self, p: &Value) -> String {
        format!("pages/places/{}-{}.html", slug(st(p, "name"), 40), pid(p))
    }
    fn family_url(&self, f: &Value) -> String {
        format!("pages/people.html#{}", self.fam_anchor(f))
    }
    fn fam_anchor(&self, f: &Value) -> String {
        self.fam_dir.get(st(f, "@id")).map(|d| d.trim_start_matches("pages/").to_string()).unwrap_or_else(|| slug(st(f, "name"), 40))
    }

    /// Where a memory is shown in full: its story, else its first person's
    /// page, else its place, else "Assorted memories".
    fn home_of(&self, m: &Value) -> String {
        if let Some(s) = self.in_story.get(st(m, "@id")).and_then(|s| self.get(s)) {
            return self.story_url(s);
        }
        for l in m.get("people").and_then(|p| p.as_array()).into_iter().flatten() {
            if let Some(who) = self.get(st(l, "individual")) {
                if st(who, "@type") == "Person" {
                    return self.person_url(who, None);
                }
            }
        }
        if let Some(p) = self.get(st(m, "contentLocation")) {
            return self.place_url(p);
        }
        "pages/assorted-memories.html".into()
    }

    fn a(&self, from: &str, to: &str, text: &str) -> String {
        format!("<a href=\"{}\">{}</a>", esc(&rel(from, to)), esc(text))
    }

    fn who_link(&self, from: &str, o: &Value) -> String {
        match st(o, "@type") {
            "Person" => self.a(from, &self.person_url(o, None), st(o, "name")),
            "Family" => self.a(from, &self.family_url(o), st(o, "name")),
            _ => {
                let sp = st(o, "species");
                format!("{} <span class=\"muted\">({})</span>", esc(st(o, "name")), esc(if sp.is_empty() { "pet" } else { sp }))
            }
        }
    }

    /// A person's or pet's portrait (sprint 8), small in lists or larger on their page.
    fn portrait(&self, from: &str, p: &Value, cls: &str) -> String {
        let Some(pic) = self.get(st(p, "image").trim_start_matches("mdpid:")).or_else(|| self.get(st(p, "image"))) else { return String::new() };
        let Some(show) = self.shown(pic) else { return String::new() };
        format!("<img class=\"{cls}\" src=\"{}\" alt=\"{}\">", self.src(from, st(show, "contentUrl")), esc(&format!("Portrait of {}", st(p, "name"))))
    }

    fn src(&self, from: &str, url: &str) -> String {
        esc(&rel(from, &(self.media)(url)))
    }

    /// The file a browser shows: the export-format copy where there is one.
    fn shown<'b>(&self, c: &'b Value) -> Option<&'b Value> {
        let enc = c.get("encoding")?.as_array()?;
        let prefs: &[&str] = match st(c, "@type") {
            "ImageObject" => &["image/jpeg", "image/png", "image/gif", "image/webp"],
            "AudioObject" => &["audio/mpeg", "audio/mp4", "audio/wav", "audio/flac", "audio/ogg"],
            "VideoObject" => &["video/mp4"],
            _ => &["text/plain"],
        };
        prefs.iter().find_map(|p| enc.iter().find(|e| st(e, "encodingFormat") == *p)).or_else(|| enc.first())
    }

    fn original<'b>(&self, c: &'b Value) -> Option<&'b Value> {
        let enc = c.get("encoding")?.as_array()?;
        enc.iter().find(|e| e.get("copy") != Some(&Value::Bool(true))).or_else(|| enc.first())
    }

    fn transcript_html(&self, from: &str, c: &Value) -> String {
        let Some(tr) = self.linked.get(&("transcriptOf".into(), st(c, "@id").to_string())).and_then(|t| self.get(t)) else {
            return String::new();
        };
        let state = if st(tr, "reviewStatus") == "automatic" { "automatic transcript, not yet checked by a person" } else { "transcript, checked" };
        let by = st(tr, "transcribedBy");
        let body = (self.text)(&pid(tr)).map(|t| paras(&t)).unwrap_or_default();
        let file = tr
            .get("encoding")
            .and_then(|e| e.as_array())
            .and_then(|e| e.iter().find(|x| st(x, "encodingFormat") == "text/plain"))
            .map(|e| format!("<p class=\"credit\"><a href=\"{}\">Transcript as a text file</a></p>", self.src(from, st(e, "contentUrl"))))
            .unwrap_or_default();
        format!(
            "<details class=\"transcript\" open><summary>Transcript <span class=\"muted\">({}{})</span></summary>{body}{file}</details>",
            esc(state),
            if by.is_empty() { String::new() } else { format!("; {}", esc(by)) }
        )
    }

    fn content_block(&self, from: &str, c: &Value) -> String {
        let ty = st(c, "@type");
        if !st(c, "transcriptOf").is_empty() || !st(c, "stillFrameOf").is_empty() {
            return String::new(); // shown with the item it belongs to
        }
        let credit: Vec<&str> = [st(c, "creditText"), st(c, "copyrightNotice")].into_iter().filter(|x| !x.is_empty()).collect();
        let credit = credit.iter().map(|x| esc(x)).collect::<Vec<_>>().join(" · ");
        let orig = self
            .original(c)
            .map(|o| format!("{}{}<a href=\"{}\">Original file</a>", credit, if credit.is_empty() { "" } else { " · " }, self.src(from, st(o, "contentUrl"))))
            .unwrap_or(credit);
        let mut notes = String::new();
        for (k, label) in [("peopleNote", "Who is where"), ("speakerNote", "Who speaks when")] {
            if !st(c, k).is_empty() {
                notes.push_str(&format!("<p class=\"note\">{label}: {}</p>", esc(st(c, k))));
            }
        }
        let w = when(c, "dateEdtf", "dateCreated");
        let cap = format!(
            "<figcaption><b>{}</b>{}{}{notes}<p class=\"credit\">{orig}</p></figcaption>",
            esc(st(c, "name")),
            if w.is_empty() { String::new() } else { format!(" <span class=\"muted\">{}</span>", esc(&w)) },
            if st(c, "description").is_empty() { String::new() } else { format!("<span>{}</span>", esc(st(c, "description"))) },
        );
        let Some(show) = self.shown(c) else { return String::new() };
        let src = self.src(from, st(show, "contentUrl"));
        match ty {
            "ImageObject" => {
                let alt = if st(c, "description").is_empty() { st(c, "name") } else { st(c, "description") };
                let size = match (c.get("width").and_then(|v| v.as_i64()), c.get("height").and_then(|v| v.as_i64())) {
                    (Some(w), Some(h)) => format!(" width=\"{w}\" height=\"{h}\""),
                    _ => String::new(),
                };
                format!("<figure class=\"c-pic\"><img src=\"{src}\" alt=\"{}\"{size} loading=\"lazy\">{cap}</figure>", esc(alt))
            }
            "AudioObject" => {
                // A media fragment skips a quiet start; the file stays whole.
                let start = c.get("mdp:playFrom").and_then(|v| v.as_f64()).map(|t| format!("#t={t}")).unwrap_or_default();
                format!(
                    "<figure class=\"c-audio\"><audio controls preload=\"none\" src=\"{src}{start}\"></audio>{cap}{}</figure>",
                    self.transcript_html(from, c)
                )
            }
            "VideoObject" => {
                let poster = self
                    .linked
                    .get(&("stillFrameOf".into(), st(c, "@id").to_string()))
                    .and_then(|s| self.get(s))
                    .and_then(|s| self.shown(s))
                    .map(|e| format!(" poster=\"{}\"", self.src(from, st(e, "contentUrl"))))
                    .unwrap_or_default();
                format!(
                    "<figure class=\"c-video\"><video controls preload=\"none\"{poster} src=\"{src}\"></video>{cap}{}</figure>",
                    self.transcript_html(from, c)
                )
            }
            _ => {
                let body = (self.text)(&pid(c)).map(|t| paras(&t)).unwrap_or_default();
                // A document (PDF, Word) shows pictures of its pages, then its text.
                let pages: Vec<String> = c
                    .get("encoding")
                    .and_then(|e| e.as_array())
                    .map(|e| e.iter().filter(|x| x.get("copy") == Some(&Value::Bool(true)) && st(x, "encodingFormat") == "image/jpeg").collect::<Vec<_>>())
                    .unwrap_or_default()
                    .iter()
                    .enumerate()
                    .map(|(n, x)| format!("<img src=\"{}\" alt=\"{}\" loading=\"lazy\">", self.src(from, st(x, "contentUrl")), esc(&format!("Page {} of {}", n + 1, st(c, "name")))))
                    .collect();
                if pages.is_empty() {
                    format!("<figure class=\"c-text\"><div class=\"prose\">{body}</div>{cap}</figure>")
                } else {
                    let text = if body.is_empty() { String::new() } else { format!("<details class=\"transcript\"><summary>Text of the document</summary><div class=\"prose\">{body}</div></details>") };
                    format!("<figure class=\"c-doc\"><div class=\"pages\">{}</div>{text}{cap}</figure>", pages.join(""))
                }
            }
        }
    }

    fn role_words(l: &Value) -> String {
        let r = if st(l, "role") == "other" { st(l, "roleNote") } else { st(l, "role") };
        format!("{r}{}", if st(l, "certainty") == "probably" { " (probably)" } else { "" })
    }

    fn memory_card(&self, from: &str, m: &Value, extra: &str) -> String {
        format!(
            "<li class=\"mem-card\"><a href=\"{}\"><b>{}</b></a>{extra}<span class=\"muted\">{}</span></li>",
            esc(&rel(from, &format!("{}#m-{}", self.home_of(m), pid(m)))),
            esc(st(m, "name")),
            esc(&when(m, "dateEdtf", "temporalCoverage"))
        )
    }

    fn memory_block(&self, from: &str, m: &Value) -> String {
        let mut bits = Vec::new();
        let w = when(m, "dateEdtf", "temporalCoverage");
        if !w.is_empty() {
            bits.push(esc(&w));
        }
        if let Some(p) = self.get(st(m, "contentLocation")) {
            bits.push(self.a(from, &self.place_url(p), st(p, "name")));
        }
        if let Some(e) = self.get(st(m, "event")) {
            bits.push(esc(st(e, "name")));
        }
        let ppl: String = m
            .get("people")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let who = self.get(st(l, "individual"))?;
                Some(format!("<li>{} <span class=\"role\">{}</span></li>", self.who_link(from, who), esc(&Self::role_words(l))))
            })
            .collect();
        let items: String = ids(m, "contents").iter().filter_map(|c| self.get(c)).map(|c| self.content_block(from, c)).collect();
        format!(
            "<article class=\"memory\" id=\"m-{}\"><header><h2>{}</h2><p class=\"meta\">{}</p></header>{}{}<div class=\"contents\">{items}</div></article>",
            pid(m),
            esc(st(m, "name")),
            bits.join(" · "),
            if st(m, "description").is_empty() { String::new() } else { format!("<p class=\"caption\">{}</p>", esc(st(m, "description"))) },
            if ppl.is_empty() { String::new() } else { format!("<ul class=\"people\" aria-label=\"People\">{ppl}</ul>") },
        )
    }

    fn page(&mut self, path: &str, title: &str, body: &str, cls: &str) -> Result<()> {
        let journey = st(self.cat, "name");
        let nav: String = NAV
            .iter()
            .map(|(h, t)| {
                let t = if *h == FRONT && self.privacy.coffee_table { "Coffee table" } else { t };
                format!("<a href=\"{}\"{}>{t}</a>", esc(&rel(path, h)), if *h == path { " aria-current=\"page\"" } else { "" })
            })
            .collect();
        let full_title = if path == FRONT { esc(title) } else { format!("{} · {}", esc(title), esc(journey)) };
        let banner = self
            .note
            .map(|n| format!("<p class=\"pv-banner\" role=\"note\"><b>Preview.</b> This is how the export will look. {}</p>", esc(n)))
            .unwrap_or_default();
        let doc = format!(
            "<!doctype html>\n<html lang=\"{lang}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
<title>{full_title}</title><link rel=\"stylesheet\" href=\"{css}\"></head>\n\
<body class=\"{cls}\"><a class=\"skip\" href=\"#main\">Skip to the page</a>\n\
<header class=\"top\"><a class=\"brand\" href=\"{home}\">{j}</a><nav aria-label=\"Sections\">{nav}</nav></header>\n\
<main id=\"main\">{banner}{body}</main>\n\
<footer class=\"foot\">A family history made with Modern Day Pioneer. Catalog format {v}, last changed {d}. Works offline; no internet needed.</footer>\n\
</body></html>\n",
            lang = esc(if st(self.cat, "inLanguage").is_empty() { "en" } else { st(self.cat, "inLanguage") }),
            css = esc(&rel(path, "pages/style.css")),
            home = esc(&rel(path, FRONT)),
            j = esc(journey),
            v = esc(st(self.cat, "formatVersion")),
            d = esc(st(self.cat, "dateModified")),
        );
        let full = path.split('/').fold(self.out.to_path_buf(), |p, s| p.join(s));
        if let Some(d) = full.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(full, doc)?;
        self.pages += 1;
        Ok(())
    }
}

/// Write the catalog files, the guide and every page into `out`. `media`
/// maps a file's export name to where the page should point (the export
/// itself, or the working file for the preview); `text` gives a text item's
/// words by ID. Returns the number of pages.
pub fn render(
    cat: &Value,
    out: &Path,
    media: &dyn Fn(&str) -> String,
    text: &dyn Fn(&str) -> Option<String>,
    privacy: Privacy,
    note: Option<&str>,
) -> Result<usize> {
    std::fs::create_dir_all(out.join("pages"))?;
    let json_err = |e: serde_json::Error| crate::Error::Invalid(e.to_string());
    std::fs::write(out.join("catalog.json"), serde_json::to_string_pretty(cat).map_err(json_err)? + "\n")?;
    std::fs::write(out.join("catalog.js"), format!("window.CATALOG = {};\n", serde_json::to_string(cat).map_err(json_err)?))?;
    std::fs::write(out.join(crate::jsonl::CONTEXT_FILE), crate::jsonl::CONTEXT)?;
    std::fs::write(out.join("pages").join("style.css"), CSS)?;

    let mut site = Site {
        cat,
        g: index(cat),
        media,
        text,
        privacy,
        note,
        fam_dir: HashMap::new(),
        fams_of: HashMap::new(),
        in_story: HashMap::new(),
        linked: HashMap::new(),
        out,
        pages: 0,
    };
    // Where each family's people live: pages/<family>/, kept unique.
    let mut taken: Vec<String> = ["stories", "places", "people"].iter().map(|s| s.to_string()).collect();
    for f in site.of("Family") {
        let mut d = slug(st(f, "name"), 40);
        if taken.contains(&d) {
            d = format!("{d}-{}", pid(f));
        }
        taken.push(d.clone());
        site.fam_dir.insert(st(f, "@id").to_string(), format!("pages/{d}"));
        for m in ids(f, "member") {
            site.fams_of.entry(m).or_default().push(st(f, "@id").to_string());
        }
    }
    for s in site.stories() {
        for m in ids(s, "memories") {
            site.in_story.entry(m).or_insert_with(|| st(s, "@id").to_string());
        }
    }
    for o in cat["@graph"].as_array().into_iter().flatten() {
        for k in ["transcriptOf", "stillFrameOf"] {
            if !st(o, k).is_empty() {
                site.linked.entry((k.into(), st(o, k).to_string())).or_insert_with(|| st(o, "@id").to_string());
            }
        }
    }
    let stories: Vec<Value> = site.stories().into_iter().cloned().collect();
    let memories: Vec<Value> = site.of("Memory").into_iter().cloned().collect();
    let families: Vec<Value> = site.of("Family").into_iter().cloned().collect();
    let places: Vec<Value> = site.of("Place").into_iter().cloned().collect();
    let events: Vec<Value> = site.of("Event").into_iter().cloned().collect();
    let people: Vec<Value> = site.of("Person").into_iter().cloned().collect();
    let tributes: Vec<Value> = site.tributes().into_iter().cloned().collect();
    let pets: Vec<Value> = site.of("Pet").into_iter().cloned().collect();
    let assorted: Vec<&Value> = memories.iter().filter(|m| site.home_of(m) == "pages/assorted-memories.html").collect();

    // ---------- front door: the bookshelf
    let featured: Vec<String> = ids(cat, "featured");
    let face = stories.iter().find(|s| featured.iter().any(|f| f == st(s, "@id"))).map(|s| st(s, "@id").to_string());
    let book = |href: &str, title: &str, sub: &str, n: usize, face: bool| {
        format!(
            "<a class=\"book{}\" style=\"--c:var(--b{})\" href=\"{}\"><span class=\"t\">{}</span><span class=\"s\">{}</span></a>",
            if face { " face" } else { "" },
            n % 6,
            esc(&rel(FRONT, href)),
            esc(title),
            esc(sub)
        )
    };
    let mut ordered: Vec<&Value> = stories.iter().collect();
    ordered.sort_by_key(|s| Some(st(s, "@id").to_string()) != face);
    let shelf_s: String = ordered
        .iter()
        .enumerate()
        .map(|(n, s)| {
            let c = ids(s, "memories").len();
            book(&site.story_url(s), st(s, "name"), &format!("{c} {}", if c == 1 { "memory" } else { "memories" }), n, Some(st(s, "@id").to_string()) == face)
        })
        .collect();
    let shelf_t: String = tributes
        .iter()
        .enumerate()
        .map(|(n, t)| {
            let about = site.get(st(t, "focus")).map(|f| st(f, "name").to_string()).unwrap_or_default();
            book(&site.tribute_url(t), st(t, "name"), &about, n + 2, false)
        })
        .collect();
    let shelf_f: String = families
        .iter()
        .enumerate()
        .map(|(n, f)| book(&site.family_url(f), st(f, "name"), &format!("{} in the family", ids(f, "member").len()), n + 3, false))
        .collect();
    let refs = [
        ("pages/people.html", "People", "by family".to_string()),
        ("pages/places.html", "Places", format!("{} {}", places.len(), if places.len() == 1 { "place" } else { "places" })),
        ("pages/timeline.html", "Timeline", "in date order".to_string()),
        ("pages/assorted-memories.html", "Assorted memories", format!("{} more", assorted.len())),
        ("pages/about.html", "About this folder", "how it works".to_string()),
    ];
    let shelf_r: String = refs.iter().enumerate().map(|(n, (h, t, s))| book(h, t, s, n + 1, false)).collect();
    let mut shelves = String::new();
    for (n, (h, b)) in [("Stories", shelf_s), ("In memory of", shelf_t), ("Families", shelf_f), ("Reference", shelf_r)].into_iter().enumerate() {
        if b.is_empty() {
            continue;
        }
        shelves.push_str(&format!(
            "<section class=\"shelf\" aria-labelledby=\"sh{n}\"><h2 id=\"sh{n}\">{h}</h2><div class=\"row\">{b}</div><div class=\"plank\"></div></section>"
        ));
    }
    let intro = st(cat, "description");
    let lede = if intro.is_empty() { String::new() } else { format!("<p class=\"lede\">{}</p>", esc(intro)) };
    let front_body = if site.privacy.coffee_table {
        // ---------- or the coffee table: magazines with a cover picture
        let mag = |href: &str, kicker: &str, title: &str, sub: &str, cover: String, n: usize| {
            format!(
                "<li class=\"mag\" style=\"--c:var(--b{});--r:{}deg\"><a href=\"{}\"><span class=\"k\">{}</span><span class=\"t\">{}</span>{}<span class=\"s\">{}</span></a></li>",
                n % 6,
                [-3, 2, -1, 3, -2, 1][n % 6],
                esc(&rel(FRONT, href)),
                esc(kicker),
                esc(title),
                if cover.is_empty() { String::new() } else { format!("<span class=\"pic\">{cover}</span>") },
                esc(sub)
            )
        };
        let mut mags = String::new();
        let mut n = 0;
        for s in &ordered {
            let c = ids(s, "memories").len();
            let sub = format!("{c} {}", if c == 1 { "memory" } else { "memories" });
            mags.push_str(&mag(&site.story_url(s), "A story", st(s, "name"), &sub, site.cover_of(FRONT, s, ""), n));
            n += 1;
        }
        for t in &tributes {
            let about = site.get(st(t, "focus"));
            let cover = about.map(|f| site.portrait(FRONT, f, "")).filter(|p| !p.is_empty()).unwrap_or_else(|| site.cover_of(FRONT, t, ""));
            let sub = about.map(|f| life(f)).unwrap_or_default();
            let kicker = if about.is_some_and(|f| f.get("living").is_some()) { "Celebrating" } else { "In memory of" };
            mags.push_str(&mag(&site.tribute_url(t), kicker, st(t, "name"), &sub, cover, n));
            n += 1;
        }
        for f in &families {
            let sub = format!("{} in the family", ids(f, "member").len());
            let face = ids(f, "member").iter().filter_map(|m| site.get(m)).map(|m| site.portrait(FRONT, m, "")).find(|p| !p.is_empty()).unwrap_or_default();
            mags.push_str(&mag(&site.family_url(f), "A family", st(f, "name"), &sub, face, n));
            n += 1;
        }
        let more: String = refs.iter().map(|(h, t, s)| format!("<li>{} <span class=\"muted\">{}</span></li>", site.a(FRONT, h, t), esc(s))).collect();
        format!(
            "<h1>{}</h1>{lede}{}<h2>Also on the table</h2><ul class=\"plain\">{more}</ul><p class=\"muted small\">Each magazine is a plain link. The same pages are listed under People, Places, Timeline and Assorted memories.</p>",
            esc(st(cat, "name")),
            if mags.is_empty() { String::new() } else { format!("<ul class=\"coffee-table\" aria-label=\"On the coffee table\">{mags}</ul>") }
        )
    } else {
        format!(
            "<h1>{}</h1>{lede}<div class=\"bookcase\">{shelves}</div><p class=\"muted small\">Each book is a plain link. The same pages are listed under People, Places, Timeline and Assorted memories.</p>",
            esc(st(cat, "name"))
        )
    };
    site.page(FRONT, st(cat, "name"), &front_body, "front")?;

    // ---------- "In memory of" pages: chosen memories, each with why it matters
    for t in &tributes {
        let path = site.tribute_url(t);
        let about = site.get(st(t, "focus"));
        let notes: HashMap<String, String> = t
            .get("memoryNotes")
            .and_then(|n| n.as_array())
            .into_iter()
            .flatten()
            .map(|l| (st(l, "memory").to_string(), st(l, "note").to_string()))
            .collect();
        let items: String = ids(t, "memories")
            .iter()
            .filter_map(|m| site.get(m))
            .map(|m| {
                let why = notes
                    .get(st(m, "@id"))
                    .filter(|n| !n.trim().is_empty())
                    .map(|n| format!("<div class=\"why\"><p class=\"kicker\">Why it matters</p>{}</div>", paras(n)))
                    .unwrap_or_default();
                format!("<section class=\"tribute-item\">{why}{}</section>", site.memory_block(&path, m))
            })
            .collect();
        let head = about
            .map(|f| {
                format!(
                    "{}<p class=\"kicker\">{}</p>{}",
                    site.portrait(&path, f, "portrait"),
                    site.who_link(&path, f),
                    if life(f).is_empty() { String::new() } else { format!("<p class=\"meta\">{}</p>", esc(&life(f))) }
                )
            })
            .unwrap_or_default();
        let desc = st(t, "description");
        let body = format!(
            "<article class=\"tribute\">{head}<h1>{}</h1>{}{items}</article>",
            esc(st(t, "name")),
            if desc.is_empty() { String::new() } else { paras(desc).replacen("<p>", "<p class=\"lede\">", 1) }
        );
        site.page(&path, st(t, "name"), &body, "")?;
    }

    // ---------- stories, as books that fill the screen
    for s in &stories {
        let path = site.story_url(s);
        let ms: Vec<&Value> = ids(s, "memories").iter().filter_map(|m| site.get(m)).collect();
        let cover = ms
            .iter()
            .flat_map(|m| ids(m, "contents"))
            .filter_map(|c| site.get(&c))
            .find(|c| st(c, "@type") == "ImageObject")
            .and_then(|c| site.shown(c))
            .map(|e| format!("<img src=\"{}\" alt=\"\">", site.src(&path, st(e, "contentUrl"))))
            .unwrap_or_default();
        let leaves: String = ms
            .iter()
            .enumerate()
            .map(|(n, m)| format!("<section class=\"leaf\">{}<p class=\"folio\">{} of {}</p></section>", site.memory_block(&path, m), n + 1, ms.len()))
            .collect();
        let focus = site.get(st(s, "focus")).map(|f| format!("<p>About {}</p>", site.who_link(&path, f))).unwrap_or_default();
        let desc = st(s, "description");
        let body = format!(
            "<section class=\"leaf cover\">{cover}<div><p class=\"kicker\">A story</p><h1>{}</h1>{}{focus}<p class=\"muted small\">Scroll or swipe to turn the pages.</p></div></section>{leaves}",
            esc(st(s, "name")),
            if desc.is_empty() { String::new() } else { format!("<p class=\"lede\">{}</p>", esc(desc)) }
        );
        site.page(&path, st(s, "name"), &body, "reader")?;
    }

    // ---------- people, grouped by Family first, then by individual
    let mut rels_of: HashMap<String, Vec<(String, String, String, bool)>> = HashMap::new();
    for r in site.of("Relationship") {
        let (f, t, k, q) = (st(r, "from").to_string(), st(r, "to").to_string(), st(r, "kind").to_string(), st(r, "qualifier").to_string());
        rels_of.entry(f.clone()).or_default().push((k.clone(), t.clone(), q.clone(), true));
        rels_of.entry(t).or_default().push((k, f, q, false));
    }
    for p in &people {
        let fams: Vec<Option<String>> = match site.fams_of.get(st(p, "@id")) {
            Some(f) if !f.is_empty() => f.iter().cloned().map(Some).collect(),
            _ => vec![None],
        };
        for fam in fams {
            let path = site.person_url(p, fam.as_deref());
            let rl: String = rels_of
                .get(st(p, "@id"))
                .into_iter()
                .flatten()
                .filter_map(|(k, other, q, out)| {
                    let o = site.get(other)?;
                    // The other person is this person's <word>: "Daughter: Ellen".
                    let word = if *out { crate::people::reverse(k).unwrap_or(k) } else { k.as_str() };
                    let g = st(o, "gender");
                    let word = crate::people::capital(&crate::people::gendered(word, if g.is_empty() { None } else { Some(g) }));
                    Some(format!("<li>{}{}: {}</li>", esc(&word), if q.is_empty() { String::new() } else { format!(" ({})", esc(q)) }, site.who_link(&path, o)))
                })
                .collect();
            let my_fams: Vec<&Value> = site.fams_of.get(st(p, "@id")).into_iter().flatten().filter_map(|f| site.get(f)).collect();
            let mut pet_list: Vec<String> = my_fams
                .iter()
                .flat_map(|f| ids(f, "member"))
                .filter_map(|m| site.get(&m))
                .filter(|x| st(x, "@type") == "Pet")
                .map(|x| st(x, "name").to_string())
                .collect();
            pet_list.sort();
            pet_list.dedup();
            let evs: String = events
                .iter()
                .filter(|e| {
                    let at = ids(e, "attendee");
                    at.iter().any(|a| a == st(p, "@id")) || my_fams.iter().any(|f| at.iter().any(|a| a == st(f, "@id")))
                })
                .map(|e| {
                    let w = when(e, "startEdtf", "startDate");
                    let place = site.get(st(e, "location")).map(|l| format!(" · {}", site.a(&path, &site.place_url(l), st(l, "name")))).unwrap_or_default();
                    format!("<li><b>{}</b> <span class=\"muted\">{}{place}</span></li>", esc(st(e, "name")), esc(&w))
                })
                .collect();
            let mine: Vec<&Value> = memories
                .iter()
                .filter(|m| m.get("people").and_then(|x| x.as_array()).is_some_and(|l| l.iter().any(|l| st(l, "individual") == st(p, "@id"))))
                .collect();
            let own_url = site.person_url(p, None);
            let own: Vec<&&Value> = mine.iter().filter(|m| site.home_of(m) == own_url && path == own_url).collect();
            let cards: String = mine
                .iter()
                .filter(|m| !own.iter().any(|o| st(o, "@id") == st(m, "@id")))
                .map(|m| {
                    let roles: Vec<String> = m["people"].as_array().into_iter().flatten().filter(|l| st(l, "individual") == st(p, "@id")).map(Site::role_words).collect();
                    site.memory_card(&path, m, &format!("<span class=\"role\">{}</span>", esc(&roles.join(", "))))
                })
                .collect();
            let kicker: Vec<String> = my_fams.iter().map(|f| site.a(&path, &site.family_url(f), st(f, "name"))).collect();
            let tr: Vec<String> = tributes.iter().filter(|t| st(t, "focus") == st(p, "@id")).map(|t| site.a(&path, &site.tribute_url(t), st(t, "name"))).collect();
            let tr = if tr.is_empty() { String::new() } else { format!("<p class=\"tribute-link\">{}</p>", tr.join(" · ")) };
            let living_note = if p.get("living").is_some() { "<p class=\"muted small\">Dates are left out for living people.</p>" } else { "" };
            let desc = st(p, "description");
            let full: String = own.iter().map(|m| site.memory_block(&path, m)).collect();
            let body = format!(
                "{}<p class=\"kicker\">{}</p><h1>{}</h1><p class=\"meta\">{}</p>{living_note}{tr}{}{}{}{}{}{full}",
                site.portrait(&path, p, "portrait"),
                kicker.join(" · "),
                esc(st(p, "name")),
                esc(&life(p)),
                if desc.is_empty() { String::new() } else { format!("<p class=\"lede\">{}</p>", esc(desc)) },
                if rl.is_empty() { String::new() } else { format!("<h2>Family</h2><ul class=\"plain\">{rl}</ul>") },
                if pet_list.is_empty() { String::new() } else { format!("<p>Pets: {}</p>", esc(&pet_list.join(", "))) },
                if evs.is_empty() { String::new() } else { format!("<h2>Events</h2><ul class=\"plain\">{evs}</ul>") },
                if cards.is_empty() { String::new() } else { format!("<h2>Memories</h2><ul class=\"cards\">{cards}</ul>") },
            );
            site.page(&path, st(p, "name"), &body, "")?;
        }
    }
    let mut groups = String::new();
    for f in &families {
        let li: String = ids(f, "member")
            .iter()
            .filter_map(|m| site.get(m))
            .map(|x| {
                let link = if st(x, "@type") == "Person" {
                    site.a("pages/people.html", &site.person_url(x, Some(st(f, "@id"))), st(x, "name"))
                } else {
                    site.who_link("pages/people.html", x)
                };
                format!("<li>{}{link} <span class=\"muted\">{}</span></li>", site.portrait("pages/people.html", x, "face"), esc(&life(x)))
            })
            .collect();
        let rf: Vec<String> = ids(f, "relatedFamily").iter().filter_map(|r| site.get(r)).map(|r| st(r, "name").to_string()).collect();
        groups.push_str(&format!(
            "<section id=\"{}\" class=\"family\"><h2>{}</h2><p class=\"muted\">{}{}</p><ul class=\"plain\">{li}</ul></section>",
            esc(&site.fam_anchor(f)),
            esc(st(f, "name")),
            esc(st(f, "description")),
            if rf.is_empty() { String::new() } else { format!(" Related to {}.", esc(&rf.join(", "))) }
        ));
    }
    let loose: String = people
        .iter()
        .chain(pets.iter())
        .filter(|p| !site.fams_of.contains_key(st(p, "@id")))
        .map(|p| format!("<li>{}{} <span class=\"muted\">{}</span></li>", site.portrait("pages/people.html", p, "face"), site.who_link("pages/people.html", p), esc(&life(p))))
        .collect();
    if !loose.is_empty() {
        groups.push_str(&format!("<section class=\"family\"><h2>Not in a family</h2><ul class=\"plain\">{loose}</ul></section>"));
    }
    if groups.is_empty() {
        groups = "<p>No one has been added yet.</p>".into();
    }
    site.page(
        "pages/people.html",
        "People",
        &format!("<h1>People</h1><p class=\"lede\">Grouped by family. Someone in two families is listed under both.</p>{groups}"),
        "",
    )?;

    // ---------- places
    for pl in &places {
        let path = site.place_url(pl);
        let here: Vec<&Value> = memories.iter().filter(|m| st(m, "contentLocation") == st(pl, "@id")).collect();
        let own: Vec<&&Value> = here.iter().filter(|m| site.home_of(m) == path).collect();
        let evs: String = events
            .iter()
            .filter(|e| st(e, "location") == st(pl, "@id"))
            .map(|e| format!("<li><b>{}</b> <span class=\"muted\">{}</span></li>", esc(st(e, "name")), esc(&when(e, "startEdtf", "startDate"))))
            .collect();
        let cards: String = here.iter().filter(|m| !own.iter().any(|o| st(o, "@id") == st(m, "@id"))).map(|m| site.memory_card(&path, m, "")).collect();
        let geo = pl.get("geo");
        let addr = st(pl, "address");
        let coords = geo
            .and_then(|g| Some(format!("{:.3}, {:.3}", g.get("latitude")?.as_f64()?, g.get("longitude")?.as_f64()?)))
            .unwrap_or_default();
        let where_ = if !addr.is_empty() || !coords.is_empty() {
            format!("<p class=\"meta\">{}{}{}</p>", esc(addr), if !addr.is_empty() && !coords.is_empty() { " · " } else { "" }, esc(&coords))
        } else {
            String::new()
        };
        let full: String = own.iter().map(|m| site.memory_block(&path, m)).collect();
        let desc = st(pl, "description");
        let body = format!(
            "<p class=\"kicker\">{}</p><h1>{}</h1>{where_}{}{}{}{full}",
            site.a(&path, "pages/places.html", "Places"),
            esc(st(pl, "name")),
            if desc.is_empty() { String::new() } else { format!("<p class=\"lede\">{}</p>", esc(desc)) },
            if evs.is_empty() { String::new() } else { format!("<h2>Events here</h2><ul class=\"plain\">{evs}</ul>") },
            if cards.is_empty() { String::new() } else { format!("<h2>Memories here</h2><ul class=\"cards\">{cards}</ul>") },
        );
        site.page(&path, st(pl, "name"), &body, "")?;
    }
    let list: String = places
        .iter()
        .map(|p| format!("<li>{} <span class=\"muted\">{}</span></li>", site.a("pages/places.html", &site.place_url(p), st(p, "name")), esc(st(p, "description"))))
        .collect();
    site.page(
        "pages/places.html",
        "Places",
        &format!("<h1>Places</h1>{}", if list.is_empty() { "<p>No places have been added yet.</p>".to_string() } else { format!("<ul class=\"plain\">{list}</ul>") }),
        "",
    )?;

    // ---------- the timeline: events and dated memories in date order
    let mut rows: Vec<(String, String, String)> = Vec::new();
    let tl = "pages/timeline.html";
    for e in &events {
        let Some(d) = sort_date(e, "startEdtf", "startDate") else { continue };
        let place = site.get(st(e, "location")).map(|l| format!(" · {}", site.a(tl, &site.place_url(l), st(l, "name")))).unwrap_or_default();
        let who: Vec<String> = ids(e, "attendee").iter().filter_map(|a| site.get(a)).map(|a| site.who_link(tl, a)).collect();
        let who = if who.is_empty() { String::new() } else { format!(" <span class=\"muted\">({})</span>", who.join(", ")) };
        rows.push((
            d,
            when(e, "startEdtf", "startDate"),
            format!("<span class=\"kind\">Event</span> <b>{}</b>{who}{place}", esc(st(e, "name"))),
        ));
    }
    for m in &memories {
        let Some(d) = sort_date(m, "dateEdtf", "temporalCoverage") else { continue };
        let place = site.get(st(m, "contentLocation")).map(|l| format!(" · {}", site.a(tl, &site.place_url(l), st(l, "name")))).unwrap_or_default();
        rows.push((
            d,
            when(m, "dateEdtf", "temporalCoverage"),
            format!("<span class=\"kind\">Memory</span> {}{place}", site.a(tl, &format!("{}#m-{}", site.home_of(m), pid(m)), st(m, "name"))),
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    let mut tl_body = String::new();
    let mut decade = String::new();
    for (d, w, item) in &rows {
        let dec = format!("{}0s", d.get(..3).unwrap_or("????"));
        if dec != decade {
            if !decade.is_empty() {
                tl_body.push_str("</ol></section>");
            }
            tl_body.push_str(&format!("<section class=\"decade\"><h2>The {}</h2><ol class=\"timeline\">", esc(&dec)));
            decade = dec;
        }
        tl_body.push_str(&format!("<li><span class=\"when\">{}</span><span class=\"what\">{item}</span></li>", esc(w)));
    }
    if !decade.is_empty() {
        tl_body.push_str("</ol></section>");
    }
    if rows.is_empty() {
        tl_body = "<p>Nothing has a date yet.</p>".into();
    }
    site.page(
        tl,
        "Timeline",
        &format!("<h1>Timeline</h1><p class=\"lede\">Events and memories in date order. A date that is only known roughly is placed by its nearest exact date.</p>{tl_body}"),
        "",
    )?;

    // ---------- assorted memories: in no story, with no person or place
    let assorted_html: String = assorted.iter().map(|m| site.memory_block("pages/assorted-memories.html", m)).collect();
    site.page(
        "pages/assorted-memories.html",
        "Assorted memories",
        &format!(
            "<h1>Assorted memories</h1><p class=\"lede\">Memories that are not yet part of a story and are not connected to a person or a place.</p>{}",
            if assorted_html.is_empty() { "<p>None.</p>".to_string() } else { assorted_html }
        ),
        "",
    )?;

    // ---------- about, and the plain-language guide
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let counts = format!(
        "{}, {}, {}, {}, {}",
        plural(stories.len(), "story", "stories"),
        plural(memories.len(), "memory", "memories"),
        plural(people.len(), "person", "people"),
        plural(pets.len(), "pet", "pets"),
        plural(places.len(), "place", "places")
    );
    let mut privacy_words = Vec::new();
    if !site.privacy.living_dates {
        privacy_words.push("Dates of living people are left out of this copy.");
    }
    if !site.privacy.exact_places {
        privacy_words.push("Places marked exact, such as the family home, show only their name, and pictures and videos carry no map location.");
    }
    if privacy_words.is_empty() {
        privacy_words.push("This copy includes the dates of living people and exact addresses. Share it with care.");
    }
    site.page(
        "pages/about.html",
        "About this folder",
        &format!("<h1>About this folder</h1><p class=\"lede\">{}: {counts}.</p>{ABOUT_HTML}<h2>Privacy</h2><p>{}</p>", esc(st(cat, "name")), privacy_words.join(" ")),
        "",
    )?;
    // ---------- search: a small index beside the pages, read by one script
    let sp = "pages/search.html";
    let mut idx: Vec<Value> = Vec::new();
    {
        let mut add = |k: &str, t: &str, u: String, x: Vec<String>| {
            let x: String = x.into_iter().filter(|w| !w.trim().is_empty()).collect::<Vec<_>>().join(" ").chars().take(4000).collect();
            idx.push(serde_json::json!({"k": k, "t": t, "u": rel(sp, &u), "x": x}));
        };
        for s in &stories {
            add("Story", st(s, "name"), site.story_url(s), vec![st(s, "description").into()]);
        }
        for t in &tributes {
            let about = site.get(st(t, "focus")).map(|f| st(f, "name").to_string()).unwrap_or_default();
            let notes: Vec<String> = t.get("memoryNotes").and_then(|n| n.as_array()).into_iter().flatten().map(|l| st(l, "note").to_string()).collect();
            add("In memory of", st(t, "name"), site.tribute_url(t), [vec![about, st(t, "description").into()], notes].concat());
        }
        for p in &people {
            let other: Vec<String> = p.get("alternateName").map(|a| match a {
                Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                Value::String(x) => vec![x.clone()],
                _ => vec![],
            }).unwrap_or_default();
            add("Person", st(p, "name"), site.person_url(p, None), [vec![life(p), st(p, "description").into()], other].concat());
        }
        for f in &families {
            add("Family", st(f, "name"), site.family_url(f), vec![st(f, "description").into()]);
        }
        for pl in &places {
            add("Place", st(pl, "name"), site.place_url(pl), vec![st(pl, "description").into()]);
        }
        for e in &events {
            add("Event", st(e, "name"), tl.to_string(), vec![when(e, "startEdtf", "startDate"), st(e, "description").into()]);
        }
        for m in &memories {
            let mut x = vec![when(m, "dateEdtf", "temporalCoverage"), st(m, "description").into()];
            if let Some(p) = site.get(st(m, "contentLocation")) {
                x.push(st(p, "name").into());
            }
            for l in m.get("people").and_then(|p| p.as_array()).into_iter().flatten() {
                if let Some(w) = site.get(st(l, "individual")) {
                    x.push(st(w, "name").into());
                }
            }
            for c in ids(m, "contents").iter().filter_map(|c| site.get(c)) {
                x.push(st(c, "name").into());
                x.push(st(c, "description").into());
                if st(c, "@type") == "CreativeWork" {
                    x.extend((site.text)(&pid(c)));
                }
                if let Some(tr) = site.linked.get(&("transcriptOf".into(), st(c, "@id").to_string())).and_then(|t| site.get(t)) {
                    x.extend((site.text)(&pid(tr)));
                }
            }
            add("Memory", st(m, "name"), format!("{}#m-{}", site.home_of(m), pid(m)), x);
        }
    }
    let json_err = |e: serde_json::Error| crate::Error::Invalid(e.to_string());
    std::fs::write(out.join("pages").join("search.js"), format!("window.SEARCH = {};\n", serde_json::to_string(&idx).map_err(json_err)?))?;
    site.page(sp, "Search", SEARCH_HTML, "search")?;

    let mut fields: Vec<String> =
        cat["@graph"].as_array().into_iter().flatten().filter_map(|o| o.as_object()).flat_map(|o| o.keys().cloned()).filter(|k| !k.starts_with('@')).collect();
    fields.sort();
    fields.dedup();
    let field_lines: String = fields.iter().map(|k| format!("  {k}: {}\n", field_words(k))).collect();
    let txt = ABOUT_TXT
        .replace("{name}", st(cat, "name"))
        .replace("{version}", st(cat, "formatVersion"))
        .replace("{date}", st(cat, "dateModified"))
        .replace("{counts}", &counts)
        .replace("{front}", FRONT)
        .replace("{privacy}", &privacy_words.join("\n"))
        .replace("{fields}", field_lines.trim_end());
    std::fs::write(out.join(ABOUT_FILE), txt.replace('\n', "\r\n"))?;
    Ok(site.pages)
}

/// The search page. Without scripts it says where the plain lists are; with
/// them, it reads `search.js` (made with the pages) and lists what matches
/// every word typed, titles first. Nothing is sent anywhere.
const SEARCH_HTML: &str = "<h1>Search</h1>\
<form role=\"search\" action=\"search.html\" method=\"get\" class=\"find\"><label for=\"q\">Search names, titles, captions and transcripts</label>\
<div><input id=\"q\" name=\"q\" type=\"search\" autocomplete=\"off\"> <button type=\"submit\">Search</button></div></form>\
<p id=\"found\" class=\"muted\" role=\"status\"></p><ol id=\"results\" class=\"plain results\"></ol>\
<noscript><p>Search needs JavaScript, which is turned off. Everything is still listed under People, Places, Timeline and Assorted memories.</p></noscript>\
<script src=\"search.js\"></script>\
<script>(function(){var all=window.SEARCH||[],box=document.getElementById('q'),list=document.getElementById('results'),found=document.getElementById('found');\
function fold(s){return (s||'').normalize('NFD').replace(/[\\u0300-\\u036f]/g,'').toLowerCase();}\
all.forEach(function(e){e.ft=fold(e.t);e.fx=fold(e.x);});\
function run(){var q=fold(box.value).trim(),words=q.split(/\\s+/).filter(Boolean);list.textContent='';\
if(!words.length){found.textContent='';return;}\
var hits=all.filter(function(e){return words.every(function(w){return e.ft.indexOf(w)>=0||e.fx.indexOf(w)>=0;});});\
hits.sort(function(a,b){var ta=words.every(function(w){return a.ft.indexOf(w)>=0;}),tb=words.every(function(w){return b.ft.indexOf(w)>=0;});return (tb-ta)||a.t.localeCompare(b.t);});\
found.textContent=hits.length===1?'1 match':hits.length+' matches';\
hits.slice(0,200).forEach(function(e){var li=document.createElement('li'),a=document.createElement('a'),k=document.createElement('span');\
a.href=e.u;a.textContent=e.t;k.className='kind';k.textContent=e.k;li.appendChild(k);li.appendChild(a);\
var i=fold(e.x).indexOf(words[0]);if(i>=0&&e.ft.indexOf(words[0])<0){var sn=document.createElement('span');sn.className='muted snip';\
sn.textContent=' '+(i>40?'…':'')+e.x.substr(Math.max(0,i-40),120)+'…';li.appendChild(sn);}list.appendChild(li);});}\
box.value=new URLSearchParams(location.search).get('q')||'';box.addEventListener('input',run);run();})();</script>";

const ABOUT_HTML: &str = "<h2>What is in this folder</h2>\
<ul><li><b>Open me - Family History.html</b> opens the bookshelf in any web browser. Nothing is fetched from the internet.</li>\
<li><b>pages/</b> holds one page per story, person and place, grouped by family, plus the timeline. Pets, events and memories appear on those pages.</li>\
<li><b>photos/, audio/, video/, stories/</b> hold the files themselves, by type and then year. Names start with the date, then a short name, then a permanent ID. Each file also carries its title, date, people and description inside it, so it still says what it is when copied on its own.</li>\
<li><b>catalog.json</b> lists everything and how it connects, in JSON-LD with schema.org names. <b>context.jsonld</b> says what each name means. <b>ABOUT-THIS-FOLDER.txt</b> explains every field in plain words.</li></ul>";

const ABOUT_TXT: &str = "{name}
Family history folder, catalog format {version}, last changed {date}.
Contents: {counts}.

HOW TO OPEN IT
Open \"{front}\" in any web browser. It works offline and needs no app.
Every page is plain HTML; it still reads with JavaScript turned off.

WHAT IS WHERE
  pages/            one page per story, person and place, grouped by family,
                    and a timeline of events and memories in date order
  photos/ audio/ video/ stories/
                    the files, by type and then year. Names are DATE_short-name_ID,
                    for example 1961-01-20_ask-not-inaugural-address_k7f3m2qa.mp3
                    A transcript sits beside its recording, as .txt and .vtt.
                    Each file carries its own title, date, people and description
                    (EXIF and XMP in pictures, ID3 in MP3s, a heading in texts).
  catalog.json      everything and how it connects (JSON-LD, schema.org names)
  catalog.js        the same, wrapped so web pages can read it from a folder
  context.jsonld    what each name in the catalog means

HOW IT FITS TOGETHER
A Journey holds stories, memories, people, pets, families, places and events.
A story is memories in order. A memory is one moment: it holds the photos,
recordings, videos and texts, and names the people (with their role), the
place and the event. Photos and recordings connect to people only through
a memory.

PRIVACY
{privacy}

FIELDS IN THE CATALOG
{fields}

If a later version of the app reads this folder, it upgrades a copy of the
catalog and leaves this one unchanged.
";

fn field_words(k: &str) -> &'static str {
    match k {
        "abstract" => "a short summary of a text",
        "addedBy" => "who added it",
        "address" => "street address",
        "alternateName" => "other names",
        "attendee" => "who took part",
        "birthDate" => "date of birth",
        "birthDateEdtf" => "date of birth as known",
        "breed" => "breed",
        "certainty" => "certain or probably",
        "contentLocation" => "where the memory happened",
        "contents" => "the photos, recordings, videos and texts in a memory, in order",
        "copyrightNotice" => "copyright",
        "cover" => "the picture shown first for a memory",
        "creditText" => "photographer, speaker or writer",
        "dateAdded" => "when it was added",
        "dateCreated" => "when it was made (nearest exact date)",
        "dateEdtf" => "the date as known; ~ means about, ? maybe, X an unknown digit, / a range",
        "deathDate" => "date of death",
        "deathDateEdtf" => "date of death as known",
        "description" => "a short description or caption",
        "duration" => "length (ISO 8601, PT1M10S = 1 minute 10 seconds)",
        "encoding" => "the files: path, format, size and SHA-256 checksum",
        "endDate" => "when the event ended",
        "endEdtf" => "when it ended, as known",
        "event" => "the event a memory is about",
        "familyName" => "surname",
        "focus" => "who the story is about",
        "gender" => "gender",
        "genre" => "\"story\" for a story",
        "geo" => "map coordinates",
        "givenName" => "first name",
        "height" => "height in pixels",
        "inLanguage" => "language",
        "joinedDate" => "when the pet joined the family",
        "kind" => "the kind of relationship or event",
        "living" => "true if the person is living (dates left out)",
        "location" => "where the event happened",
        "member" => "the people and pets in a family",
        "memories" => "the memories in a story, in order",
        "name" => "the title or name",
        "people" => "who is connected to a memory, with their role",
        "peopleNote" => "who is where in the picture",
        "qualifier" => "more about a parent link (birth, adoptive, step, foster)",
        "relatedFamily" => "a family this one is related to",
        "reviewStatus" => "automatic, or checked by a person",
        "sourceNote" => "where it came from",
        "speakerNote" => "who speaks when",
        "species" => "kind of animal",
        "startDate" => "when the event began",
        "startEdtf" => "when it began, as known",
        "status" => "complete, or still needs information",
        "stillFrameOf" => "the video this picture comes from",
        "temporalCoverage" => "when the moment happened (nearest exact date)",
        "transcribedBy" => "who or what wrote the transcript",
        "transcriptOf" => "the recording this text transcribes",
        "from" => "the first person in a relationship",
        "to" => "the second person",
        "width" => "width in pixels",
        _ => "see context.jsonld",
    }
}
