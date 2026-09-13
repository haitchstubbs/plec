use clap::{Parser, Subcommand};
use plec_build::{
    build, modules::host::resolve_custom_elements, modules::host::resolve_host_imports,
    BuildOptions, RuntimeSource,
};
use plec_compiler::{
    compile_with_options, load_with_options, lower_route_manifest, lower_routes, CompilerOptions,
};
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
        #[arg(default_value = "src/router.tsx")]
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

        #[arg(long, value_enum, default_value_t = RuntimeSourceArg::Auto)]
        runtime_source: RuntimeSourceArg,
    },

    /// Build and serve an application, rebuilding when source files change.
    Dev {
        #[arg(default_value = "src/router.tsx")]
        source: PathBuf,
        #[arg(short, long, default_value = "dist")]
        out_dir: PathBuf,
        #[arg(long, default_value = "src/client.tsx")]
        client_entry: PathBuf,
        #[arg(long, default_value = "src/server.ts")]
        server_entry: PathBuf,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },

    /// Create a minimal Plec application.
    Init {
        /// Directory to create.
        #[arg(default_value = ".")]
        directory: PathBuf,

        /// Allow initialization in a non-empty directory.
        #[arg(long)]
        force: bool,
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
            let options = compiler_options_for_source(&source)?;
            let application = compile_with_options(&source, &options)?;
            let inspector = Inspector::new(&application);
            let result = block_on(inspector.query(&query));
            for error in &result.errors {
                eprintln!("query error: {error}");
            }
            println!("{}", serde_json::to_string_pretty(&result.data)?);
        }

        Command::Raw { source } => {
            let options = compiler_options_for_source(&source)?;
            let application = compile_with_options(&source, &options)?;
            println!("{}", serde_json::to_string_pretty(&application)?);
        }

        Command::Routes { source } => {
            let options = compiler_options_for_source(&source)?;
            let (modules, semantic_graph) = load_with_options(&source, &options)?;
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
            runtime_source,
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
                runtime_source: runtime_source.into(),
            };

            let result = build(options)?;

            println!(
                "Plec build complete (revision {}) in {}",
                result.revision,
                result.out_dir.display()
            );
        }

        Command::Dev {
            source,
            out_dir,
            client_entry,
            server_entry,
            host,
            port,
        } => crate::dev_loop::run(crate::dev_loop::DevOptions {
            source,
            out_dir,
            client_entry,
            server_entry,
            host,
            port,
        })?,

        Command::Init { directory, force } => crate::app::init::run(directory, force)?,

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

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum RuntimeSourceArg {
    Auto,
    Package,
}

impl From<RuntimeSourceArg> for RuntimeSource {
    fn from(value: RuntimeSourceArg) -> Self {
        match value {
            RuntimeSourceArg::Auto => RuntimeSource::Auto,
            RuntimeSourceArg::Package => RuntimeSource::Package,
        }
    }
}

fn compiler_options_for_source(
    source: &std::path::Path,
) -> Result<CompilerOptions, Box<dyn std::error::Error>> {
    let app_dir = source
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| std::path::Path::new("."));
    Ok(CompilerOptions {
        host_imports: resolve_host_imports(app_dir)?.into_iter().collect(),
        custom_elements: resolve_custom_elements(app_dir)?,
    })
}
