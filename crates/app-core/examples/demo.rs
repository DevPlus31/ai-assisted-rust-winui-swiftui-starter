//! A two-minute tour of the engine, in a throwaway folder:
//! analyse (with progress) → preview → apply → undo, plus a translated
//! message. Nothing outside a temporary directory is touched.
//!
//!   make demo        (or: cargo run -p app-core --example demo)

use std::fs;
use std::path::Path;
use std::sync::Arc;

use app_core::workflow::Destinations;
use app_core::{Engine, Job, TextKey, UserText};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let sandbox = tempfile::tempdir()?;
    let inbox = sandbox.path().join("Downloads");
    let organized = sandbox.path().join("Organized");
    make_messy_folder(&inbox)?;

    step("A messy Downloads folder");
    print_tree(&inbox, &inbox)?;

    let mut engine = Engine::open(&sandbox.path().join("app-data"))?;

    step("1. Analyse it on a background thread, watching progress");
    let job = Arc::new(Job::new());
    let (insights, failures) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| engine.analyze_folder(&inbox, false, &job));
        // The UI polls the job; it never gets called back from engine threads.
        while !worker.is_finished() {
            std::thread::yield_now();
        }
        worker.join().expect("analysis thread panicked")
    })?;
    let progress = job.progress();
    println!(
        "   analysed {} of {} files, {} unreadable",
        progress.done,
        progress.total,
        failures.len()
    );
    for insight in &insights {
        println!(
            "   {:<38} {:<12} {}",
            insight.name,
            insight.category.label(),
            insight.reason
        );
    }

    step("2. Preview: nothing has moved yet");
    let pending = engine.plan(&insights, &Destinations::single(&organized))?;
    for planned in &pending.plan.moves {
        println!(
            "   {:<38} → {}",
            file_name(&planned.from),
            planned.to.strip_prefix(sandbox.path())?.display()
        );
    }
    println!("   ({} left as they are)", pending.plan.unchanged);

    step("3. Apply the plan by its id (the shell never sends paths)");
    let report = engine.apply_plan(pending.id, &[])?;
    println!("   moved {} files", report.batch.applied.len());
    print_tree(&organized, sandbox.path())?;
    println!("   Downloads now holds {} files", count_files(&inbox)?);

    step("4. Undo puts everything back");
    let undone = engine.undo_last()?;
    println!("   put back {} files", undone.batch.applied.len());
    print_tree(&inbox, &inbox)?;

    step("5. Every message has a key, so shells can translate it");
    let error = engine
        .plan(&insights, &Destinations::single("Organized"))
        .expect_err("a relative destination is refused");
    let text = error.user_text();
    println!("   key:     {}", text.key.id());
    println!("   English: {text}");
    println!(
        "   Deutsch: {}",
        text.render("„{folder}“ ist kein vollständiger Ordnerpfad.")
    );
    let cancelled = UserText::new(TextKey::Cancelled);
    println!("   (another one: {} → {cancelled})", cancelled.key.id());

    println!("\nDone. The temporary folder is deleted now.");
    Ok(())
}

fn make_messy_folder(inbox: &Path) -> Result<()> {
    fs::create_dir_all(inbox)?;
    fs::write(
        inbox.join("scan_0042.txt"),
        "Invoice number 1042\nBill to: Sam Lee\nAmount due on 2026-03-15",
    )?;
    fs::write(
        inbox.join("document (3).txt"),
        "Employment Agreement\nThis agreement is made between the parties on 2026-01-10.",
    )?;
    image::RgbImage::new(64, 40).save(inbox.join("Screenshot 2026-02-01 at 09.30.12.png"))?;
    fs::write(inbox.join("track01.mp3"), b"ID3")?;
    fs::write(
        inbox.join("backup.zip"),
        b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    )?;
    fs::write(inbox.join("ideas.md"), "# Ideas\n- ship the starter")?;
    Ok(())
}

fn step(title: &str) {
    println!("\n── {title}");
}

/// Lists files under `dir`, shown relative to `base`.
fn print_tree(dir: &Path, base: &Path) -> Result<()> {
    let mut files = Vec::new();
    collect(dir, &mut files)?;
    files.sort();
    for file in files {
        println!("   {}", file.strip_prefix(base)?.display());
    }
    Ok(())
}

fn collect(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn count_files(dir: &Path) -> Result<usize> {
    let mut files = Vec::new();
    collect(dir, &mut files)?;
    Ok(files.len())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
