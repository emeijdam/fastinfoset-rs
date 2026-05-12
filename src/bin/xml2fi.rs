//! `xml2fi <in.xml> <out.fi>` — encode a textual XML document as Fast Infoset.

use std::env;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: xml2fi <input.xml> <output.fi>");
        return ExitCode::from(2);
    };

    let src = match File::open(&input) {
        Ok(f) => BufReader::new(f),
        Err(e) => {
            eprintln!("open {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let dst = match File::create(&output) {
        Ok(f) => BufWriter::new(f),
        Err(e) => {
            eprintln!("create {output}: {e}");
            return ExitCode::FAILURE;
        }
    };

    match fastinfoset::xml::xml_to_fi(src, dst) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xml2fi: {e}");
            ExitCode::FAILURE
        }
    }
}
