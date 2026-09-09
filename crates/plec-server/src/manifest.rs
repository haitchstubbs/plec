//! The generated server manifest (`dist/plec-server.json`).
//!
//! `plec build` emits it; this module is the versioned deserialization
//! mirror the host consumes. Every path inside resolves relative to the
//! manifest's own directory — never the process working directory — so a
//! deployment directory is portable as a unit.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{DocumentMetadata, PlecServerOptions, ServerError};

pub const SERVER_MANIFEST_VERSION: u32 = 1;
pub const SERVER_MANIFEST_FILE: &str = "plec-server.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerManifest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_public_dir")]
    pub public_dir: PathBuf,
    #[serde(default = "default_artifact")]
    pub artifact: PathBuf,
    #[serde(default)]
    pub client_script: Option<String>,
    #[serde(default)]
    pub styles_href: Option<String>,
    #[serde(default)]
    pub preloads: Vec<String>,
    /// Trusted custom element tags (the application's `plec.toml`
    /// `[compiler] custom-elements`), consumed by the SSR element-tag
    /// policy. Absent in older manifests.
    #[serde(default)]
    pub custom_elements: Vec<String>,
    #[serde(default)]
    pub document: DocumentMetadata,
    /// The Node application-runtime section. Absent for builds that ship no
    /// server bundle; the host then serves documents and assets only.
    #[serde(default)]
    pub server: Option<ServerManifestRuntime>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerManifestRuntime {
    /// The application server bundle (`server/app.mjs`).
    pub entry: PathBuf,
    /// The `plec-node-runtime` sidecar script (`server/runtime.mjs`).
    pub runtime: PathBuf,
}

fn default_version() -> u32 {
    SERVER_MANIFEST_VERSION
}

fn default_public_dir() -> PathBuf {
    PathBuf::from("public")
}

fn default_artifact() -> PathBuf {
    PathBuf::from("public/route-artifact.json")
}

/// A manifest loaded from disk, with every relative path resolved against
/// the manifest's directory.
#[derive(Debug, Clone)]
pub struct LoadedServerManifest {
    manifest: ServerManifest,
    base: PathBuf,
}

impl LoadedServerManifest {
    /// Reads `dir/plec-server.json` and validates its version.
    pub fn load(dir: &Path) -> Result<Self, ServerError> {
        let text = std::fs::read_to_string(dir.join(SERVER_MANIFEST_FILE)).map_err(|error| {
            ServerError::message(format!(
                "cannot read server manifest {}{}: {error}",
                dir.display(),
                std::path::MAIN_SEPARATOR
            ))
        })?;
        let manifest: ServerManifest = serde_json::from_str(&text)
            .map_err(|error| ServerError::message(format!("invalid server manifest: {error}")))?;
        if manifest.version != SERVER_MANIFEST_VERSION {
            return Err(ServerError::message(format!(
                "unsupported server manifest version {} (expected {})",
                manifest.version, SERVER_MANIFEST_VERSION
            )));
        }
        Ok(Self {
            manifest,
            base: dir.to_path_buf(),
        })
    }

    /// The public asset directory.
    pub fn public_dir(&self) -> PathBuf {
        self.base.join(&self.manifest.public_dir)
    }

    /// The compiled application artifact.
    pub fn artifact_path(&self) -> PathBuf {
        self.base.join(&self.manifest.artifact)
    }

    /// The application server bundle, when the build emitted one.
    pub fn server_entry(&self) -> Option<PathBuf> {
        self.manifest
            .server
            .as_ref()
            .map(|server| self.base.join(&server.entry))
    }

    /// The `plec-node-runtime` sidecar script, when the build emitted one.
    pub fn server_runtime(&self) -> Option<PathBuf> {
        self.manifest
            .server
            .as_ref()
            .map(|server| self.base.join(&server.runtime))
    }

    /// Host options for everything the manifest owns. The application
    /// runtime is deliberately left unset — the supervisor attaches it.
    pub fn options(&self, development: bool) -> PlecServerOptions {
        PlecServerOptions {
            public_dir: self.public_dir(),
            artifact_path: self.artifact_path(),
            client_script: self.manifest.client_script.clone(),
            styles_href: self.manifest.styles_href.clone(),
            preloads: self.manifest.preloads.clone(),
            custom_elements: self.manifest.custom_elements.clone(),
            document: self.manifest.document.clone(),
            application_runtime: None,
            development,
        }
    }
}
