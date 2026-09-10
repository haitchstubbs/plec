use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use super::build::{BuildError, Stage};
use super::esbuild;
use super::id::sanitize;

/// Bundle the browser client with Plec-owned configuration.
///
/// Applications never configure platform, target, JSX handling or output
/// conventions; those are Plec build semantics.
pub fn bundle(
    entry: &Path,
    app_dir: &Path,
    outfile: &Path,
    metafile: &Path,
    optimize: bool,
) -> Result<(), BuildError> {
    let stage = Stage::BrowserBundle;
    let esbuild_bin = esbuild::resolve(app_dir).map_err(|error| BuildError::new(stage, error))?;

    let mut args = vec![
        esbuild_bin.to_string_lossy().into_owned(),
        "--bundle".into(),
        "--format=esm".into(),
        "--tree-shaking=true".into(),
        "--legal-comments=none".into(),
        "--platform=browser".into(),
        "--target=es2022".into(),
        "--jsx=automatic".into(),
        "--jsx-import-source=plec".into(),
        format!("--outfile={}", outfile.display()),
        format!("--metafile={}", metafile.display()),
    ];

    if optimize {
        args.push("--minify".into());
    }

    args.push(entry.to_string_lossy().into_owned());

    esbuild::run(&args).map_err(|error| BuildError::new(stage, error))
}

/// Bundle one provider factory per declared host provider. The generated entry
/// imports only component names found in executable artifacts, preserving
/// ESM tree shaking without writing an intermediate source file.
pub fn bundle_host_providers(
    providers: &BTreeMap<String, BTreeSet<String>>,
    adapters: &BTreeMap<String, String>,
    app_dir: &Path,
    assets_dir: &Path,
    optimize: bool,
) -> Result<(), BuildError> {
    let stage = Stage::BrowserBundle;
    let esbuild_bin = esbuild::resolve(app_dir).map_err(|error| BuildError::new(stage, error))?;
    let provider_dir = assets_dir.join("providers");

    if !providers.is_empty() {
        fs::create_dir_all(&provider_dir).map_err(|error| {
            BuildError::with_source(
                stage,
                format!("failed to create {}", provider_dir.display()),
                error,
            )
        })?;
    }

    for (provider, components) in providers {
        let adapter = adapters.get(provider).ok_or_else(|| {
            BuildError::new(
                stage,
                format!("host provider {provider:?} has no configured browser adapter"),
            )
        })?;
        let component_names = components.iter().cloned().collect::<Vec<_>>();
        if component_names.iter().any(|name| !is_identifier(name)) {
            return Err(BuildError::new(
                stage,
                format!("host provider {provider:?} contains a non-identifier component name"),
            ));
        }
        let imports = component_names.join(", ");
        let definitions = component_names.join(", ");
        let source = format!(
            "import createProvider, {{ {imports} }} from {adapter:?};\nexport default () => createProvider({{ {definitions} }});\n"
        );
        let outfile = provider_dir.join(format!("{}.js", sanitize(provider)));
        let mut args = vec![
            esbuild_bin.to_string_lossy().into_owned(),
            "--bundle".into(),
            "--format=esm".into(),
            "--tree-shaking=true".into(),
            "--legal-comments=none".into(),
            "--platform=browser".into(),
            "--target=es2022".into(),
            format!("--sourcefile=plec-host-provider-{provider}.ts"),
            format!("--outfile={}", outfile.display()),
        ];
        if optimize {
            args.push("--minify".into());
        }
        esbuild::run_with_stdin(&args, &source, app_dir)
            .map_err(|error| BuildError::new(stage, error))?;
    }

    Ok(())
}

fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some(character) if character == '_' || character == '$' || character.is_ascii_alphabetic())
        && characters.all(|character| {
            character == '_' || character == '$' || character.is_ascii_alphanumeric()
        })
}
