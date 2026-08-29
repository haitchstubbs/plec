use crate::load::load;
use plec_compiler::{discover_root_component, lower_application, lower_application_to_executable};
use plec_ir::ComponentApplication;
use std::path::Path;
pub fn compile(source: &Path) -> Result<ComponentApplication, Box<dyn std::error::Error>> {
    let (modules, semantic_graph) = load(source)?;

    // read_source_graph guarantees modules[0] is the entry module.
    let entry_module_id = modules[0].id.clone();

    let root = discover_root_component(&modules, &semantic_graph, &entry_module_id, None)?;

    let hir = lower_application(&modules, &root, &semantic_graph)?;

    Ok(lower_application_to_executable(&hir)?)
}
