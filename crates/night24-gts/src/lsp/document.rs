//! In-memory document store for the language server.
//!
//! Tracks open documents (`textDocument/didOpen`, `/didChange`, `/didClose`)
//! keyed by URI. Supports full-document text sync (the simplest LSP sync mode);
//! range/Incremental sync is out of MVP scope.

use std::collections::HashMap;

/// An open document: its URI and full text.
#[derive(Debug, Clone)]
pub struct Document {
    pub uri: String,
    pub text: String,
}

/// Maps document URI → [`Document`].
#[derive(Debug, Default)]
pub struct DocumentStore {
    docs: HashMap<String, Document>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open (or replace) a document.
    pub fn open(&mut self, uri: impl Into<String>, text: impl Into<String>) {
        let uri = uri.into();
        self.docs.insert(
            uri.clone(),
            Document {
                uri,
                text: text.into(),
            },
        );
    }

    /// Apply a full-document change (`textDocument/didChange` with full text).
    pub fn update(&mut self, uri: &str, text: impl Into<String>) {
        if let Some(doc) = self.docs.get_mut(uri) {
            doc.text = text.into();
        }
    }

    /// Close a document.
    pub fn close(&mut self, uri: &str) {
        self.docs.remove(uri);
    }

    /// Get a document's text, if open.
    pub fn get(&self, uri: &str) -> Option<&str> {
        self.docs.get(uri).map(|d| d.text.as_str())
    }

    /// Iterate all open documents.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Document)> {
        self.docs.iter()
    }
}
