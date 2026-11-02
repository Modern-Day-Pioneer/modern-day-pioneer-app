//! Checks the plain-text copy is unchanged by a new way of building it:
//! `cargo run --example jsonl_check make DIR`, change the code, then
//! `cargo run --example jsonl_check check DIR`.

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(&a[2]);
    if a[1] == "make" {
        let j = mdp_core::sample::create(&dir).expect("sample");
        mdp_core::jsonl::write(&j).unwrap();
        return;
    }
    let j = mdp_core::Journey::open(&dir).expect("open");
    let file = std::fs::read_to_string(dir.join("catalog.jsonl")).unwrap();
    let mut now = String::new();
    for l in mdp_core::jsonl::lines(j.conn()).unwrap() {
        now.push_str(&serde_json::to_string(&l).unwrap());
        now.push('\n');
    }
    assert_eq!(file, now, "the copy changed");
    println!("same: {} lines", now.lines().count());
}
