use clap::{Parser, Subcommand};
use plec_compiler::{
    discover_root_component, lower_application, lower_application_to_executable, lower_route_manifest,
    lower_routes, read_source_graph,
};
use plec_ir::ComponentApplication;
use std::path::PathBuf;

use plec_inspect::Inspector;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Inspect { source: String, query: String },
    Raw { source: String },
    Routes { source: String },
}

/// Parse the source graph reachable from `source` and build its semantic graph.
fn load_semantic_graph(
    source: &str,
) -> Result<
    (
        Vec<plec_parser::ParsedModule>,
        plec_sema::SemanticGraph,
    ),
    Box<dyn std::error::Error>,
> {
    let entry = PathBuf::from(source);
    let root_dir = std::env::current_dir()?;

    let source_graph = read_source_graph(&entry, &root_dir, &root_dir)?;

    let semantic_graph =
        plec_sema::build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)?;

    Ok((source_graph.modules, semantic_graph))
}

/// Parse, type-check, discover the entry component, and lower it to the
/// executable component graph.
fn compile(source: &str) -> Result<ComponentApplication, Box<dyn std::error::Error>> {
    let (modules, semantic_graph) = load_semantic_graph(source)?;

    // read_source_graph guarantees modules[0] is the entry module.
    let entry_module_id = modules[0].id.clone();

    let root = discover_root_component(&modules, &semantic_graph, &entry_module_id, None)?;

    let hir = lower_application(&modules, &root, &semantic_graph)?;

    Ok(lower_application_to_executable(&hir)?)
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Inspect { source, query } => {
            let application = compile(&source)?;

            let inspector = Inspector::new(&application);

            let result = pollster::block_on(inspector.query(&query));

            for error in &result.errors {
                eprintln!("query error: {error}");
            }

            println!("{}", serde_json::to_string_pretty(&result.data)?);
        }
        Command::Raw { source } => {
            let application = compile(&source)?;

            println!("{}", serde_json::to_string_pretty(&application)?);
        }
        Command::Routes { source } => {
            let (modules, semantic_graph) = load_semantic_graph(&source)?;

            let routes = lower_routes(&modules, &semantic_graph)?;

            println!("{}", serde_json::to_string_pretty(&lower_route_manifest(&routes))?);
        }
    }

    Ok(())
}
