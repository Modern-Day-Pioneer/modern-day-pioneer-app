//! Sprint 17: the Scrapbook style for the exported history (scrapbook theme
//! plan phase 5). A second stylesheet over the same pages, chosen per Journey;
//! the plain style stays the default.

use std::path::{Path, PathBuf};

use mdp_core::export::{Privacy, FRONT};
use mdp_core::jobs::JobContext;

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn pages(dest: &Path) -> Vec<PathBuf> {
    walk(dest).into_iter().filter(|p| p.extension().is_some_and(|e| e == "html")).collect()
}

const LINK: &str = "scrapbook.css\">";

#[test]
fn scrapbook_style_is_a_second_stylesheet_over_the_same_pages() {
    let dir = tempfile::tempdir().unwrap();
    let j = mdp_core::sample::create(&dir.path().join("sample")).unwrap();
    let base = Privacy::from_settings("leave out", "town only");
    let plain = dir.path().join("plain");
    let scrap = dir.path().join("scrap");
    j.export(&plain, base, &JobContext::new()).unwrap();
    j.export(&scrap, base.with_export_style("scrapbook"), &JobContext::new()).unwrap();

    // The plain style is the default and has no trace of the theme.
    assert!(!Privacy::default().scrapbook && !base.with_export_style("plain").scrapbook);
    assert!(!plain.join("pages/scrapbook.css").exists() && !plain.join("pages/props").exists());
    for p in pages(&plain) {
        assert!(!std::fs::read_to_string(&p).unwrap().contains("scrapbook.css"), "{}", p.display());
    }

    // E15: every Scrapbook page is the plain page plus one stylesheet link,
    // after style.css, so with the file removed every page reads as plain.
    let (a, b) = (pages(&plain), pages(&scrap));
    assert_eq!(a.len(), b.len());
    assert!(b.iter().any(|p| p.ends_with(FRONT)));
    for (pa, pb) in a.iter().zip(&b) {
        assert_eq!(pa.strip_prefix(&plain).unwrap(), pb.strip_prefix(&scrap).unwrap());
        let (ha, hb) = (std::fs::read_to_string(pa).unwrap(), std::fs::read_to_string(pb).unwrap());
        let i = hb.find(LINK).unwrap_or_else(|| panic!("{} has no Scrapbook link", pb.display()));
        let start = hb[..i].rfind("<link").unwrap();
        assert!(hb[..start].contains("style.css"), "the Scrapbook sheet comes after the plain one");
        let href = &hb[start..i + LINK.len()];
        let target = href.trim_start_matches("<link rel=\"stylesheet\" href=\"").trim_end_matches("\">");
        assert!(pb.parent().unwrap().join(target).exists(), "{} links to missing {target}", pb.display());
        assert_eq!(hb.replacen(href, "", 1), ha, "{} differs from the plain page", pb.display());
    }
    // The data is the same in both.
    for f in ["catalog.json", "catalog.js"] {
        assert_eq!(std::fs::read(plain.join(f)).unwrap(), std::fs::read(scrap.join(f)).unwrap(), "{f}");
    }

    // E2/E3 and E16: nothing from outside, and the theme's files stay within 300 KB.
    let css = std::fs::read_to_string(scrap.join("pages/scrapbook.css")).unwrap();
    assert!(!css.contains("@import") && !css.contains("@font-face"));
    let mut rest = css.as_str();
    while let Some(i) = rest.find("url(") {
        rest = &rest[i + 4..];
        let u = rest[..rest.find(')').unwrap()].trim_matches(|c| c == '"' || c == '\'');
        if !u.starts_with("data:") && !u.starts_with('%') {
            assert!(!u.contains("://"), "outside file {u}");
            assert!(scrap.join("pages").join(u).exists(), "missing prop {u}");
        }
    }
    let theme: u64 = std::iter::once(scrap.join("pages/scrapbook.css"))
        .chain(walk(&scrap.join("pages/props")))
        .map(|p| std::fs::metadata(p).unwrap().len())
        .sum();
    assert!(theme <= 300 * 1024, "theme files are {theme} bytes");
    // Print, high contrast and "more contrast" fall back to the plain style.
    assert!(css.contains("@media screen") && css.contains("forced-colors: active") && css.contains("prefers-contrast: more"));
    assert!(!css.contains("animation") && !css.contains("transition"), "nothing moves");

    // E14: the same Journey exported twice gives the same pages.
    let again = dir.path().join("again");
    j.export(&again, base.with_export_style("scrapbook"), &JobContext::new()).unwrap();
    for p in pages(&scrap) {
        let q = again.join(p.strip_prefix(&scrap).unwrap());
        assert_eq!(std::fs::read(&p).unwrap(), std::fs::read(&q).unwrap(), "{}", p.display());
    }
}

#[test]
fn the_preview_shows_the_chosen_style() {
    let dir = tempfile::tempdir().unwrap();
    let j = mdp_core::sample::create(&dir.path().join("sample")).unwrap();
    let (front, _) = j.preview(Privacy::default().with_export_style("scrapbook")).unwrap();
    let html = std::fs::read_to_string(&front).unwrap();
    assert!(html.contains(LINK));
    let pages_dir = front.parent().unwrap().join("pages");
    assert!(pages_dir.join("scrapbook.css").exists() && pages_dir.join("props/tape.webp").exists());
}
