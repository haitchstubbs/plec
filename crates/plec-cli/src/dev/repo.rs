use std::path::PathBuf;

/// The Plec workspace, discovered by walking up from the current directory.
///
/// Dev commands operate on the repository itself (crate sources, built
/// packages, staged app artifacts), so every command needs the workspace root
/// rather than the process cwd.
pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    pub fn discover() -> Result<Repo, String> {
        let mut current =
            std::env::current_dir().map_err(|error| format!("cannot read cwd: {error}"))?;

        loop {
            if current.join("crates/plec-cli").is_dir() && current.join("crates/plec-ir").is_dir() {
                return Ok(Repo { root: current });
            }

            if !current.pop() {
                return Err(
                    "could not locate the Plec workspace root (no crates/plec-cli above the \
                     current directory)"
                        .into(),
                );
            }
        }
    }

    /// Dev-CLI scratch state. `.cache/` is gitignored at the workspace root;
    /// the app CLI reserves `.plec/` for its own state.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(".cache").join("plec")
    }

    pub fn ensure_cache_dir(&self) -> Result<PathBuf, String> {
        let dir = self.cache_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create cache dir {}: {error}", dir.display()))?;
        Ok(dir)
    }

    /// wasm-pack output for the runtime package.
    pub fn runtime_dist_dir(&self) -> PathBuf {
        self.root.join("packages/plec-runtime/dist/runtime")
    }

    /// Runtime artifacts staged into the fullstack application build
    /// (the plec-build pipeline emits them under the document root).
    pub fn staged_runtime_dir(&self) -> PathBuf {
        self.root.join("apps/fullstack/dist/public/runtime")
    }

    /// Default application source used by graph-level commands: the
    /// fullstack app's router entry, the same source `plec build` compiles.
    pub fn default_app_source(&self) -> PathBuf {
        self.root.join("apps/fullstack/src/router.tsx")
    }
}
