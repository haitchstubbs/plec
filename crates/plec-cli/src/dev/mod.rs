//! Dev-CLI workflow helpers.
//!
//! These commands encode the validation and investigation workflows of the
//! Plec workspace itself: running and querying the WASM suite, checking the
//! serialized-protocol contracts, tracing symbols and adoption error codes,
//! inspecting artifact provenance, and health-checking SSR adoption. They
//! exist so agents and developers stop rebuilding repo topology, protocol
//! invariants, and test-failure context by hand.
//!
//! This module belongs to the **dev** CLI frontend only — the release (app)
//! frontend never exposes `plec workspace`.

pub mod artifact;
pub mod cli;
pub mod compile;
pub mod context;
pub mod contract;
pub mod doctor;
pub mod graph;
pub mod limits;
pub mod markers;
pub mod repo;
pub mod trace;
pub mod verify;
pub mod wasm_section;
pub mod wasmtest;

use clap::Subcommand;
use repo::Repo;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum WorkspaceCommand {
    /// Compile the Plec-owned runtime WASM artifact (wasm-pack pipeline).
    Compile {
        /// Toolchain profile compiled into the artifact.
        #[arg(long, value_enum, default_value_t = compile::CompileProfile::Full)]
        profile: compile::CompileProfile,

        /// Extra cargo features, comma-separated.
        #[arg(long)]
        features: Option<String>,

        /// Skip wasm-tools optimization.
        #[arg(long)]
        no_optimize: bool,

        /// Print the build summary as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Run and query the WASM browser test suite.
    Test {
        #[command(subcommand)]
        command: TestCommand,
    },

    /// Inspect built/staged runtime artifacts for staleness.
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommand,
    },

    /// Check serialized-protocol constants across the repo.
    Contract {
        #[command(subcommand)]
        command: ContractCommand,
    },

    /// Emit a condensed architecture packet for a workspace domain.
    Context {
        /// Domain to inspect: ssr-adoption, routing, typed-events, cookies, artifacts.
        #[arg(value_enum)]
        domain: context::Domain,

        /// Print the packet as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Categorized search for a symbol or adoption error code.
    Trace {
        /// Symbol, error code, or identifier to trace.
        query: String,

        /// Emit the full result as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Health-check a pipeline end to end.
    Doctor {
        #[command(subcommand)]
        command: DoctorCommand,
    },

    /// Run the complete SSR adoption validation matrix.
    Verify {
        #[command(subcommand)]
        command: VerifyCommand,
    },

    /// Inspect compiled graph registry resolution and node structure.
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },

    /// Explain and validate executable DOM marker addresses.
    Markers {
        #[command(subcommand)]
        command: MarkersCommand,
    },
}

#[derive(Subcommand)]
pub enum MarkersCommand {
    /// Explain structural descent encoded by an address.
    Explain {
        address: String,
        #[arg(long)]
        json: bool,
    },
    /// Compare HTML markers with markers required by a compiled graph.
    Validate {
        #[arg(long)]
        graph: String,
        #[arg(long)]
        html: PathBuf,
        #[arg(long)]
        source: Option<PathBuf>,
        /// Captured `PlecSsrSnapshot` selecting branches and keyed rows.
        #[arg(long)]
        snapshot: Option<PathBuf>,
        /// Matched route path (e.g. `/about`) choosing outlet children.
        #[arg(long)]
        route: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum TestCommand {
    /// Run the WASM suite once, capture the output, and report failures.
    Wasm {
        /// Optional test-name filters forwarded to wasm-pack.
        filters: Vec<String>,

        /// Print only the parsed failure summary.
        #[arg(long)]
        failures: bool,

        /// Print the captured report as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Query the most recent captured run (no re-run).
    Last {
        /// Show only failures whose names contain this substring.
        #[arg(long)]
        failure: Option<String>,

        /// Print the captured report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ArtifactCommand {
    /// Full provenance report for a runtime artifact.
    Provenance {
        /// Artifact to inspect (currently: runtime).
        #[arg(default_value = "runtime")]
        target: String,

        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Terse staleness gate over every pipeline layer.
    Stale {
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ContractCommand {
    /// SSR protocol versions: snapshot, bootstrap, manifest.
    Ssr {
        /// Exit non-zero on any conflicting hard-coded version.
        #[arg(long)]
        check: bool,

        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Generate and check browser resource-limit constants.
    Limits {
        /// Exit non-zero when the checked-in module differs from Rust.
        #[arg(long)]
        check: bool,

        /// Rewrite the checked-in generated TypeScript module.
        #[arg(long)]
        write: bool,

        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DoctorCommand {
    /// SSR adoption health: protocol, graph resolution, markers, artifacts.
    Adoption {
        /// Application source entry (default: the fullstack app).
        #[arg(long)]
        source: Option<PathBuf>,

        /// Only resolve graphs for routes with this path.
        #[arg(long)]
        route: Option<String>,

        /// Validate DOM markers in this HTML file.
        #[arg(long)]
        html: Option<PathBuf>,

        /// Resolve graph references in a captured PlecSsrSnapshot JSON file.
        #[arg(long)]
        snapshot: Option<PathBuf>,

        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum VerifyCommand {
    /// Run Rust, WASM, browser, typecheck, E2E, and artifact gates.
    Adoption {
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum GraphCommand {
    /// Resolve a graph id through the runtime's registry rules.
    Resolve {
        /// Graph id to resolve.
        graph_id: String,

        /// Application source entry (default: the fullstack app).
        #[arg(long)]
        source: Option<PathBuf>,

        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Print a resolved component's compact node structure.
    Tree {
        /// Graph id to resolve and render.
        graph_id: String,

        /// Application source entry (default: the fullstack app).
        #[arg(long)]
        source: Option<PathBuf>,

        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub fn dispatch(command: WorkspaceCommand) -> Result<(), String> {
    let repo = Repo::discover()?;

    match command {
        WorkspaceCommand::Compile {
            profile,
            features,
            no_optimize,
            json,
        } => {
            let report = compile::compile(
                &repo,
                &compile::CompileOptions {
                    profile,
                    features,
                    no_optimize,
                },
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
            } else {
                compile::print_report(&report);
            }
            Ok(())
        }
        WorkspaceCommand::Test { command } => dispatch_test(&repo, command),
        WorkspaceCommand::Artifact { command } => dispatch_artifact(&repo, command),
        WorkspaceCommand::Contract { command } => dispatch_contract(&repo, command),
        WorkspaceCommand::Context { domain, json } => {
            let report = context::build(&repo, domain)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize context report: {error}"))?
                );
            } else {
                context::print_report(&report);
            }
            Ok(())
        }
        WorkspaceCommand::Trace { query, json } => {
            let report = trace::trace(&repo, &query);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
            } else {
                trace::print_report(&report);
            }
            Ok(())
        }
        WorkspaceCommand::Doctor { command } => dispatch_doctor(&repo, command),
        WorkspaceCommand::Verify { command } => dispatch_verify(&repo, command),
        WorkspaceCommand::Graph { command } => dispatch_graph(&repo, command),
        WorkspaceCommand::Markers { command } => dispatch_markers(&repo, command),
    }
}

fn dispatch_markers(repo: &Repo, command: MarkersCommand) -> Result<(), String> {
    match command {
        MarkersCommand::Explain { address, json } => {
            let report = markers::explain(&address)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                );
            } else {
                markers::print_explain(&report);
            }
            Ok(())
        }
        MarkersCommand::Validate {
            graph,
            html,
            source,
            snapshot,
            route,
            json,
        } => {
            let report = markers::validate(
                repo,
                &source.unwrap_or_else(|| repo.default_app_source()),
                &markers::ValidateOptions {
                    graph: &graph,
                    html: &html,
                    snapshot: snapshot.as_deref(),
                    route: route.as_deref(),
                },
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                );
            } else {
                markers::print_validation(&report);
            }
            if report.ok {
                Ok(())
            } else {
                Err("marker validation failed".into())
            }
        }
    }
}

fn dispatch_verify(repo: &Repo, command: VerifyCommand) -> Result<(), String> {
    match command {
        VerifyCommand::Adoption { json } => {
            let report = verify::run(repo);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
            } else {
                verify::print_report(&report);
            }
            if report.ok {
                Ok(())
            } else {
                Err("adoption verification failed".into())
            }
        }
    }
}

fn dispatch_test(repo: &Repo, command: TestCommand) -> Result<(), String> {
    match command {
        TestCommand::Wasm {
            filters,
            failures,
            json,
        } => {
            let report = wasmtest::run_wasm_tests(repo, &filters)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
            } else {
                wasmtest::print_report(&report, failures);
            }

            if report.ok() {
                Ok(())
            } else {
                Err(format!(
                    "{} of {} WASM tests failed (captured: plec workspace test last)",
                    report.failed,
                    report.passed + report.failed
                ))
            }
        }

        TestCommand::Last { failure, json } => {
            let report = wasmtest::load_last_report(repo)?;

            if let Some(failure) = &failure {
                let matches: Vec<_> = report
                    .failures
                    .iter()
                    .filter(|item| item.name.contains(failure))
                    .collect();
                if matches.is_empty() {
                    println!(
                        "no captured failure matching {failure:?} ({} failures recorded)",
                        report.failures.len()
                    );
                } else {
                    for item in matches {
                        println!("{}", item.name);
                        if let (Some(file), Some(line)) = (&item.file, item.line) {
                            println!("  {file}:{line}");
                        }
                        println!("  {}\n", item.message.lines().next().unwrap_or(""));
                    }
                }
                return Ok(());
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
            } else {
                println!(
                    "captured run: {} passed, {} failed ({} ms ago, filters: {})",
                    report.passed,
                    report.failed,
                    report.duration_ms,
                    if report.filters.is_empty() {
                        "none".into()
                    } else {
                        report.filters.join(", ")
                    }
                );
                wasmtest::print_failures(&report);
            }
            Ok(())
        }
    }
}

fn dispatch_artifact(repo: &Repo, command: ArtifactCommand) -> Result<(), String> {
    match command {
        ArtifactCommand::Provenance { target, json } => {
            if target != "runtime" {
                return Err(format!(
                    "unknown artifact {target:?} — currently supported: runtime"
                ));
            }
            let report = artifact::inspect(repo);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
                Ok(())
            } else {
                if artifact::print_provenance(&report, repo) {
                    Ok(())
                } else {
                    Err("artifact provenance problems found".into())
                }
            }
        }

        ArtifactCommand::Stale { json } => {
            let report = artifact::inspect(repo);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
                Ok(())
            } else if artifact::print_stale(&report) {
                Ok(())
            } else {
                Err("stale or missing artifacts — see details above".into())
            }
        }
    }
}

fn dispatch_contract(repo: &Repo, command: ContractCommand) -> Result<(), String> {
    match command {
        ContractCommand::Ssr { check, json } => {
            let report = contract::scan(repo).map_err(|error| error)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
                return Ok(());
            }

            let ok = contract::print_report(&report);
            if check && !ok {
                Err("protocol contract conflicts found (--check)".into())
            } else {
                Ok(())
            }
        }
        ContractCommand::Limits { check, write, json } => limits::run(repo, check, write, json),
    }
}

fn dispatch_doctor(repo: &Repo, command: DoctorCommand) -> Result<(), String> {
    match command {
        DoctorCommand::Adoption {
            source,
            route,
            html,
            snapshot,
            json,
        } => {
            let options = doctor::DoctorOptions {
                source: source.unwrap_or_else(|| repo.default_app_source()),
                route,
                html,
                snapshot,
            };

            let report = doctor::run(repo, &options)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
                return Ok(());
            }

            doctor::print_report(&report);
            if report.ok {
                Ok(())
            } else {
                Err("adoption doctor found problems".into())
            }
        }
    }
}

fn dispatch_graph(repo: &Repo, command: GraphCommand) -> Result<(), String> {
    match command {
        GraphCommand::Resolve {
            graph_id,
            source,
            json,
        } => {
            let report = graph::resolve(
                repo,
                &source.unwrap_or_else(|| repo.default_app_source()),
                &graph_id,
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
            } else {
                graph::print_resolve(&report);
            }
            if report.found() {
                Ok(())
            } else {
                Err(format!("graph {graph_id:?} is not registered"))
            }
        }
        GraphCommand::Tree {
            graph_id,
            source,
            json,
        } => {
            let report = graph::tree(
                repo,
                &source.unwrap_or_else(|| repo.default_app_source()),
                &graph_id,
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("serialize: {error}"))?
                );
            } else {
                graph::print_tree(&report);
            }
            if report.found() {
                Ok(())
            } else {
                Err(format!("graph {graph_id:?} is not registered"))
            }
        }
    }
}
