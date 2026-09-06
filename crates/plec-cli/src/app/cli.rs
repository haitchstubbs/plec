use clap::{Parser, Subcommand};
use plec_build::{build, BuildOptions};
use plec_compiler::{compile, load, lower_route_manifest, lower_routes};
use plec_inspect::Inspector;
use pollster::block_on;
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

        /// Document title; empty defers to `plec.toml`.
        #[arg(long, default_value = "")]
        title: String,

        /// Document description for the generated HTML shell.
        #[arg(long)]
        description: Option<String>,

        /// Stylesheet URL emitted in the document head.
        #[arg(long)]
        styles: Option<String>,

        /// Font preload URL emitted before the stylesheet; repeatable.
        #[arg(long = "preload")]
        preloads: Vec<String>,

        /// Skip minification of browser/server bundles.
        #[arg(long)]
        no_optimize: bool,
    },

    /// Serve a built Plec application with the native host.
    ///
    /// Reads `plec-server.json` from the build output; every path inside
    /// resolves relative to that manifest, so the directory is portable.
    /// When the manifest carries a server bundle, a Node application
    /// runtime is spawned for `/api/*` traffic and shut down with the host.
    Serve {
        /// Build output directory containing `plec-server.json`.
        #[arg(default_value = "dist")]
        dir: PathBuf,

        /// Bind address. Defaults to 127.0.0.1, or 0.0.0.0 in a container.
        #[arg(long)]
        host: Option<String>,

        /// Port; falls back to `$PORT`, then 3000.
        #[arg(long)]
        port: Option<u16>,

        /// Development mode (also enabled unless NODE_ENV=production).
        #[arg(long)]
        development: bool,
    },
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Inspect { source, query } => {
            let application = compile(&source)?;
            let inspector = Inspector::new(&application);
            let result = block_on(inspector.query(&query));
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
            description,
            styles,
            preloads,
            no_optimize,
        } => {
            let options = BuildOptions {
                source,
                client_entry,
                server_entry,
                out_dir,
                optimize: !no_optimize,
                title,
                description,
                styles_href: styles,
                preloads,
            };

            let result = build(options)?;

            println!(
                "Plec build complete (revision {}) in {}",
                result.revision,
                result.out_dir.display()
            );
        }

        Command::Serve {
            dir,
            host,
            port,
            development,
        } => crate::serve::serve(crate::serve::ServeOptions {
            dir,
            host,
            port,
            development,
        })?,
    }
    Ok(())
}
