//! Browser execution contracts.
//!
//! Browser automation is a first-class capability of Fawx OS. This crate
//! defines the typed surface the harness can depend on without coupling itself
//! to a specific browser engine.

/// A browser action request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserAction {
    pub target: String,
    pub action: String,
}
