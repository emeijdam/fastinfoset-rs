# fastinfoset-rs

Pure-Rust streaming reader and writer for **Fast Infoset** — the binary
encoding of the XML Information Set defined by
[ITU-T X.891 / ISO/IEC 24824-1][spec].

[spec]: https://www.itu.int/rec/T-REC-X.891

[![Crates.io](https://img.shields.io/crates/v/fastinfoset-rs.svg)](https://crates.io/crates/fastinfoset-rs)
[![Documentation](https://docs.rs/fastinfoset-rs/badge.svg)](https://docs.rs/fastinfoset-rs)
[![License](https://img.shields.io/crates/l/fastinfoset-rs.svg)](#license)

- No FFI, no `unsafe` (`unsafe_code = "forbid"`).
- Pull-style `Reader::next_event()` and event-driven `Writer::write_event()`.
- Optional `xml` feature (default-on) bridges to `quick-xml` for textual
  XML in / textual XML out.
- Two CLI binaries: `fi2xml` and `xml2fi`.

The crate is published as `fastinfoset-rs` and imported as `fastinfoset`:

```toml
[dependencies]
fastinfoset-rs = "0.1"
```

```rust
use fastinfoset::{Reader, Writer, Event};
```

## Install the CLIs

```bash
cargo install fastinfoset-rs
# now: fi2xml <in.fi> [out.xml]
#      xml2fi <in.xml> <out.fi>
```

## Quick start — text in, text out

```rust
use std::fs::File;
use std::io::{BufReader, BufWriter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Fast Infoset → XML text
    let src = BufReader::new(File::open("input.fi")?);
    let dst = BufWriter::new(File::create("output.xml")?);
    fastinfoset::xml::fi_to_xml(src, dst)?;

    // XML text → Fast Infoset
    let src = BufReader::new(File::open("edited.xml")?);
    let dst = BufWriter::new(File::create("edited.fi")?);
    fastinfoset::xml::xml_to_fi(src, dst)?;

    Ok(())
}
```

## Event-level access

If you need to filter mid-stream rather than round-tripping through text:

```rust
use fastinfoset::{Reader, Event};

let mut reader = Reader::new(std::fs::File::open("input.fi")?);
while let Some(event) = reader.next_event()? {
    match event {
        Event::StartElement { name, attributes, .. } => {
            println!("<{}> with {} attrs", name.local_name, attributes.len());
        }
        Event::Characters(chunk) => { /* … */ }
        _ => {}
    }
}
# Ok::<_, fastinfoset::Error>(())
```

The symmetric `Writer::write_event(&Event)` API is available for emitting
documents.

## Status

The crate handles the production subset needed to round-trip real-world
Fast Infoset documents that don't use namespaces, alternative encodings, or
encoding algorithms. Specifically:

| Feature | Reader | Writer |
|---|---|---|
| Document header + presence flags | ✓ | ✓ (empty flags) |
| Elements — literal and indexed names | ✓ | ✓ |
| Attributes — literal and indexed names | ✓ | ✓ |
| Local-name / attribute-value / CII vocabularies | ✓ | ✓ |
| UTF-8 character chunks (small / medium / large) | ✓ | ✓ |
| Single & double terminators | ✓ | single only |
| Namespace declarations | — | — |
| UTF-16, restricted alphabets, encoding algorithms | — | — |
| PI, comment, DTD, unexpanded entity refs | — | — |

Unimplemented productions return `Error::Malformed` (reader) or
`Error::Other` (writer) with a specific message identifying which
production was hit. Contributions welcome.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  https://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
