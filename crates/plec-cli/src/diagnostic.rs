use std::{error::Error, fmt};

use plec_build::{BuildError, modules::build::Stage};

/// Narrow CLI adapter for authoritative compiler/build failures.
pub(crate) struct DevelopmentDiagnostic {
    code: &'static str,
    phase: &'static str,
    message: String,
    source: Option<String>,
    location: Option<String>,
    detail: Option<String>,
    suggestion: Option<&'static str>,
}

impl DevelopmentDiagnostic {
    pub(crate) fn compiler(error: impl fmt::Display) -> Self {
        Self::compiler_message(error.to_string())
    }

    fn compiler_message(raw: String) -> Self {
        let parsed = raw.strip_prefix("Failed to parse ").and_then(|value| {
            let (module, rest) = value.split_once(": ")?;
            let rest = rest.strip_prefix(&format!("{module}:"))?;
            let position = rest.split(": ").next()?;
            let (line, column) = position.split_once(':')?;
            Some((module.to_owned(), format!("{line}:{column}")))
        });
        let source = parsed.as_ref().map(|(module, _)| module.clone());
        let location = parsed.map(|(_, location)| location);
        let message = if source.is_some() {
            "TypeScript/TSX syntax could not be parsed".to_owned()
        } else {
            raw.lines().next().unwrap_or("compiler failed").to_owned()
        };
        let has_source = source.is_some();
        Self {
            code: if has_source {
                "PLEC-PARSE-001"
            } else {
                "PLEC-COMPILE-001"
            },
            phase: if has_source { "parse" } else { "compile" },
            message,
            source,
            location,
            detail: verbose().then_some(raw),
            suggestion: has_source
                .then_some("Check the reported source location for a TypeScript/TSX syntax error."),
        }
    }

    pub(crate) fn build(error: BuildError) -> Self {
        let (code, phase) = match error.stage {
            Stage::Compile => ("PLEC-COMPILE-001", "compile"),
            Stage::ApiRoutes => ("PLEC-DISCOVERY-ROUTES", "discovery"),
            Stage::ServerManifest if error.message.contains("plec.toml") => {
                ("PLEC-BUILD-CONFIG", "build")
            }
            _ => ("PLEC-BUILD-001", "build"),
        };
        let detail = verbose()
            .then(|| error.source().map(ToString::to_string))
            .flatten();
        Self {
            code,
            phase,
            message: error.message,
            source: None,
            location: None,
            detail,
            suggestion: (code == "PLEC-BUILD-CONFIG")
                .then_some("Review the Plec configuration file and its field names/values."),
        }
    }

    pub(crate) fn server_manifest(error: impl fmt::Display) -> Self {
        Self {
            code: "PLEC-SERVER-MANIFEST",
            phase: "server",
            message: error.to_string(),
            source: None,
            location: None,
            detail: None,
            suggestion: Some("Rebuild the application or check the selected serve directory."),
        }
    }
}

fn verbose() -> bool {
    std::env::var("PLEC_DIAGNOSTICS").as_deref() == Ok("verbose")
}

impl fmt::Display for DevelopmentDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.phase, self.message)?;
        if let Some(source) = &self.source {
            write!(f, "\n{source}")?;
            if let Some(location) = &self.location {
                write!(f, ":{location}")?;
            }
        }
        if let Some(detail) = &self.detail {
            write!(f, "\n\n{detail}")?;
        }
        if let Some(suggestion) = self.suggestion {
            write!(f, "\n\nTry: {suggestion}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for DevelopmentDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for DevelopmentDiagnostic {}

#[cfg(test)]
mod tests {
    use super::*;
    use plec_build::{BuildOptions, RuntimeSource, modules::host::resolve_host_config};
    use std::path::PathBuf;

    #[test]
    fn compiler_diagnostic_keeps_module_and_source_position() {
        let directory = tempfile::tempdir().expect("source root");
        let routes = directory.path().join("src/routes");
        std::fs::create_dir_all(&routes).unwrap();
        let source = routes.join("todos.tsx");
        std::fs::write(&source, "export const = 1;").unwrap();
        let error = plec_compiler::read_source_graph_with_options(
            &source,
            directory.path(),
            directory.path(),
            &Default::default(),
        )
        .expect_err("unsupported source must be diagnosed");
        let diagnostic = DevelopmentDiagnostic::compiler(error);
        let output = diagnostic.to_string();
        assert!(output.contains("[PLEC-PARSE-001] parse:"));
        assert!(output.contains("src/routes/todos.tsx:1:"));
        assert!(output.contains("TSX syntax could not be parsed"));
    }

    #[test]
    fn invalid_plec_config_is_presented_as_a_build_diagnostic() {
        let directory = tempfile::tempdir().expect("app directory");
        std::fs::write(directory.path().join("plec.toml"), "[compiler\n").unwrap();
        let options = BuildOptions {
            source: PathBuf::from("src/router.tsx"),
            client_entry: PathBuf::from("src/client.tsx"),
            server_entry: PathBuf::from("src/server.ts"),
            out_dir: PathBuf::from("dist"),
            optimize: false,
            title: String::new(),
            description: None,
            styles_href: None,
            preloads: Vec::new(),
            runtime_source: RuntimeSource::Auto,
        };
        let error = resolve_host_config(directory.path(), &options).unwrap_err();
        let output = DevelopmentDiagnostic::build(error).to_string();
        assert!(output.contains("[PLEC-BUILD-CONFIG] build:"));
        assert!(output.contains("plec.toml"));
        assert!(output.contains("Try: Review the Plec configuration"));
    }
}
