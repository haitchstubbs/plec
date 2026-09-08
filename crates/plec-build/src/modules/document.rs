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

    // Application metadata is untrusted input (`plec.toml` or a CLI flag);
    // the title lands in an HTML text context, so it must not be able to
    // close the element or inject markup.
    let title = escape_html(title);

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

/// Escape a value for an HTML text context. Mirrors the host's SSR escaping
/// (`plec-server` `ssr::escape_html`): the document shell and SSR documents
/// must agree on how application metadata renders.
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_index_to(title: &str) -> String {
        let dir = tempfile::tempdir().expect("dir");
        write_index(dir.path(), title, "rev0123456789").expect("document write");
        std::fs::read_to_string(dir.path().join("index.html")).expect("document read")
    }

    #[test]
    fn title_is_html_escaped() {
        let index = write_index_to("</title><script>alert(1)</script>");
        assert!(!index.contains("<script>"));
        assert!(
            index.contains("<title>&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;</title>")
        );
    }

    #[test]
    fn plain_title_passes_through_unchanged() {
        let index = write_index_to("Plec & friends <3");
        assert!(index.contains("<title>Plec &amp; friends &lt;3</title>"));
    }
}
