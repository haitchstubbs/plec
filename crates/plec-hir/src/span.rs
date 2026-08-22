#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    /// Canonical source-graph module ID, not a filesystem path.
    pub module_id: String,
    pub start: u32,
    pub end: u32,
}

impl SourceSpan {
    pub fn new(module_id: impl Into<String>, start: u32, end: u32) -> Self {
        Self {
            module_id: module_id.into(),
            start,
            end,
        }
    }
}
