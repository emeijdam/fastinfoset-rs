//! Streaming event model — what the reader yields and the writer accepts.

use crate::algorithm::AlgorithmData;
use crate::name::{NamespaceDecl, QualifiedName};

/// One node in the XML Information Set, in document order.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Document Information Item — emitted exactly once at the start.
    StartDocument,
    /// Matching end of `StartDocument`.
    EndDocument,

    /// Element Information Item — `<name xmlns:p="..." attr="...">`.
    StartElement {
        name: QualifiedName,
        namespaces: Vec<NamespaceDecl>,
        attributes: Vec<Attribute>,
    },
    EndElement,

    /// Character Information Items — text content.
    Characters(CharacterChunk),

    /// `<?target data?>`
    ProcessingInstruction { target: String, data: String },

    /// `<!-- ... -->` — only present when comments are not skipped.
    Comment(String),

    /// Document Type Declaration, if present.
    DocumentTypeDeclaration {
        system_id: Option<String>,
        public_id: Option<String>,
    },

    /// Unexpanded entity reference.
    UnexpandedEntityReference {
        name: String,
        system_id: Option<String>,
        public_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: QualifiedName,
    pub value: CharacterChunk,
}

/// Character data — either a plain string or content encoded with one of
/// the built-in or application-defined encoding algorithms (§ 10).
#[derive(Debug, Clone, PartialEq)]
pub enum CharacterChunk {
    Text(String),
    Encoded(AlgorithmData),
}

impl From<String> for CharacterChunk {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for CharacterChunk {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}
