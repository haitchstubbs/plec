use crate::dev;

use clap::{Parser, Subcommand};
use plec_build::{build, BuildOptions};
use plec_compiler::{compile, load, lower_route_manifest, lower_routes};
use plec_inspect::Inspector;
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Inspect {
        source: PathBuf,
        query: String,
    },

    Raw {
        source: PathBuf,
    },

    Routes {
        source: PathBuf,
    },

    /// Compile a routed Plec application into deployable artifacts.
    Build {
        /// Route source entry (e.g. src/router.tsx).
        source: PathBuf,

        /// Build output root; Plec artifacts are emitted under `public/`.
        #[arg(short, long, default_value = "dist")]
        out_dir: PathBuf,

        /// Browser client entry, relative to the app directory.
        #[arg(long, default_value = "src/client.tsx")]
        client_entry: PathBuf,

        /// Server entry, relative to the app directory.
        #[arg(long, default_value = "src/server.ts")]
        server_entry: PathBuf,

        /// Document title for the generated HTML shell.
        #[arg(long, default_value = "Plec app")]
        title: String,

        /// Skip minification of browser/server bundles.
        #[arg(long)]
        no_optimize: bool,
    },

    /// Developer workflow helpers for working on the Plec workspace itself.
    // Hidden `dev` alias: transition shim until the consumer-facing `plec dev`
    // watch + serve loop lands (wasm-runtime-1wj.6), then remove it.
    #[command(alias = "dev")]
    Workspace {
        #[command(subcommand)]
        command: dev::WorkspaceCommand,
    },
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
            let (modules, semantic_graph) = load(&source)?;
            let routes = lower_routes(&modules, &semantic_graph)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&lower_route_manifest(&routes))?
            );
        }

        Command::Build {
            source,
            out_dir,
            client_entry,
            server_entry,
            title,
            no_optimize,
        } => {
            let result = build(BuildOptions {
                source,
                client_entry,
                server_entry,
                out_dir,
                optimize: !no_optimize,
                title,
            })?;
            println!(
                "Plec build complete (revision {}) in {}",
                result.revision,
                result.out_dir.display()
            );
        }

        Command::Workspace { command } => {
            return dev::dispatch(command).map_err(std::convert::Into::into);
        }
    }
    Ok(())
}
