//! Text-XML conversion helpers, gated on the `xml` feature.
//!
//! Bridges the FI event model to `quick-xml`, so downstream projects can stay
//! decoupled from FI internals — they feed XML in and get XML out.

use std::io::{Read, Write};

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event as QXmlEvent};
use quick_xml::writer::Writer as QXmlWriter;

use crate::error::{Error, Result};
use crate::event::{CharacterChunk, Event};
use crate::name::QualifiedName;
use crate::reader::Reader;

/// Decode a Fast Infoset document from `src` and write its textual XML
/// serialization to `dst`. UTF-8 output, no XML declaration emitted.
pub fn fi_to_xml<R: Read, W: Write>(src: R, dst: W) -> Result<()> {
    let mut reader = Reader::new(src);
    let mut writer = QXmlWriter::new(dst);
    let mut open: Vec<String> = Vec::new();

    while let Some(event) = reader.next_event()? {
        match event {
            Event::StartDocument | Event::EndDocument => {}
            Event::StartElement {
                name,
                namespaces,
                attributes,
            } => {
                let qname = qualified_name_string(&name);
                let mut elem = BytesStart::new(qname.as_str());
                for ns in &namespaces {
                    let attr_name = match &ns.prefix {
                        Some(p) => format!("xmlns:{p}"),
                        None => "xmlns".to_owned(),
                    };
                    elem.push_attribute((attr_name.as_str(), ns.namespace_name.as_str()));
                }
                for attr in &attributes {
                    let name = qualified_name_string(&attr.name);
                    let value = character_chunk_text(&attr.value)?;
                    elem.push_attribute((name.as_str(), value.as_str()));
                }
                writer.write_event(QXmlEvent::Start(elem)).map_err(Error::Io)?;
                open.push(qname);
            }
            Event::EndElement => {
                let name = open
                    .pop()
                    .ok_or_else(|| Error::Other("EndElement with no matching open tag".into()))?;
                writer
                    .write_event(QXmlEvent::End(BytesEnd::new(name)))
                    .map_err(Error::Io)?;
            }
            Event::Characters(chunk) => {
                let text = character_chunk_text(&chunk)?;
                writer
                    .write_event(QXmlEvent::Text(BytesText::new(&text)))
                    .map_err(Error::Io)?;
            }
            Event::ProcessingInstruction { .. }
            | Event::Comment(_)
            | Event::DocumentTypeDeclaration { .. }
            | Event::UnexpandedEntityReference { .. } => {
                return Err(Error::Other(
                    "PI / comment / DTD / entity-ref to XML not yet implemented".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Encode a textual XML document from `src` into a Fast Infoset stream on `dst`.
pub fn xml_to_fi<R: Read, W: Write>(src: R, dst: W) -> Result<()> {
    use quick_xml::events::Event as QXmlIn;
    use quick_xml::reader::Reader as QXmlReader;

    let mut reader = QXmlReader::from_reader(std::io::BufReader::new(src));
    reader.config_mut().trim_text(false);
    let mut writer = crate::writer::Writer::new(dst);

    writer.write_event(&Event::StartDocument)?;

    let mut buf = Vec::new();
    loop {
        buf.clear();
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| Error::Other(format!("quick-xml read: {e}")))?;
        match ev {
            QXmlIn::Eof => break,
            QXmlIn::Decl(_) => {} // ignored
            QXmlIn::Start(e) => {
                let (name, attrs) = parse_start(&e)?;
                writer.write_event(&Event::StartElement {
                    name,
                    namespaces: Vec::new(),
                    attributes: attrs,
                })?;
            }
            QXmlIn::Empty(e) => {
                let (name, attrs) = parse_start(&e)?;
                writer.write_event(&Event::StartElement {
                    name,
                    namespaces: Vec::new(),
                    attributes: attrs,
                })?;
                writer.write_event(&Event::EndElement)?;
            }
            QXmlIn::End(_) => {
                writer.write_event(&Event::EndElement)?;
            }
            QXmlIn::Text(t) => {
                let s = t.xml10_content();
                if !s.is_empty() {
                    writer.write_event(&Event::Characters(CharacterChunk::Text(
                        s.into_owned(),
                    )))?;
                }
            }
            // quick-xml 0.38+ hands `&amp;`, `&#10;` and the like as their own
            // events; text between them is already literal.
            QXmlIn::GeneralRef(r) => {
                let s = resolve_ref(&r)?;
                writer.write_event(&Event::Characters(CharacterChunk::Text(s)))?;
            }
            QXmlIn::CData(c) => {
                let s = c.into_inner();
                if !s.is_empty() {
                    writer.write_event(&Event::Characters(CharacterChunk::Text(
                        s.into_owned(),
                    )))?;
                }
            }
            QXmlIn::Comment(_) | QXmlIn::PI(_) | QXmlIn::DocType(_) => {
                return Err(Error::Other(
                    "writer: comment/PI/DocType not yet supported".into(),
                ));
            }
        }
    }
    writer.write_event(&Event::EndDocument)?;
    writer.finish()?;
    Ok(())
}

/// A character reference or one of XML's five predefined entities, as text.
fn resolve_ref(r: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    if let Some(c) = r
        .resolve_char_ref()
        .map_err(|e| Error::Other(format!("character reference: {e}")))?
    {
        return Ok(c.to_string());
    }
    quick_xml::escape::resolve_predefined_entity(r.as_ref())
        .map(str::to_owned)
        .ok_or_else(|| Error::Other(format!("unknown entity &{};", r.as_ref())))
}

fn parse_start(
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<(QualifiedName, Vec<crate::event::Attribute>)> {
    let qname = QualifiedName::local(e.name().as_ref().to_owned());

    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::Other(format!("attribute parse: {e}")))?;
        let n = a.key.as_ref().to_owned();
        let v = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| Error::Other(format!("attr decode: {e}")))?
            .into_owned();
        attrs.push(crate::event::Attribute {
            name: QualifiedName::local(n),
            value: CharacterChunk::Text(v),
        });
    }
    Ok((qname, attrs))
}

fn qualified_name_string(name: &QualifiedName) -> String {
    match &name.prefix {
        Some(p) if !p.is_empty() => format!("{p}:{}", name.local_name),
        _ => name.local_name.clone(),
    }
}

fn character_chunk_text(chunk: &CharacterChunk) -> Result<String> {
    match chunk {
        CharacterChunk::Text(s) => Ok(s.clone()),
        CharacterChunk::Encoded(_) => Err(Error::Other(
            "character chunk encoded with an algorithm — not yet representable as XML text".into(),
        )),
    }
}

