use super::api_routes;
use super::artifacts;
use super::assets;
use super::clean;
use super::document;
use super::host;
use super::server;

use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Deserialize)]
struct ViteClientResult {
    entry: String,
    styles: Vec<String>,
    providers: BTreeMap<String, String>,
}

fn run_vite_client(
    app_dir: &Path,
    entry: &Path,
    out_dir: &Path,
    providers: &BTreeMap<String, std::collections::BTreeSet<String>>,
    adapters: &BTreeMap<String, String>,
    optimize: bool,
) -> Result<ViteClientResult, BuildError> {
    let providers = providers
        .iter()
        .map(|(id, components)| {
            let adapter = adapters.get(id).ok_or_else(|| {
                BuildError::new(
                    Stage::BrowserBundle,
                    format!("host provider {id:?} has no configured browser adapter"),
                )
            })?;
            Ok((
                id.clone(),
                serde_json::json!({ "adapter": adapter, "components": components }),
            ))
        })
        .collect::<Result<serde_json::Map<_, _>, BuildError>>()?;
    let request = serde_json::json!({
        "root": app_dir,
        "entry": entry,
        "outDir": out_dir,
        "providers": providers,
        "optimize": optimize,
    });
    let script = "import '@plec/core/vite-build';";
    let mut child = Command::new("node")
        .arg("--input-type=module")
        .arg("-e")
        .arg(script)
        .current_dir(app_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            BuildError::with_source(
                Stage::BrowserBundle,
                "failed to start @plec/vite production adapter",
                error,
            )
        })?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(request.to_string().as_bytes())
        .map_err(|error| {
            BuildError::with_source(
                Stage::BrowserBundle,
                "failed to send Vite build configuration",
                error,
            )
        })?;
    let output = child.wait_with_output().map_err(|error| {
        BuildError::with_source(
            Stage::BrowserBundle,
            "failed waiting for Vite production build",
            error,
        )
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let (stage, detail) = if let Some(detail) = detail
            .split_once("[PLEC-DEPENDENCY-VALIDATION]")
            .map(|(_, detail)| detail.trim())
        {
            (Stage::DependencyValidation, detail)
        } else {
            (Stage::BrowserBundle, detail.trim())
        };
        return Err(BuildError::new(
            stage,
            format!("Vite production build failed: {detail}"),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        BuildError::with_source(
            Stage::BrowserBundle,
            "Vite adapter returned an invalid Plec build result",
            error,
        )
    })
}

/// Configuration for the shared application build pipeline.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Route source entry (e.g. `src/router.tsx`).
    pub source: PathBuf,
    /// Browser client entry (e.g. `src/client.tsx`).
    pub client_entry: PathBuf,
    /// Server entry (e.g. `src/server.ts`).
    pub server_entry: PathBuf,
    /// Build output root. Browser artifacts are separated into `client/`.
    pub out_dir: PathBuf,
    pub optimize: bool,
    /// Document title for the generated HTML shell and server manifest.
    pub title: String,
    /// Document description; CLI flag or `plec.toml`.
    pub description: Option<String>,
    /// Stylesheet URL; CLI flag or `plec.toml`.
    pub styles_href: Option<String>,
    /// Font preload URLs; CLI flags or `plec.toml`.
    pub preloads: Vec<String>,
    /// Runtime asset resolution policy. Auto preserves workspace development
    /// behavior; Package is used when dogfooding the release artifact.
    pub runtime_source: RuntimeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeSource {
    #[default]
    Auto,
    Package,
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub out_dir: PathBuf,
    /// Plec build identity for the provider manifest. Vite's content-hashed
    /// URLs own browser cache busting; this is not appended to asset URLs.
    pub revision: String,
}

/// The build stage a [`BuildError`] originates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Clean,
    Configuration,
    Compile,
    BrowserBundle,
    DependencyValidation,
    ServerBundle,
    ApiRoutes,
    ServerManifest,
    Hash,
    Brotli,
    Document,
    PublicAssets,
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Stage::Clean => "clean",
            Stage::Configuration => "configuration",
            Stage::Compile => "compile",
            Stage::BrowserBundle => "browser bundle",
            Stage::DependencyValidation => "dependency validation",
            Stage::ServerBundle => "server bundle",
            Stage::ApiRoutes => "API routes",
            Stage::ServerManifest => "server manifest",
            Stage::Hash => "hash",
            Stage::Brotli => "brotli",
            Stage::Document => "document",
            Stage::PublicAssets => "public assets",
        };
        write!(f, "{name}")
    }
}

/// A build failure tied to the pipeline stage that produced it.
#[derive(Debug)]
pub struct BuildError {
    pub stage: Stage,
    pub message: String,
    source: Option<Box<dyn std::error::Error + 'static>>,
}

impl BuildError {
    pub fn new(stage: Stage, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(
        stage: Stage,
        message: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + 'static>>,
    ) -> Self {
        Self {
            stage,
            message: message.into(),
            source: Some(source.into()),
        }
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} failed: {}", self.stage, self.message)
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref()
    }
}

