/// Canonical source identity for a component declaration.
///
/// `module_id` is the portable module identity assigned by the source graph;
/// it is never an absolute filesystem path. `local_name` is the defining
/// declaration, so imports and re-exports do not create new component IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentId {
    pub module_id: String,
    pub local_name: String,
}

impl ComponentId {
    pub fn new(module_id: impl Into<String>, local_name: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            local_name: local_name.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

/// Component-local semantic identity for parameters and lexical bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub u32);
