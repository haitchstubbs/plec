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
    /// Explicitly permits this adapter to load in the Node host for SSR.
    /// Browser-only providers stay inert on the server.
    #[serde(default)]
    ssr: bool,
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
    pub host_ssr_providers: BTreeSet<String>,
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
    let host_ssr_providers = config
        .as_ref()
        .map(|config| {
            config
                .compiler
                .host_imports
                .values()
                .filter(|binding| binding.ssr)
                .map(|binding| binding.provider.clone())
                .collect()
        })
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
        host_ssr_providers,
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
/// providers with a configured adapter get a bundled `/_plec/assets/*.js`
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
            Stage::Configuration,
            format!("cannot read {}", config_path.display()),
            error,
        )
    })?;
    toml::from_str(&text).map(Some).map_err(|error| {
        BuildError::with_source(
            Stage::Configuration,
            format!("invalid {}: {error}", config_path.display()),
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
            Stage::Configuration,
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
            Stage::Configuration,
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
    client_dir: &'static str,
    artifact: &'static str,
    client_script: String,
    client_styles: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles_href: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    preloads: Vec<String>,
    /// Trusted custom element tags; the SSR host applies the same element
    /// policy the CSR runtime enforces.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    custom_elements: Vec<String>,
    document: ManifestDocument,
    server: ManifestServerSection,
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
struct ManifestServerSection {
    entry: &'static str,
}

/// Wire version of `/host-providers.json`. Browser glue gates the same
/// value in `registerPlecProviders`; `plec workspace contract ssr` tracks
/// the pair. The manifest versions independently of the SSR snapshot and
/// bootstrap wrappers: producer and consumer ship in the same build, so it
/// never rides a snapshot version bump.
pub(crate) const PROVIDER_MANIFEST_VERSION: u32 = 2;

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
    ssr: bool,
}

/// Emits `dist/client/host-providers.json` for `registerPlecProviders` in
/// `packages/plec-browser`. Module paths are build-owned asset URLs so the
/// browser gate can reject everything outside `/_plec/assets/`.
pub fn emit_provider_manifest(
    client_dir: &Path,
    providers: &BTreeMap<String, BTreeSet<String>>,
    ssr_providers: &BTreeSet<String>,
    revision: &str,
    module_urls: &BTreeMap<String, String>,
) -> Result<(), BuildError> {
    let mut entries = Vec::with_capacity(providers.len());
    for (id, components) in providers {
        let module = module_urls.get(id).ok_or_else(|| {
            BuildError::new(
                Stage::BrowserBundle,
                format!("Vite output is missing the browser module for provider {id:?}"),
            )
        })?;
        entries.push(ProviderManifestEntry {
            id,
            module: module.clone(),
            components: components.iter().map(String::as_str).collect(),
            ssr: ssr_providers.contains(id),
        });
    }
    let manifest = ProviderManifest {
        version: PROVIDER_MANIFEST_VERSION,
        revision,
        providers: entries,
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|error| {
        BuildError::with_source(
            Stage::BrowserBundle,
            "failed to serialize host provider manifest",
            error,
        )
    })?;
    std::fs::write(client_dir.join("host-providers.json"), json).map_err(|error| {
        BuildError::with_source(
            Stage::BrowserBundle,
            format!(
                "cannot write {}",
                client_dir.join("host-providers.json").display()
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
    client_script: &str,
    client_styles: &[String],
) -> Result<(), BuildError> {
    let manifest = ServerManifest {
        version: 1,
        public_dir: "public",
        client_dir: "client",
        artifact: "server/route-artifact.json",
        // The build owns where it emitted the client bundle; the app never
        // writes this path.
        client_script: client_script.to_owned(),
        client_styles: client_styles.to_vec(),
        styles_href: config.styles_href.clone(),
        preloads: config.preloads.clone(),
        custom_elements: config.custom_elements.iter().cloned().collect(),
        document: ManifestDocument {
            title: Some(config.title.clone()),
            description: config.description.clone(),
        },
        server: ManifestServerSection {
            entry: "server/app.mjs",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeSource;

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
"lucide" = { provider = "lucide", adapter = "plec-lucide", ssr = true }
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
            runtime_source: RuntimeSource::Auto,
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
            Some(&String::from("plec-lucide"))
        );
        assert!(config.host_ssr_providers.contains("lucide"));
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
            host_ssr_providers: BTreeSet::new(),
            custom_elements: Default::default(),
        };
        emit_server_manifest(dir.path(), &config, "/_plec/assets/client-hash.js", &[])
            .expect("manifest");

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("plec-server.json")).expect("manifest read"),
        )
        .expect("json");
        assert_eq!(json["version"], 1);
        assert_eq!(json["publicDir"], "public");
        assert_eq!(json["artifact"], "server/route-artifact.json");
        assert_eq!(json["clientScript"], "/_plec/assets/client-hash.js");
        assert_eq!(json["server"]["entry"], "server/app.mjs");
        assert_eq!(json["document"]["title"], "Plec fullstack playground");
        assert!(json["document"]["description"] == "desc");
    }

    #[test]
    fn manifest_declares_node_host_application_entry() {
        let dir = tempfile::tempdir().expect("dir");
        let config = HostConfig {
            title: "t".into(),
            description: None,
            server_entry: "src/server.ts".into(),
            styles_href: None,
            preloads: Vec::new(),
            host_imports: BTreeMap::new(),
            host_adapters: BTreeMap::new(),
            host_ssr_providers: BTreeSet::new(),
            custom_elements: Default::default(),
        };
        emit_server_manifest(dir.path(), &config, "/_plec/assets/client-hash.js", &[])
            .expect("manifest");
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("plec-server.json")).expect("manifest read"),
        )
        .expect("json");
        assert_eq!(json["server"]["entry"], "server/app.mjs");
    }

    #[test]
    fn provider_manifest_records_build_owned_module_urls() {
        let dir = tempfile::tempdir().expect("dir");
        let client_dir = dir.path().join("client");
        std::fs::create_dir_all(&client_dir).expect("client dir");
        let providers = BTreeMap::from([(
            String::from("lucide"),
            BTreeSet::from([String::from("House"), String::from("Beaker")]),
        )]);

        emit_provider_manifest(
            &client_dir,
            &providers,
            &BTreeSet::from([String::from("lucide")]),
            "revision-1",
            &BTreeMap::from([(
                String::from("lucide"),
                String::from("/_plec/assets/provider-lucide-hash.js"),
            )]),
        )
        .expect("manifest");

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(client_dir.join("host-providers.json"))
                .expect("manifest read"),
        )
        .expect("json");
        assert_eq!(json["version"], PROVIDER_MANIFEST_VERSION);
        assert_eq!(json["revision"], "revision-1");
        assert_eq!(json["providers"][0]["id"], "lucide");
        assert_eq!(
            json["providers"][0]["module"],
            "/_plec/assets/provider-lucide-hash.js"
        );
        assert_eq!(json["providers"][0]["components"][0], "Beaker");
        assert_eq!(json["providers"][0]["components"][1], "House");
    }
}
