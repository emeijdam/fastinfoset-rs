//! Diagnostic tool: open a Fast Infoset document and report what we can decode.

use std::env;
use std::fs::File;
use std::io::BufReader;
use std::process::ExitCode;

use fastinfoset::Reader;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: fi-dump <path-to-fi-document>");
        return ExitCode::from(2);
    };

    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut reader = Reader::new(BufReader::new(file));
    let mut event_count = 0u64;

    loop {
        match reader.next_event() {
            Ok(Some(event)) => {
                event_count += 1;
                println!(
                    "@octet {:>6}  {event:?}",
                    reader.octet_position()
                );
            }
            Ok(None) => break,
            Err(e) => {
                println!("@octet {:>6}  ERROR: {e}", reader.octet_position());
                eprintln!(
                    "stopped after {event_count} events at octet {}",
                    reader.octet_position()
                );
                return ExitCode::FAILURE;
            }
        }
    }

    eprintln!("decoded {event_count} events");
    ExitCode::SUCCESS
}
