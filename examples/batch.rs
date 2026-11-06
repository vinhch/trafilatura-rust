//! Extracts every HTML file in a directory with one shared `Extractor`, four worker threads and a
//! 30-second batch deadline, and prints one line per file: `index path outcome chars`.
//!
//! Run it as `cargo run --example batch -- <dir>`. Files are taken in name order, so the indexes
//! are stable from run to run.

use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use trafilatura_rust::{
    BatchEnd, BatchOptions, Call, Document, ExtractError, Input, Options, Skipped,
};

/// What a worker returns for one file: the read error, or the extraction's result.
type Page = io::Result<Result<Document, ExtractError>>;

fn main() -> ExitCode {
    let Some(dir) = std::env::args_os().nth(1) else {
        eprintln!("usage: batch <dir>");
        return ExitCode::from(2);
    };
    match run(Path::new(&dir)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("batch: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let paths = html_files(dir)?;
    let extractor = Options::new().build();
    let threads = NonZeroUsize::new(4).ok_or("thread count is zero")?;
    // The deadline covers the whole batch: files not yet pulled when it passes are skipped.
    let call = Call::default().deadline(Instant::now() + Duration::from_secs(30));

    // One slot per file, filled as results arrive: the collect pattern. It holds a label per file,
    // not the extracted text, so it costs little; a caller who keeps the text pays for it in memory.
    let mut labels: Vec<Option<&'static str>> = vec![None; paths.len()];
    let status = extractor.extract_batch(
        paths.iter(),
        BatchOptions::new(threads),
        &call,
        |path: &PathBuf, call: &Call<'_>| -> Page {
            let bytes = fs::read(path)?;
            Ok(extractor.bare_extraction(&Input::bytes(&bytes), call))
        },
        |index, result| {
            let (outcome, chars) = describe(result);
            if let Some(slot) = labels.get_mut(index) {
                *slot = Some(outcome);
            }
            if let Some(path) = paths.get(index) {
                let chars = chars.map_or_else(|| "-".to_owned(), |count| count.to_string());
                println!("{index} {} {outcome} {chars}", path.display());
            }
        },
    )?;

    if status != BatchEnd::Completed {
        let pulled = labels.iter().flatten().count();
        return Err(format!(
            "stopped early: {pulled} of {} files were pulled",
            paths.len()
        )
        .into());
    }
    Ok(())
}

/// The `.html` files directly in `dir`, sorted by name.
fn html_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "html")
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// The output label of one result, and the character count of its text when there is one.
fn describe(result: Result<Page, Skipped>) -> (&'static str, Option<usize>) {
    match result {
        Ok(Ok(Ok(document))) => (
            "ok",
            document.text.as_ref().map(|text| text.chars().count()),
        ),
        Ok(Ok(Err(_))) => ("rejected", None),
        Ok(Err(_)) => ("io-error", None),
        Err(Skipped::Cancelled) => ("cancelled", None),
        Err(Skipped::DeadlineExceeded) => ("deadline", None),
        Err(_) => ("skipped", None),
    }
}
