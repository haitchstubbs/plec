mod component_discovery;
mod hir_builder;
mod lowering;
mod read_source_graph;
mod routes;

pub use component_discovery::{
    discover_root_component, ComponentDeclaration, ComponentDiscoveryError,
    ReturnedComponentExpression, RootComponent,
};
pub use hir_builder::{lower_application, lower_root_component};
pub use lowering::{
    lower_application_to_executable, lower_component_to_executable,
    lower_route_loader_to_executable, LoweringError,
};
pub use read_source_graph::{read_source_graph, SourceGraph};
pub use routes::{lower_route_application_to_executable, lower_route_manifest, lower_routes, RouteError};