/// Run the full shared Plec application build pipeline:
///
/// ```text
/// clean -> compile Plec artifacts -> Vite browser build and graph validation
///       -> revision -> brotli -> server bundle -> manifests -> document
/// ```
///
/// This is the single entry point both CLI variants call. Host wiring
/// (`plec.toml` -> `dist/plec-server.json`) is emitted alongside the
/// artifacts so `plec serve` never evaluates application source.
pub fn build(options: BuildOptions) -> Result<BuildResult, BuildError> {
    // Anchor everything to absolute paths so app-directory and repository
    // derivation never depend on the process working directory.
    let source = std::path::absolute(&options.source).map_err(|error| {
        BuildError::with_source(
            Stage::Compile,
            format!(
                "failed to resolve source entry {}",
                options.source.display()
            ),
            error,
        )
    })?;
    let out_dir = std::path::absolute(&options.out_dir).map_err(|error| {
        BuildError::with_source(
            Stage::Clean,
            format!(
                "failed to resolve output directory {}",
                options.out_dir.display()
            ),
            error,
        )
    })?;

    let app_dir = source
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            BuildError::new(
                Stage::Compile,
                format!(
                    "could not determine app directory from {}",
                    source.display()
                ),
            )
        })?
        .to_path_buf();

    let repo_root = find_repo_root(&app_dir);
    let public_dir = out_dir.join("public");
    let client_dir = out_dir.join("client");
    let client_entry = resolve_entry(&options.client_entry, &app_dir);
    let host_config = host::resolve_host_config(&app_dir, &options)?;
    let server_entry = resolve_entry(&host_config.server_entry, &app_dir);
    let api_routes =
        api_routes::discover(&app_dir).map_err(|error| BuildError::new(Stage::ApiRoutes, error))?;

    clean::prepare(&out_dir, &public_dir.join("assets"))?;
    assets::copy_public(&app_dir, &public_dir)?;

    let artifacts = artifacts::emit(
        &source,
        &app_dir,
        &repo_root,
        &public_dir,
        &client_dir,
        &out_dir.join("server"),
        &host_config.host_imports,
        &host_config.custom_elements,
        options.runtime_source,
    )?;
    let server_actions = artifacts.server_actions;

    let automatic_providers = artifacts
        .host_components
        .into_iter()
        .filter(|(provider, _)| host_config.host_adapters.contains_key(provider))
        .collect::<BTreeMap<_, _>>();

    let vite_result = run_vite_client(
        &app_dir,
        &client_entry,
        &client_dir,
        &automatic_providers,
        &host_config.host_adapters,
        options.optimize,
    )?;
    let mut revision_paths = vec![client_dir.join(vite_result.entry.trim_start_matches("/_plec/"))];
    revision_paths.extend(
        vite_result
            .styles
            .iter()
            .map(|url| client_dir.join(url.trim_start_matches("/_plec/"))),
    );
    revision_paths.extend(
        vite_result
            .providers
            .values()
            .map(|url| client_dir.join(url.trim_start_matches("/_plec/"))),
    );
    let revision = assets::revision(&revision_paths)?;
    host::emit_provider_manifest(
        &client_dir,
        &automatic_providers,
        &host_config.host_ssr_providers,
        &revision,
        &vite_result.providers,
    )?;
    assets::brotli_vite_assets(&client_dir.join("assets"))?;

    // The native host imports this application bundle through its Node
    // sidecar; the manifest records the path.
    let server_bundle = out_dir.join("server").join("app.mjs");
    server::bundle(
        &app_dir,
        server_entry.exists().then_some(server_entry.as_path()),
        &api_routes,
        &server_actions,
        &server_bundle,
        options.optimize,
    )?;

    let has_node_runtime = host::emit_node_runtime(&repo_root, &app_dir, &out_dir)?;
    if !has_node_runtime
        && (server_entry.exists() || !api_routes.is_empty() || !server_actions.is_empty())
    {
        // The app authored server code, but this workspace does not vendor
        // the Node application runtime: the emitted manifest carries no
        // `server` section and `/api/*` will 404 at runtime. Loud here beats
        // a wall of 404s in the browser.
        eprintln!(
            "warning: server entry {} exists, but packages/plec-node-runtime is not vendored in \
             this workspace; the emitted manifest will have no application runtime and /api/* \
             will 404",
            server_entry.display(),
        );
    }
    host::emit_server_manifest(
        &out_dir,
        &host_config,
        has_node_runtime,
        &vite_result.entry,
        &vite_result.styles,
    )?;

    let mut document_styles = vite_result.styles.clone();
    if let Some(styles_href) = &host_config.styles_href {
        document_styles.push(styles_href.clone());
    }
    document::write_index(
        &public_dir,
        &host_config.title,
        &vite_result.entry,
        &document_styles,
    )?;

    Ok(BuildResult { out_dir, revision })
}

/// Locate the repository root used for workspace package resolution and
/// runtime staging: the nearest ancestor of the app directory that carries a
/// Cargo workspace manifest and a `packages/` directory. Falls back to the
/// app directory itself so minimal workspaces still build.
fn find_repo_root(app_dir: &Path) -> PathBuf {
    app_dir
        .ancestors()
        .find(|candidate| {
            candidate.join("Cargo.toml").is_file() && candidate.join("packages").is_dir()
        })
        .map(Path::to_path_buf)
        .unwrap_or_else(|| app_dir.to_path_buf())
}

/// Resolve entry points relative to the app directory unless given absolute.
fn resolve_entry(entry: &Path, app_dir: &Path) -> PathBuf {
    if entry.is_absolute() {
        entry.to_path_buf()
    } else {
        app_dir.join(entry)
    }
}
