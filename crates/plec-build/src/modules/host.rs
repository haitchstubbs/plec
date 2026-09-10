//! Host-side build configuration: the app's `plec.toml` plus build-derived
//! facts, resolved into the generated `dist/plec-server.json` manifest.
//!
//! The split is deliberate: serializable host configuration flows from
//! `plec.toml` through `plec build` into the manifest, and the Rust host
//! (`plec serve`) consumes only the generated artifact — production never
//! needs source files, and neither host variant evaluates application code
//! for its own wiring.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

use super::build::{BuildError, BuildOptions, Stage};

/// The `plec.toml` file at the application root. Fully optional.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct AppConfig {
    #[serde(default)]
    app: AppSection,
    #[serde(default)]
    server: ServerSection,
    #[serde(default)]
    client: ClientSection,
    #[serde(default)]
    compiler: CompilerSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppSection {
    title: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerSection {
    entry: Option<std::path::PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientSection {
    styles: Option<String>,
    preloads: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct CompilerSection {
    #[serde(default)]
    host_imports: BTreeMap<String, HostImportBinding>,
    /// Trusted custom element tags executable IR may instantiate. The
    /// compiler diagnostics and the runtime element policy enforce the same
    /// list (crates/plec-ir/src/sink.rs).
    #[serde(default)]
    custom_elements: std::collections::BTreeSet<String>,
}

/// One `[compiler.host-imports]` entry. `provider` is the artifact-declared
/// host component provider id the import specifier resolves to; `adapter`
/// is the browser module the build bundles the provider factory from.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostImportBinding {
    provider: String,
    adapter: String,
}

/// Host configuration resolved from `plec.toml` merged with explicit build
/// options (CLI flags win, then toml, then the existing defaults).
#[derive(Debug, Clone)]
pub struct HostConfig {
    pub title: String,
    pub description: Option<String>,
    pub server_entry: std::path::PathBuf,
    pub styles_href: Option<String>,
    pub preloads: Vec<String>,
    pub host_imports: BTreeMap<String, String>,
    pub host_adapters: BTreeMap<String, String>,
    pub custom_elements: std::collections::BTreeSet<String>,
}

const DEFAULT_SERVER_ENTRY: &str = "src/server.ts";
const DEFAULT_TITLE: &str = "Plec app";

pub fn resolve_host_config(
    app_dir: &Path,
    options: &BuildOptions,
) -> Result<HostConfig, BuildError> {
    let config_path = app_dir.join("plec.toml");
    let config = read_app_config(&config_path)?;
    let host_imports = config
        .as_ref()
        .map(|config| {
            config
                .compiler
                .host_imports
                .iter()
                .map(|(specifier, binding)| (specifier.clone(), binding.provider.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    validate_host_imports(&host_imports, &config_path)?;
    let host_adapters = config
        .as_ref()
        .map(|config| {
            config
                .compiler
                .host_imports
                .iter()
                .map(|(_, binding)| (binding.provider.clone(), binding.adapter.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    validate_host_adapters(&host_adapters, &config_path)?;
    let custom_elements = config
        .as_ref()
        .map(|config| config.compiler.custom_elements.clone())
        .unwrap_or_default();

    Ok(HostConfig {
        title: {
            let flag = options.title.trim();
            if flag.is_empty() {
                config
                    .as_ref()
                    .and_then(|config| config.app.title.clone())
                    .unwrap_or_else(|| DEFAULT_TITLE.to_owned())
            } else {
                flag.to_owned()
            }
        },
        description: options.description.clone().or_else(|| {
            config
                .as_ref()
                .and_then(|config| config.app.description.clone())
        }),
        server_entry: if options.server_entry.as_os_str()
            == std::path::Path::new(DEFAULT_SERVER_ENTRY).as_os_str()
        {
            config
                .as_ref()
                .and_then(|config| config.server.entry.clone())
                .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_SERVER_ENTRY))
        } else {
            options.server_entry.clone()
        },
        styles_href: options.styles_href.clone().or_else(|| {
            config
                .as_ref()
                .and_then(|config| config.client.styles.clone())
        }),
        preloads: if options.preloads.is_empty() {
            config
                .as_ref()
                .and_then(|config| config.client.preloads.clone())
                .unwrap_or_default()
        } else {
            options.preloads.clone()
        },
        host_imports,
        host_adapters,
        custom_elements,
    })
}

/// The compiler-facing view: import specifier -> artifact-declared provider
/// id (`host:{provider}` module resolution).
pub fn resolve_host_imports(app_dir: &Path) -> Result<BTreeMap<String, String>, BuildError> {
    let config_path = app_dir.join("plec.toml");
    let imports = read_host_import_bindings(&config_path)?
        .into_iter()
        .map(|(specifier, binding)| (specifier, binding.provider))
        .collect();
    validate_host_imports(&imports, &config_path)?;
    Ok(imports)
}

/// The bundler-facing view: provider id -> browser adapter module. Only
/// providers with a configured adapter get a bundled `/assets/providers/*.js`
/// entry and a `host-providers.json` record.
pub fn resolve_host_adapters(app_dir: &Path) -> Result<BTreeMap<String, String>, BuildError> {
    let config_path = app_dir.join("plec.toml");
    let adapters = read_host_import_bindings(&config_path)?
        .into_iter()
        .map(|(_, binding)| (binding.provider, binding.adapter))
        .collect();
    validate_host_adapters(&adapters, &config_path)?;
    Ok(adapters)
}

fn read_host_import_bindings(
    config_path: &Path,
) -> Result<BTreeMap<String, HostImportBinding>, BuildError> {
    Ok(read_app_config(config_path)?
        .map(|config| config.compiler.host_imports)
        .unwrap_or_default())
}

/// The trusted custom element list from the application's `plec.toml`
/// (`[compiler] custom-elements`).
pub fn resolve_custom_elements(
    app_dir: &Path,
) -> Result<std::collections::BTreeSet<String>, BuildError> {
    let config_path = app_dir.join("plec.toml");
    Ok(read_app_config(&config_path)?
        .map(|config| config.compiler.custom_elements)
        .unwrap_or_default())
}

fn read_app_config(config_path: &Path) -> Result<Option<AppConfig>, BuildError> {
    if !config_path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(config_path).map_err(|error| {
        BuildError::with_source(
            Stage::ServerManifest,
            format!("cannot read {}", config_path.display()),
            error,
        )
    })?;
    toml::from_str(&text).map(Some).map_err(|error| {
        BuildError::with_source(
            Stage::ServerManifest,
            format!("invalid {}", config_path.display()),
            error,
        )
    })
}

fn validate_host_imports(
    imports: &BTreeMap<String, String>,
    config_path: &Path,
) -> Result<(), BuildError> {
    if let Some((specifier, provider)) = imports
        .iter()
        .find(|(specifier, provider)| specifier.trim().is_empty() || provider.trim().is_empty())
    {
        return Err(BuildError::new(
            Stage::ServerManifest,
            format!(
                "invalid host import binding in {}: import specifier and provider id must be non-empty (found {specifier:?} -> {provider:?})",
                config_path.display()
            ),
        ));
    }
    Ok(())
}

fn validate_host_adapters(
    adapters: &BTreeMap<String, String>,
    config_path: &Path,
) -> Result<(), BuildError> {
    if let Some((provider, adapter)) = adapters
        .iter()
        .find(|(provider, adapter)| provider.trim().is_empty() || adapter.trim().is_empty())
    {
        return Err(BuildError::new(
            Stage::ServerManifest,
            format!(
                "invalid host provider adapter in {}: provider id and adapter module must be non-empty (found {provider:?} -> {adapter:?})",
                config_path.display()
            ),
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerManifest {
    version: u32,
    public_dir: &'static str,
    artifact: &'static str,
    client_script: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles_href: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    preloads: Vec<String>,
    /// Trusted custom element tags; the SSR host applies the same element
    /// policy the CSR runtime enforces.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    custom_elements: Vec<String>,
    document: ManifestDocument,
    /// Absent when the workspace does not vendor `packages/plec-node-runtime`;
    /// the host then serves documents and assets only.
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<ManifestRuntimeSection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestDocument {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestRuntimeSection {
    entry: &'static str,
    runtime: &'static str,
}

/// Wire version of `/host-providers.json`. Browser glue gates the same
/// value in `registerPlecProviders`; `plec workspace contract ssr` tracks
/// the pair. The manifest versions independently of the SSR snapshot and
/// bootstrap wrappers: producer and consumer ship in the same build, so it
/// never rides a snapshot version bump.
pub(crate) const PROVIDER_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderManifest<'a> {
    version: u32,
    revision: &'a str,
    providers: Vec<ProviderManifestEntry<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderManifestEntry<'a> {
    id: &'a str,
    module: String,
    components: Vec<&'a str>,
}

/// Emits `dist/public/host-providers.json` for `registerPlecProviders` in
/// `packages/plec-browser`. Module paths are build-owned asset URLs so the
/// browser gate can reject everything outside `/assets/providers/`.
pub fn emit_provider_manifest(
    public_dir: &Path,
    providers: &BTreeMap<String, BTreeSet<String>>,
    revision: &str,
) -> Result<(), BuildError> {
    let manifest = ProviderManifest {
        version: PROVIDER_MANIFEST_VERSION,
        revision,
        providers: providers
            .iter()
            .map(|(id, components)| ProviderManifestEntry {
                id,
                module: format!(
                    "/assets/providers/{}.js?v={revision}",
                    super::id::sanitize(id)
                ),
                components: components.iter().map(String::as_str).collect(),
            })
            .collect(),
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|error| {
        BuildError::with_source(
            Stage::BrowserBundle,
            "failed to serialize host provider manifest",
            error,
        )
    })?;
    std::fs::write(public_dir.join("host-providers.json"), json).map_err(|error| {
        BuildError::with_source(
            Stage::BrowserBundle,
            format!(
                "cannot write {}",
                public_dir.join("host-providers.json").display()
            ),
            error,
        )
    })
}

/// Writes `dist/plec-server.json`. All paths are relative to the manifest's
/// own directory, so the build output is portable as a unit and the host
/// never resolves anything against its working directory.
pub fn emit_server_manifest(
    out_dir: &std::path::Path,
    config: &HostConfig,
    has_node_runtime: bool,
) -> Result<(), BuildError> {
    let manifest = ServerManifest {
        version: 1,
        public_dir: "public",
        artifact: "public/route-artifact.json",
        // The build owns where it emitted the client bundle; the app never
        // writes this path.
        client_script: "/assets/client.js",
        styles_href: config.styles_href.clone(),
        preloads: config.preloads.clone(),
        custom_elements: config.custom_elements.iter().cloned().collect(),
        document: ManifestDocument {
            title: Some(config.title.clone()),
            description: config.description.clone(),
        },
        server: if has_node_runtime {
            Some(ManifestRuntimeSection {
                entry: "server/app.mjs",
                runtime: "server/runtime.mjs",
            })
        } else {
            None
        },
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|error| {
        BuildError::with_source(
            Stage::ServerManifest,
            "server manifest serialization failed",
            error,
        )
    })?;
    std::fs::write(out_dir.join("plec-server.json"), json).map_err(|error| {
        BuildError::with_source(
            Stage::ServerManifest,
            "cannot write plec-server.json",
            error,
        )
    })
}

/// Bundles the vendored `plec-node-runtime` TypeScript source into the build
/// output and reports whether the Node application-runtime section applies.
/// The build owns the bundling (it already drives esbuild), so no separate
/// package build step or workspace task ordering is required.
pub fn emit_node_runtime(
    repo_root: &Path,
    app_dir: &Path,
    out_dir: &std::path::Path,
) -> Result<bool, BuildError> {
    let source = repo_root.join("packages/plec-node-runtime/src/runtime.ts");
    if !source.exists() {
        return Ok(false);
    }
    let server_dir = out_dir.join("server");
    std::fs::create_dir_all(&server_dir).map_err(|error| {
        BuildError::with_source(
            Stage::ServerManifest,
            format!("cannot create {}", server_dir.display()),
            error,
        )
    })?;
    let esbuild_bin = super::esbuild::resolve(app_dir)
        .map_err(|error| BuildError::new(Stage::ServerManifest, error))?;
    super::esbuild::run(&[
        esbuild_bin.to_string_lossy().into_owned(),
        "--bundle".into(),
        "--format=esm".into(),
        "--platform=node".into(),
        "--target=node20".into(),
        format!("--outfile={}", server_dir.join("runtime.mjs").display()),
        source.to_string_lossy().into_owned(),
    ])
    .map_err(|error| BuildError::new(Stage::ServerManifest, error))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plec_toml_resolves_with_cli_flags_winning() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(
            dir.path().join("plec.toml"),
            r#"
[app]
title = "From toml"
description = "toml description"

[server]
entry = "src/server.ts"

[client]
styles = "/assets/styles.css"
preloads = ["/a.woff2", "/b.woff2"]

[compiler.host-imports]
"lucide" = { provider = "lucide", adapter = "@wasm-runtime/lucide-plec" }
"#,
        )
        .expect("toml write");
        let options = BuildOptions {
            source: dir.path().join("src/app.tsx"),
            client_entry: "src/client.tsx".into(),
            server_entry: "src/server.ts".into(),
            out_dir: dir.path().join("dist"),
            optimize: true,
            title: String::new(),
            description: Some("cli description".into()),
            styles_href: None,
            preloads: Vec::new(),
        };
        let config = resolve_host_config(dir.path(), &options).expect("config");
        assert_eq!(config.title, "From toml");
        assert_eq!(config.description.as_deref(), Some("cli description"));
        assert_eq!(config.styles_href.as_deref(), Some("/assets/styles.css"));
        assert_eq!(config.preloads.len(), 2);
        assert_eq!(
            config.host_imports.get("lucide"),
            Some(&String::from("lucide"))
        );
        assert_eq!(
            config.host_adapters.get("lucide"),
            Some(&String::from("@wasm-runtime/lucide-plec"))
        );
    }

    #[test]
    fn manifest_paths_are_manifest_relative_and_portable() {
        let dir = tempfile::tempdir().expect("dir");
        let config = HostConfig {
            title: "Plec fullstack playground".into(),
            description: Some("desc".into()),
            server_entry: "src/server.ts".into(),
            styles_href: Some("/assets/styles.css".into()),
            preloads: vec!["/assets/files/a.woff2".into()],
            host_imports: BTreeMap::new(),
            host_adapters: BTreeMap::new(),
            custom_elements: Default::default(),
        };
        emit_server_manifest(dir.path(), &config, true).expect("manifest");

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("plec-server.json")).expect("manifest read"),
        )
        .expect("json");
        assert_eq!(json["version"], 1);
        assert_eq!(json["publicDir"], "public");
        assert_eq!(json["artifact"], "public/route-artifact.json");
        assert_eq!(json["clientScript"], "/assets/client.js");
        assert_eq!(json["server"]["entry"], "server/app.mjs");
        assert_eq!(json["server"]["runtime"], "server/runtime.mjs");
        assert_eq!(json["document"]["title"], "Plec fullstack playground");
        assert!(json["document"]["description"] == "desc");
    }

    #[test]
    fn manifest_omits_the_runtime_section_without_the_node_package() {
        let dir = tempfile::tempdir().expect("dir");
        let config = HostConfig {
            title: "t".into(),
            description: None,
            server_entry: "src/server.ts".into(),
            styles_href: None,
            preloads: Vec::new(),
            host_imports: BTreeMap::new(),
            host_adapters: BTreeMap::new(),
            custom_elements: Default::default(),
        };
        emit_server_manifest(dir.path(), &config, false).expect("manifest");
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("plec-server.json")).expect("manifest read"),
        )
        .expect("json");
        assert!(json.get("server").is_none());
    }

    #[test]
    fn provider_manifest_records_build_owned_module_urls() {
        let dir = tempfile::tempdir().expect("dir");
        let public_dir = dir.path().join("public");
        std::fs::create_dir_all(&public_dir).expect("public dir");
        let providers = BTreeMap::from([(
            String::from("lucide"),
            BTreeSet::from([String::from("House"), String::from("Beaker")]),
        )]);

        emit_provider_manifest(&public_dir, &providers, "revision-1").expect("manifest");

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(public_dir.join("host-providers.json"))
                .expect("manifest read"),
        )
        .expect("json");
        assert_eq!(json["version"], PROVIDER_MANIFEST_VERSION);
        assert_eq!(json["revision"], "revision-1");
        assert_eq!(json["providers"][0]["id"], "lucide");
        assert_eq!(
            json["providers"][0]["module"],
            "/assets/providers/lucide.js?v=revision-1"
        );
        assert_eq!(json["providers"][0]["components"][0], "Beaker");
        assert_eq!(json["providers"][0]["components"][1], "House");
    }
}
