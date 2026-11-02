//! Saves the sample family from this build as a release sample:
//! a backup and a zipped export, which `tests/release_samples.rs` opens with
//! every later build. Run once per public release:
//! cargo run --example release_sample -- tests/fixtures/releases/<format>
use std::io::Write;
use std::path::Path;

use mdp_core::export::Privacy;
use mdp_core::jobs::JobContext;

fn zip_dir(dir: &Path, out: &Path) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(out).unwrap());
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![dir.to_path_buf()];
    let mut files = vec![];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { stack.push(p) } else { files.push(p) }
        }
    }
    files.sort();
    for p in files {
        let name = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
        zip.start_file(name, opts).unwrap();
        zip.write_all(&std::fs::read(&p).unwrap()).unwrap();
    }
    zip.finish().unwrap();
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: release_sample <folder>"));
    std::fs::create_dir_all(&out).unwrap();
    let t = std::env::temp_dir().join(format!("mdp-release-sample-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&t);
    let j = mdp_core::sample::create(&t.join("journey")).unwrap();
    let ctx = JobContext::new();
    mdp_core::backup::backup(j.root(), &out.join("journey.mdp-backup.zip"), &ctx).unwrap();
    j.export(&t.join("export"), Privacy::from_settings("include", "exact"), &ctx).unwrap();
    zip_dir(&t.join("export"), &out.join("export.zip"));
    let _ = std::fs::remove_dir_all(&t);
    println!("Saved the release sample in {}", out.display());
}
