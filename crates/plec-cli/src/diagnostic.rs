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
        Self::compiler_message(error.to_string(), verbose())
    }

    fn compiler_message(raw: String, include_detail: bool) -> Self {
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
            detail: include_detail.then_some(raw),
            suggestion: has_source
                .then_some("Check the reported source location for a TypeScript/TSX syntax error."),
        }
    }

    pub(crate) fn build(error: BuildError) -> Self {
        if error.stage == Stage::Compile {
            if let Some(parse_error) = error.message.find("Failed to parse ") {
                return Self::compiler_message(error.message[parse_error..].to_owned(), verbose());
            }
        }
        let (code, phase) = match error.stage {
            Stage::Configuration => ("PLEC-BUILD-CONFIG", "build"),
            Stage::Compile => ("PLEC-COMPILE-001", "compile"),
            Stage::ApiRoutes => ("PLEC-DISCOVERY-ROUTES", "discovery"),
            Stage::DependencyValidation => ("PLEC-BUILD-FAILURE", "dependency validation"),
            _ => ("PLEC-BUILD-FAILURE", "build"),
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
    verbose_from(std::env::var("PLEC_DIAGNOSTICS").ok().as_deref())
}

fn verbose_from(setting: Option<&str>) -> bool {
    setting == Some("verbose")
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
    fn verbose_mode_is_opted_in_only_by_the_documented_value() {
        assert!(!verbose_from(None));
        assert!(!verbose_from(Some("true")));
        assert!(verbose_from(Some("verbose")));
    }

    #[test]
    fn compiler_diagnostic_keeps_root_and_imported_source_positions() {
        let directory = tempfile::tempdir().expect("source root");
        let routes = directory.path().join("src/routes");
        std::fs::create_dir_all(&routes).unwrap();
        let entry = directory.path().join("src/router.tsx");
        std::fs::write(&entry, "import './routes/todos';").unwrap();
        let imported = routes.join("todos.tsx");
        std::fs::write(&imported, "export const = 1;").unwrap();

        let imported_error = plec_compiler::read_source_graph_with_options(
            &entry,
            directory.path(),
            directory.path(),
            &Default::default(),
        )
        .expect_err("invalid imported source must be diagnosed");
        let imported_diagnostic = DevelopmentDiagnostic::compiler(imported_error);
        let imported_output = imported_diagnostic.to_string();
        assert!(imported_output.contains("[PLEC-PARSE-001] parse:"));
        assert!(imported_output.contains("src/routes/todos.tsx:1:"));
        assert_eq!(imported_output.matches("src/routes/todos.tsx").count(), 1);
        assert!(imported_output.contains("TSX syntax could not be parsed"));

        let root = directory.path().join("src/entry.tsx");
        std::fs::write(&root, "export const = 1;").unwrap();
        let root_error = plec_compiler::read_source_graph_with_options(
            &root,
            directory.path(),
            directory.path(),
            &Default::default(),
        )
        .expect_err("invalid entry source must be diagnosed");
        let root_output = DevelopmentDiagnostic::compiler(root_error).to_string();
        assert!(root_output.contains("src/entry.tsx:1:"));
        assert_eq!(root_output.matches("src/entry.tsx").count(), 1);
    }

    #[test]
    fn verbose_compiler_diagnostic_adds_original_detail_only_when_requested() {
        let raw = "Failed to parse src/app.tsx: src/app.tsx:2:7: UnexpectedToken".to_owned();
        let concise = DevelopmentDiagnostic::compiler_message(raw.clone(), false).to_string();
        let verbose = DevelopmentDiagnostic::compiler_message(raw.clone(), true).to_string();
        assert!(concise.contains("src/app.tsx:2:7"));
        assert!(!concise.contains("UnexpectedToken"));
        assert!(verbose.contains("UnexpectedToken"));
    }

    #[test]
    fn build_preserves_parse_diagnostic_code_and_source_location() {
        let error = BuildError::new(
            Stage::Compile,
            "compile failed: Failed to parse src/home.tsx: src/home.tsx:4:9: UnexpectedToken",
        );
        let diagnostic = DevelopmentDiagnostic::build(error).to_string();
        assert!(diagnostic.contains("[PLEC-PARSE-001] parse:"));
        assert!(diagnostic.contains("src/home.tsx:4:9"));
    }

    #[test]
    fn dependency_validation_keeps_its_build_phase_in_the_cli_diagnostic() {
        let error = BuildError::new(
            Stage::DependencyValidation,
            "Vite production build failed: forbidden browser dependency",
        );
        let diagnostic = DevelopmentDiagnostic::build(error).to_string();
        assert!(diagnostic.contains("[PLEC-BUILD-FAILURE] dependency validation:"));
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
        assert_eq!(error.stage, Stage::Configuration);
        let output = DevelopmentDiagnostic::build(error).to_string();
        assert!(output.contains("[PLEC-BUILD-CONFIG] build:"));
        assert!(output.contains("plec.toml"));
        assert!(output.contains("Try: Review the Plec configuration"));
    }
}
