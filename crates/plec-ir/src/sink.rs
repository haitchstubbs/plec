//! The single DOM-sink policy for executable Plec applications.
//!
//! Every path that turns executable IR into a DOM mutation goes through this
//! policy: the compiler rejects hostile authored props, typed-IR validation
//! (`plec_schema::typed`) rejects substituted artifacts, the runtime binding
//! applier (`plec_client::bindings`) enforces it at apply time, and the SSR
//! serializer in `crates/plec-server` mirrors it. Keep the four surfaces in
//! lockstep; this module is the authority.

/// Attribute names owned by the structural DOM address protocol
/// (docs/dom-address-protocol.md). Authored or substituted IR must never
/// collide with them: a duplicate `data-plec-node` fails adoption for the
/// whole page. HTML attribute lookup is case-insensitive, so the reserved
/// prefixes are matched ASCII-case-insensitively.
pub fn is_reserved_attribute_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("data-plec-") || lower.starts_with("data-runtime-") || lower.starts_with("plec:")
}

/// The strict name grammar shared by HTML and SVG-safe names: an ASCII
/// letter, then ASCII letters, digits, and the separator characters HTML,
/// SVG, and ARIA rely on (`:`, `.`, `_`, `-`). Anything else — whitespace,
/// quotes, `=`, `<`, `/`, non-ASCII — could smuggle markup through a
/// serializer that interpolates the name verbatim, so it is rejected here.
fn is_safe_name_grammar(name: &str) -> bool {
    let mut characters = name.chars();
    let first = characters.next();
    match first {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, ':' | '.' | '_' | '-')
    })
}

/// Element tag names writable through executable IR: the same strict grammar
/// applied to attribute names, minus the namespace separator, which no HTML
/// or SVG element needs. Rejects markup-bearing or whitespace-bearing tags
/// before any serializer can interpolate them.
pub fn is_safe_tag_name(name: &str) -> bool {
    if name.is_empty() || !name.is_ascii() {
        return false;
    }
    // No HTML or SVG element carries a namespace separator; only attribute
    // names (e.g. `xlink:href`) do.
    if name.contains(':') {
        return false;
    }
    is_safe_name_grammar(name)
}

