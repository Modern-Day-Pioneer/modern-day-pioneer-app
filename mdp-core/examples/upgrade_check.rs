//! Upgrade safety check (sprint 12): restore a backup made by an
//! older build, open it with this build, and confirm nothing is lost.
//! Usage: cargo run --example upgrade_check -- <backup.zip> <empty folder>
use std::path::Path;

fn count(j: &mdp_core::Journey, t: &str) -> i64 {
    j.conn().query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ctx = mdp_core::jobs::JobContext::new();
    let root = mdp_core::backup::restore(Path::new(&a[1]), Path::new(&a[2]), &ctx).expect("restore");
    let mut j = mdp_core::Journey::open(&root).expect("open with this build");
    for t in ["individual", "family", "place", "event", "story", "memory", "content", "content_file", "memory_content", "memory_individual"] {
        println!("{t}: {}", count(&j, t));
    }
    let check = j.check_files(&ctx).expect("check files");
    println!("file check: {check:?}");
    println!("home: {} stories", j.stories().unwrap().len());
}
