mod id;
mod json_out;

mod build;
mod compile;
mod load;
mod stage;

use build::build;
use compile::compile;
use load::load;

use clap::{Parser, Subcommand};
use plec_compiler::{lower_route_manifest, lower_routes};
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

    /// Compile a routed Plec application into deployable compiler artifacts.
    Build {
        source: PathBuf,

        /// Directory to emit Plec artifacts into.
        #[arg(short, long, default_value = "dist/public")]
        out_dir: PathBuf,
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

        Command::Build { source, out_dir } => {
            build(&source, &out_dir)?;
        }
    }
    Ok(())
}
