//! Qualified names and namespace declarations (§ 7.2.10 / 7.4.1 of X.891).

/// A fully-resolved qualified name — `{namespace}local` plus an optional prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct QualifiedName {
    pub prefix: Option<String>,
    pub namespace_name: Option<String>,
    pub local_name: String,
}

impl QualifiedName {
    pub fn local(name: impl Into<String>) -> Self {
        Self {
            prefix: None,
            namespace_name: None,
            local_name: name.into(),
        }
    }

    pub fn qualified(
        prefix: Option<String>,
        namespace_name: Option<String>,
        local_name: impl Into<String>,
    ) -> Self {
        Self {
            prefix,
            namespace_name,
            local_name: local_name.into(),
        }
    }
}

/// A namespace declaration attached to a start-element.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamespaceDecl {
    pub prefix: Option<String>,
    pub namespace_name: String,
}
