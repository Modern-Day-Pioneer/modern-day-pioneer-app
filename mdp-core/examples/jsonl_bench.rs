//! Times building the plain-text copy on a Journey of N plain pictures:
//! `cargo run --release --example jsonl_bench -- 10000`.

use serde_json::json;

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(10000);
    let dir = std::env::temp_dir().join(format!("mdp-bench-{}", std::process::id()));
    let mut j = mdp_core::Journey::create(&dir, "Bench").unwrap();
    j.act(|a| {
        for i in 0..n {
            let id = a.create("content", json!({"kind": "picture", "name": format!("IMG_{i}"), "state": "in a memory", "date": "1962"}).as_object().cloned().unwrap())?;
            a.insert("picture", json!({"id": id, "width": 4000, "height": 3000}).as_object().cloned().unwrap())?;
            a.insert("content_file", json!({"content_id": id, "is_copy": 0, "path": format!("managed-content/x/{i}.jpg"), "format": "image/jpeg", "size": 123456, "sha256": "ab"}).as_object().cloned().unwrap())?;
        }
        Ok(())
    })
    .unwrap();
    for _ in 0..3 {
        let t = std::time::Instant::now();
        let l = mdp_core::jsonl::lines(j.conn()).unwrap();
        let built = t.elapsed();
        mdp_core::jsonl::write(&j).unwrap();
        println!("{} lines: built in {:.3} s, all in {:.3} s", l.len(), built.as_secs_f64(), t.elapsed().as_secs_f64());
    }
    drop(j);
    let _ = std::fs::remove_dir_all(dir);
}
