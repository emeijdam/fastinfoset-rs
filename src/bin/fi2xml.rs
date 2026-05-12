//! `fi2xml <in.fi> [out.xml]` — decode a Fast Infoset document to textual XML.
//!
//! With one argument, writes to stdout. With two, writes to the named file.

use std::env;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: fi2xml <input.fi> [output.xml]");
        return ExitCode::from(2);
    };
    let output: Option<PathBuf> = args.next().map(PathBuf::from);

    let src = match File::open(&input) {
        Ok(f) => BufReader::new(f),
        Err(e) => {
            eprintln!("open {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = if let Some(path) = output {
        match File::create(&path) {
            Ok(f) => fastinfoset::xml::fi_to_xml(src, BufWriter::new(f)),
            Err(e) => {
                eprintln!("create {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        }
    } else {
        let stdout = io::stdout().lock();
        fastinfoset::xml::fi_to_xml(src, BufWriter::new(stdout))
    };

    match result {
        Ok(()) => {
            // Ensure any buffered stdout is flushed.
            let _ = io::stdout().flush();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("fi2xml: {e}");
            ExitCode::FAILURE
        }
    }
}
