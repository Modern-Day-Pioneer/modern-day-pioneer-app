use std::time::{Duration, Instant};

use mdp_core::jobs::{spawn, Outcome, Staging};

fn wait_until(mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_job_reports_progress_and_skipped_items() {
    let job = spawn(|ctx| {
        ctx.set_total(5);
        for i in 0..5 {
            if i == 2 {
                ctx.skip("photo-3.heic", "The file could not be read.");
            } else {
                ctx.advance(format!("Copied {}", i + 1));
            }
        }
        Ok(42)
    });
    match job.wait() {
        Outcome::Finished { value, skipped } => {
            assert_eq!(value, 42);
            assert_eq!(skipped.len(), 1);
            assert_eq!(skipped[0].item, "photo-3.heic");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn progress_can_be_read_while_it_runs() {
    let job = spawn(|ctx| {
        ctx.set_total(1000);
        for i in 0..1000 {
            ctx.check()?;
            ctx.advance(format!("{i}"));
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    });
    wait_until(|| job.progress().done >= 10);
    let p = job.progress();
    assert_eq!(p.total, 1000);
    assert!(p.done < 1000);
    job.cancel();
    assert!(matches!(job.wait(), Outcome::Canceled));
}

#[test]
fn canceling_leaves_nothing_half_written() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("Rowans.mdp-backup.zip");
    let d = dest.clone();
    let job = spawn(move |ctx| {
        let staging = Staging::new(&d)?;
        std::fs::create_dir_all(staging.path())?;
        for i in 0.. {
            ctx.check()?;
            std::fs::write(staging.path().join(format!("{i}.txt")), "x")?;
            ctx.advance("writing");
            std::thread::sleep(Duration::from_millis(1));
        }
        staging.commit()
    });
    wait_until(|| job.progress().done >= 5);
    job.cancel();
    assert!(matches!(job.wait(), Outcome::Canceled));
    assert!(!dest.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "no partial files left");
}

#[test]
fn a_finished_staging_file_lands_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.txt");
    let s = Staging::new(&dest).unwrap();
    std::fs::write(s.path(), "done").unwrap();
    s.commit().unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "done");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(Staging::new(&dest).is_err(), "never overwrites");
}

#[test]
fn a_failing_job_reports_its_error() {
    let job = spawn(|_| -> mdp_core::Result<()> { Err(mdp_core::Error::Invalid("Disk full.".into())) });
    match job.wait() {
        Outcome::Failed(e) => assert_eq!(e.to_string(), "Disk full."),
        other => panic!("{other:?}"),
    }
}

#[test]
fn item_names_stay_out_of_the_message_and_only_the_latest_are_kept() {
    let ctx = mdp_core::jobs::JobContext::new();
    ctx.message("Analyzing content…");
    ctx.set_total(250);
    for i in 0..250 {
        ctx.advance_item(format!("file {i}.jpg"));
    }
    let p = ctx.progress();
    assert_eq!((p.done, p.total, p.message.as_str()), (250, 250, "Analyzing content…"));
    let recent = ctx.recent();
    assert_eq!(recent.len(), mdp_core::jobs::RECENT_ITEMS);
    assert_eq!(recent.first().map(String::as_str), Some("file 50.jpg"));
    assert_eq!(recent.last().map(String::as_str), Some("file 249.jpg"));
    ctx.start_step("Making copies…", 3);
    assert_eq!((ctx.progress().done, ctx.progress().total), (0, 3));
}
