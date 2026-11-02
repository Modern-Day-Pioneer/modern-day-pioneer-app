//! A small command-line tool for testing the core before the app has screens.
//!
//!   mdp-tool create <folder> <journey name>
//!   mdp-tool backup <journey folder> [archive.zip]
//!   mdp-tool restore <archive.zip> <new folder>

use std::path::{Path, PathBuf};
use std::time::Duration;

use mdp_core::backup::{backup, restore, suggested_name};
use mdp_core::jobs::{spawn, Outcome};
use mdp_core::Journey;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["create", folder, name] => Journey::create(Path::new(folder), name).map(|j| format!("Created {}", j.root().display())),
        ["backup", folder] | ["backup", folder, _] => {
            let root = PathBuf::from(folder);
            let dest = match args.get(2) {
                Some(d) => PathBuf::from(d),
                None => match Journey::open(&root).and_then(|j| j.name()) {
                    Ok(name) => PathBuf::from(suggested_name(&name)),
                    Err(e) => fail(e),
                },
            };
            run(move |ctx| backup(&root, &dest, ctx))
        }
        ["restore", archive, folder] => {
            let (a, f) = (PathBuf::from(archive), PathBuf::from(folder));
            run(move |ctx| restore(&a, &f, ctx))
        }
        _ => {
            eprintln!("usage:\n  mdp-tool create <folder> <journey name>\n  mdp-tool backup <journey folder> [archive.zip]\n  mdp-tool restore <archive.zip> <new folder>");
            std::process::exit(2);
        }
    };
    match result {
        Ok(msg) => println!("{msg}"),
        Err(e) => fail(e),
    }
}

fn run<F>(f: F) -> mdp_core::Result<String>
where
    F: FnOnce(&mdp_core::jobs::JobContext) -> mdp_core::Result<PathBuf> + Send + 'static,
{
    let job = spawn(f);
    while !job.is_finished() {
        let p = job.progress();
        eprint!("\r{} of {}  {:<60}", p.done, p.total, p.message.chars().take(60).collect::<String>());
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!();
    match job.wait() {
        Outcome::Finished { value, .. } => Ok(format!("Done: {}", value.display())),
        Outcome::Canceled => Ok("Canceled.".into()),
        Outcome::Failed(e) => Err(e),
    }
}

fn fail(e: mdp_core::Error) -> ! {
    eprintln!("Error: {e}");
    std::process::exit(1);
}
