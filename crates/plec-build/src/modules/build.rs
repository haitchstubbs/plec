use super::artifacts;
use super::assets;
use super::bundle::bundle;
use super::clean;
use super::document;
use super::host;
use super::server;
use super::validate;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Configuration for the shared application build pipeline.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Route source entry (e.g. `src/router.tsx`).
    pub source: PathBuf,
    /// Browser client entry (e.g. `src/client.tsx`).
    pub client_entry: PathBuf,
    /// Server entry (e.g. `src/server.ts`).
    pub server_entry: PathBuf,
    /// Build output root. Plec-owned artifacts land under `public/`.
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
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub out_dir: PathBuf,
    /// Truncated SHA-256 revision of the emitted browser client.
    pub revision: String,
}

/// The build stage a [`BuildError`] originates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Clean,
    Compile,
    BrowserBundle,
    DependencyValidation,
    ServerBundle,
    ServerManifest,
    Hash,
    Brotli,
    Document,
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Stage::Clean => "clean",
            Stage::Compile => "compile",
            Stage::BrowserBundle => "browser bundle",
            Stage::DependencyValidation => "dependency validation",
            Stage::ServerBundle => "server bundle",
            Stage::ServerManifest => "server manifest",
            Stage::Hash => "hash",
            Stage::Brotli => "brotli",
            Stage::Document => "document",
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
/// clean -> compile artifacts -> browser bundle -> dependency validation
///       -> revision -> brotli -> server bundle -> document
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
    let assets_dir = public_dir.join("assets");
    let client_entry = resolve_entry(&options.client_entry, &app_dir);
    let host_config = host::resolve_host_config(&app_dir, &options)?;
    let server_entry = resolve_entry(&host_config.server_entry, &app_dir);

    clean::prepare(&out_dir, &assets_dir)?;

    let artifacts = artifacts::emit(
        &source,
        &app_dir,
        &repo_root,
        &public_dir,
        &host_config.host_imports,
        &host_config.custom_elements,
    )?;

    let automatic_providers = artifacts
        .host_components
        .into_iter()
        .filter(|(provider, _)| host_config.host_adapters.contains_key(provider))
        .collect::<BTreeMap<_, _>>();

    super::bundle::bundle_host_providers(
        &automatic_providers,
        &host_config.host_adapters,
        &app_dir,
        &assets_dir,
        options.optimize,
    )?;

    let client_path = assets_dir.join("client.js");
    let metafile_path = out_dir.join("client.meta.json");

    bundle(
        &client_entry,
        &app_dir,
        &client_path,
        &metafile_path,
        options.optimize,
    )?;

    validate::browser_dependencies(&metafile_path)?;

    let mut revision_paths = vec![client_path.clone()];
    revision_paths.extend(automatic_providers.keys().map(|provider| {
        assets_dir
            .join("providers")
            .join(format!("{}.js", super::id::sanitize(provider)))
    }));
    let revision = assets::revision(&revision_paths)?;
    host::emit_provider_manifest(&public_dir, &automatic_providers, &revision)?;
    assets::brotli(&client_path)?;

    // The native host imports this application bundle through its Node
    // sidecar; the manifest records the path.
    let server_bundle = out_dir.join("server").join("app.mjs");
    server::bundle(&server_entry, &app_dir, &server_bundle, options.optimize)?;

    let has_node_runtime = host::emit_node_runtime(&repo_root, &app_dir, &out_dir)?;
    if !has_node_runtime && server_entry.exists() {
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
    host::emit_server_manifest(&out_dir, &host_config, has_node_runtime)?;

    document::write_index(&public_dir, &host_config.title, &revision)?;

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
