use std::path::Path;

use plec_compiler::read_source_graph;

/// Parse the source graph reachable from `source` and build its semantic graph.
pub fn load(
    source: &Path,
) -> Result<(Vec<plec_parser::ParsedModule>, plec_sema::SemanticGraph), Box<dyn std::error::Error>>
{
    let root_dir = std::env::current_dir()?;

    let source_graph = read_source_graph(source, &root_dir, &root_dir)?;

    let semantic_graph =
        plec_sema::build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)?;

    Ok((source_graph.modules, semantic_graph))
}