/// Attribute names the executable runtime may ever write.
///
/// HTML attribute lookup is case-insensitive, so `ONCLICK` is a live event
/// handler exactly like `onclick`; the check is therefore case-insensitive.
/// Declared Plec events go through the event contract instead, never through
/// attributes. Names must also satisfy the strict HTML/SVG-safe grammar so a
/// serializer that interpolates them verbatim cannot be turned into a markup
/// injection channel.
pub fn is_safe_attribute_name(name: &str) -> bool {
    if name.is_empty() || !name.is_ascii() {
        return false;
    }
    if !is_safe_name_grammar(name) {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    // Event-handler attributes are script sinks.
    if lower.starts_with("on") {
        return false;
    }
    // `srcdoc` is an inline document sink; a framed document inherits the
    // embedder origin and bypasses every URL policy.
    if lower == "srcdoc" {
        return false;
    }
    !is_reserved_attribute_name(name)
}

/// Property names writable through the explicit `property` sink. This is an
/// allowlist, not a blocklist: arbitrary property writes would let
/// substituted IR select sinks such as `innerHTML`. The sink exists only for
/// live form-control semantics that attributes cannot express.
pub fn is_safe_property_name(name: &str) -> bool {
    matches!(name, "checked" | "disabled" | "value")
}

/// Values written to URL-bearing attributes must not resolve to a script
/// scheme. Browsers strip ASCII whitespace and control characters before
/// scheme parsing, so the normalization mirrors that behaviour.
pub fn is_safe_attribute_value(name: &str, value: &str) -> bool {
    let url_attribute = matches!(
        name.to_ascii_lowercase().as_str(),
        "href"
            | "src"
            | "action"
            | "formaction"
            | "xlink:href"
            | "poster"
            | "background"
            | "cite"
            | "data"
            | "longdesc"
            | "ping"
    );
    if !url_attribute {
        return true;
    }
    let normalized: String = value
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized.starts_with("javascript:") || normalized.starts_with("vbscript:") {
        return false;
    }
    // `data:` documents execute in navigation and framing contexts; only
    // raster image payloads stay allowed.
    if normalized.starts_with("data:") {
        return [
            "data:image/png",
            "data:image/jpeg",
            "data:image/gif",
            "data:image/webp",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_event_handler_attributes_regardless_of_case() {
        for name in [
            "onclick",
            "ONCLICK",
            "OnClick",
            "onerror",
            "onmouseover",
            "onfocusin",
        ] {
            assert!(!is_safe_attribute_name(name), "{name} must be rejected");
        }
    }

    #[test]
    fn rejects_srcdoc_and_reserved_namespaces() {
        assert!(!is_safe_attribute_name("srcdoc"));
        assert!(!is_safe_attribute_name("SRCDOC"));
        assert!(!is_safe_attribute_name("data-plec-node"));
        assert!(!is_safe_attribute_name("data-runtime-row-key"));
        assert!(!is_safe_attribute_name("plec:kind"));
    }

    #[test]
    fn rejects_reserved_namespaces_regardless_of_case() {
        assert!(!is_safe_attribute_name("DATA-PLEC-NODE"));
        assert!(!is_safe_attribute_name("Data-Runtime-Row-Key"));
        assert!(!is_safe_attribute_name("PLEC:kind"));
        assert!(is_reserved_attribute_name("DATA-PLEC-NODE"));
        assert!(is_reserved_attribute_name("Data-Runtime-Row-Key"));
        assert!(is_reserved_attribute_name("PLEC:kind"));
    }

    #[test]
    fn rejects_empty_and_non_ascii_names() {
        assert!(!is_safe_attribute_name(""));
        assert!(!is_safe_attribute_name("hreﬀ"));
    }

    #[test]
    fn rejects_names_outside_the_html_grammar() {
        // Markup/attribute-boundary smuggling through interpolated names.
        for name in [
            "a href",
            "a\thref",
            "a\nhref",
            "a/b",
            "a=b",
            "a>b",
            "a<b",
            "a\"b",
            "a'b",
            "a`b",
            "a=b href",
            "href ",
            " href",
            "1abc",
            "-abc",
            ":abc",
            ".abc",
        ] {
            assert!(
                !is_safe_attribute_name(name),
                "{name:?} must be rejected"
            );
        }
    }

    #[test]
    fn allows_grammar_safe_attribute_names() {
        for name in [
            "class",
            "className",
            "href",
            "HREF",
            "viewBox",
            "xlink:href",
            "xml:lang",
            "data-id",
            "aria-label",
            "stroke-width",
            "http-equiv",
        ] {
            assert!(is_safe_attribute_name(name), "{name} must be allowed");
        }
    }

    #[test]
    fn tag_names_follow_the_strict_html_svg_grammar() {
        for tag in [
            "div",
            "p",
            "h1",
            "a",
            "svg",
            "clipPath",
            "feGaussianBlur",
            "font-face",
            "linearGradient",
            "my-element",
        ] {
            assert!(is_safe_tag_name(tag), "{tag} must be allowed");
        }
        for tag in [
            "",
            "img src=x onerror=alert(1)",
            "img src=x",
            "<script>",
            "script>",
            "a/b",
            "svg:path",
            "div ",
            " div",
            "1div",
            "dïv",
        ] {
            assert!(!is_safe_tag_name(tag), "{tag:?} must be rejected");
        }
    }

    #[test]
    fn allows_supported_attribute_names() {
        for name in [
            "class",
            "className",
            "href",
            "value",
            "data-id",
            "aria-label",
        ] {
            assert!(is_safe_attribute_name(name), "{name} must be allowed");
        }
    }

    #[test]
    fn property_sink_is_an_allowlist() {
        assert!(is_safe_property_name("checked"));
        assert!(is_safe_property_name("disabled"));
        assert!(is_safe_property_name("value"));
        for name in [
            "innerHTML",
            "outerHTML",
            "srcdoc",
            "src",
            "href",
            "formAction",
        ] {
            assert!(!is_safe_property_name(name), "{name} must be rejected");
        }
    }

    #[test]
    fn rejects_script_url_schemes_after_browser_normalization() {
        for value in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            "  javascript:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "\u{0}javascript:alert(1)",
            "vbscript:msgbox(1)",
            "VBSCRIPT:msgbox(1)",
        ] {
            assert!(
                !is_safe_attribute_value("href", value),
                "{value:?} must be rejected"
            );
        }
    }

    #[test]
    fn allows_safe_urls_and_non_url_attributes() {
        assert!(is_safe_attribute_value("href", "https://plec.dev"));
        assert!(is_safe_attribute_value("href", "/routes"));
        assert!(is_safe_attribute_value("href", "#anchor"));
        assert!(is_safe_attribute_value("src", "data:image/png;base64,AAAA"));
        assert!(is_safe_attribute_value("title", "javascript:alert(1)"));
    }

    #[test]
    fn rejects_data_documents_outside_raster_images() {
        assert!(!is_safe_attribute_value("src", "data:text/html,<script>"));
        assert!(!is_safe_attribute_value(
            "href",
            "data:image/svg+xml,<svg onload=alert(1)>"
        ));
    }
}
