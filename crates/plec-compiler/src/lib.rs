mod component_discovery;
mod read_source_graph;

pub use component_discovery::{
    discover_root_component, ComponentDeclaration, ComponentDiscoveryError,
    ReturnedComponentExpression, RootComponent,
};
pub use read_source_graph::{read_source_graph, SourceGraph};
