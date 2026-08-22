mod component_discovery;
mod read_source_graph;
mod hir_lowering;

pub use component_discovery::{
    discover_root_component, ComponentDeclaration, ComponentDiscoveryError,
    ReturnedComponentExpression, RootComponent,
};
pub use hir_lowering::{build_component_prop_lookup, lower_root_component, HirComponentPropLookup};
pub use read_source_graph::{read_source_graph, SourceGraph};
