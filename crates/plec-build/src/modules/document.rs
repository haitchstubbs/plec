use std::{fs, path::Path};

use super::build::{BuildError, Stage};

/// Generate the Plec document shell.
///
/// Plec owns the document structure, mount root, client script injection and
/// the Plec-owned asset revision. Application metadata (the title) is passed
/// in rather than hardcoded.
///
/// This intentionally stays a plain static shell; it may later merge into
/// SSR document rendering, so nothing here assumes `index.html` is the final
/// document architecture.
pub fn write_index(public_dir: &Path, title: &str, revision: &str) -> Result<(), BuildError> {
    let index_path = public_dir.join("index.html");

    let document = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link
    rel="stylesheet"
    href="/assets/styles.css?v={revision}"
  >
</head>
<body>
  <div id="app" aria-live="polite"></div>
  <script
    type="module"
    src="/assets/client.js?v={revision}"
  ></script>
</body>
</html>
"#
    );

    fs::write(&index_path, document).map_err(|error| {
        BuildError::with_source(
            Stage::Document,
            format!("failed to write document {}", index_path.display()),
            error,
        )
    })
}
