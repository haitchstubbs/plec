use std::{env, path::PathBuf};

use plec_compiler::{
    lower_route_application_to_executable, lower_route_manifest, lower_routes, read_source_graph,
};
use plec_sema::build_semantic_graph;

fn main() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let entry = PathBuf::from(
        args.next()
            .ok_or("usage: plec-route-manifest <entry> <app-root> <repo-root>")?,
    );
    let app_root = PathBuf::from(
        args.next()
            .ok_or("usage: plec-route-manifest <entry> <app-root> <repo-root>")?,
    );
    let repo_root = PathBuf::from(
        args.next()
            .ok_or("usage: plec-route-manifest <entry> <app-root> <repo-root>")?,
    );
    let application = match args.next().as_deref() {
        None => false,
        Some(value) if value == "--application" && args.next().is_none() => true,
        _ => {
            return Err(
                "usage: plec-route-manifest <entry> <app-root> <repo-root> [--application]".into(),
            )
        }
    };

    let source = read_source_graph(entry, app_root, repo_root)?;
    let graph = build_semantic_graph(&source.modules, &source.resolved_imports)
        .map_err(|error| error.to_string())?;
    let routes = lower_routes(&source.modules, &graph).map_err(|error| error.to_string())?;
    let manifest = lower_route_manifest(&routes);
    if !application {
        println!(
            "{}",
            serde_json::to_string(&manifest).map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    let application = lower_route_application_to_executable(&source.modules, &graph, &routes)
        .map_err(|error| format!("unsupported compiled route application: {error}"))?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({ "manifest": manifest, "application": application })
        )
        .map_err(|error| error.to_string())?
    );
    Ok(())
}
