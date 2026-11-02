//! Prints the sample family as the app shell sees it, for checking the
//! screens in a plain browser: `cargo run --example shell_fixture > fixture.json`.

fn main() {
    let dir = std::env::temp_dir().join(format!("mdp-fixture-{}", std::process::id()));
    let mut j = mdp_core::sample::create(&dir).expect("sample");
    let mut search = serde_json::Map::new();
    let names: Vec<(String, String, String)> = {
        let mut stmt = j.conn().prepare("SELECT id, name FROM individual UNION ALL SELECT id, name FROM family UNION ALL SELECT id, name FROM place UNION ALL SELECT id, name FROM story UNION ALL SELECT id, name FROM memory UNION ALL SELECT id, name FROM event").unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, String::new()))).unwrap().map(|r| r.unwrap()).collect()
    };
    for q in ["w", "wa", "wal", "walt", "walter", "j", "ju", "jun", "june", "r", "ro", "row", "rowan", "o", "og", "ogd", "ogden", "k", "ki", "kit", "kitchen", "p", "pe", "pep", "pepper", "s", "su", "sun", "sunday", "zz"] {
        search.insert(q.into(), serde_json::to_value(j.search(q, 20).unwrap()).unwrap());
    }
    let items: serde_json::Map<String, serde_json::Value> =
        names.iter().map(|(id, _, _)| (id.clone(), j.item(id).unwrap().unwrap())).collect();
    let out = serde_json::json!({
        "home": j.home().unwrap(),
        "stories": j.stories().unwrap(),
        "people": j.people_by_family().unwrap(),
        "waiting": j.waiting().unwrap(),
        "search": search,
        "items": items,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
    drop(j);
    let _ = std::fs::remove_dir_all(dir);
}
