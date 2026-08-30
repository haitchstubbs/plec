use std::{fs, path::Path};
pub fn stage(repo_root: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source_dir = repo_root
        .join("packages")
        .join("plec-runtime")
        .join("dist")
        .join("runtime");

    let destination_dir = out_dir.join("runtime");

    fs::create_dir_all(&destination_dir)?;

    for file in ["runtime.js", "runtime_bg.wasm"] {
        let source = source_dir.join(file);

        if !source.exists() {
            return Err(format!("Plec runtime artifact not found: {}", source.display()).into());
        }

        fs::copy(&source, destination_dir.join(file))?;
    }

    for file in ["runtime.js.br", "runtime_bg.wasm.br"] {
        let source = source_dir.join(file);

        if source.exists() {
            fs::copy(&source, destination_dir.join(file))?;
        }
    }

    Ok(())
}
