//! The single DOM-sink policy for executable Plec applications.
//!
//! Every path that turns executable IR into a DOM mutation goes through this
//! policy: the compiler rejects hostile authored props, typed-IR validation
//! (`plec_schema::typed`) rejects substituted artifacts, the runtime binding
//! applier (`plec_client::bindings`) enforces it at apply time, and the SSR
//! serializer in `crates/plec-server` mirrors it. Keep the four surfaces in
//! lockstep; this module is the authority. Element instantiation is gated by
//! the element-tag allowlists below (`is_allowed_element_tag`); the host
//! component registry is a separate boundary outside this module.

/// Attribute names owned by the structural DOM address protocol
/// (docs/dom-address-protocol.md). Authored or substituted IR must never
/// collide with them: a duplicate `data-plec-node` fails adoption for the
/// whole page. HTML attribute lookup is case-insensitive, so the reserved
/// prefixes are matched ASCII-case-insensitively.
pub fn is_reserved_attribute_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("data-plec-")
        || lower.starts_with("data-runtime-")
        || lower.starts_with("plec:")
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

/// Tags that must never be instantiated or serialized from executable IR,
/// whatever a tag policy says. This is defense in depth behind the
/// allowlists below, not the primary boundary: the grammar check above
/// already passes every name here. Matching is ASCII-case-insensitive
/// because HTML-namespace element lookup is case-insensitive
/// (`create_element("SCRIPT")` creates a script element).
pub fn is_forbidden_element_tag(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "script"
            | "style"
            | "base"
            | "object"
            | "embed"
            | "iframe"
            | "frame"
            | "frameset"
            | "applet"
            | "portal"
            | "link"
            | "meta"
            | "noscript"
            | "html"
            | "head"
            | "body"
            | "title"
    )
}

/// The standard HTML element allowlist for executable IR: content,
/// sectioning, text-level, embedded-media, and form/table elements. Active,
/// embedding, and document-metadata elements live in
/// [`is_forbidden_element_tag`]. Membership is exact and canonical-lowercase,
/// so callers get deterministic rejection of case-mangled spellings.
fn is_standard_html_element_tag(name: &str) -> bool {
    matches!(
        name,
        "a" | "abbr"
            | "address"
            | "area"
            | "article"
            | "aside"
            | "audio"
            | "b"
            | "bdi"
            | "bdo"
            | "blockquote"
            | "br"
            | "button"
            | "canvas"
            | "caption"
            | "cite"
            | "code"
            | "col"
            | "colgroup"
            | "data"
            | "datalist"
            | "dd"
            | "del"
            | "details"
            | "dfn"
            | "dialog"
            | "div"
            | "dl"
            | "dt"
            | "em"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "i"
            | "img"
            | "input"
            | "ins"
            | "kbd"
            | "label"
            | "legend"
            | "li"
            | "main"
            | "map"
            | "mark"
            | "menu"
            | "meter"
            | "nav"
            | "ol"
            | "optgroup"
            | "option"
            | "output"
            | "p"
            | "picture"
            | "pre"
            | "progress"
            | "q"
            | "rp"
            | "rt"
            | "ruby"
            | "s"
            | "samp"
            | "search"
            | "section"
            | "select"
            | "slot"
            | "small"
            | "source"
            | "span"
            | "strong"
            | "sub"
            | "summary"
            | "sup"
            | "table"
            | "tbody"
            | "td"
            | "template"
            | "textarea"
            | "tfoot"
            | "th"
            | "thead"
            | "time"
            | "tr"
            | "track"
            | "u"
            | "ul"
            | "var"
            | "video"
            | "wbr"
    )
}

/// The standard SVG element allowlist for executable IR: the structural,
/// shape, text, paint-server, and clipping elements plus the filter
/// primitives icon sets rely on. Deliberately excludes `foreignObject`
/// (an HTML re-entry point), SMIL animation elements (which mutate
/// attributes outside the attribute policy), and `title`/`desc`
/// (`title` is forbidden in the HTML namespace and this policy keeps a
/// single identity rule per name).
fn is_standard_svg_element_tag(name: &str) -> bool {
    matches!(
        name,
        "svg"
            | "g"
            | "defs"
            | "symbol"
            | "use"
            | "circle"
            | "ellipse"
            | "line"
            | "polyline"
            | "polygon"
            | "path"
            | "rect"
            | "text"
            | "tspan"
            | "textPath"
            | "clipPath"
            | "mask"
            | "pattern"
            | "marker"
            | "linearGradient"
            | "radialGradient"
            | "stop"
            | "filter"
            | "feBlend"
            | "feColorMatrix"
            | "feComposite"
            | "feFlood"
            | "feGaussianBlur"
            | "feMerge"
            | "feMergeNode"
            | "feOffset"
    )
}

/// Whether `tag` is a standard (non-custom) element of `namespace`
/// (`"html"` or `"svg"`). Exposed for compiler diagnostics so authoring
/// errors surface at the earliest layer with the same identity rule the
/// runtime enforces.
pub fn is_standard_element_tag(tag: &str, namespace: &str) -> bool {
    match namespace {
        "html" => is_standard_html_element_tag(tag),
        "svg" => is_standard_svg_element_tag(tag),
        _ => false,
    }
}

/// The trusted element-tag capability for executable IR. Strict by default:
/// only standard HTML/SVG elements are allowed. Custom elements are rejected
/// unless the host explicitly configures them here; membership in
/// `custom_elements`, never the hyphen grammar, is the trust boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagPolicy {
    /// Trusted custom element tags (HTML namespace, canonical lowercase),
    /// e.g. `my-widget`.
    pub custom_elements: std::collections::BTreeSet<String>,
}

/// Whether executable IR may instantiate or serialize `tag` in `namespace`
/// under `policy`. The namespace must be exactly `"html"` or `"svg"`
/// (`TypedNode::Element.namespace` defaults to `"html"`); forbidden tags win
/// over every other rule.
pub fn is_allowed_element_tag_with_policy(tag: &str, namespace: &str, policy: &TagPolicy) -> bool {
    if namespace != "html" && namespace != "svg" {
        return false;
    }
    if is_forbidden_element_tag(tag) {
        return false;
    }
    if !is_safe_tag_name(tag) {
        return false;
    }
    if is_standard_element_tag(tag, namespace) {
        return true;
    }
    namespace == "html" && policy.custom_elements.contains(tag)
}

/// Strict default policy: standard HTML/SVG elements only, no custom
/// elements. This is the boundary untrusted/substituted artifacts face.
pub fn is_allowed_element_tag(tag: &str, namespace: &str) -> bool {
    is_allowed_element_tag_with_policy(tag, namespace, &TagPolicy::default())
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
            "a href", "a\thref", "a\nhref", "a/b", "a=b", "a>b", "a<b", "a\"b", "a'b", "a`b",
            "a=b href", "href ", " href", "1abc", "-abc", ":abc", ".abc",
        ] {
            assert!(!is_safe_attribute_name(name), "{name:?} must be rejected");
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

    #[test]
    fn forbidden_element_tags_are_rejected_in_every_namespace() {
        for tag in [
            "script", "SCRIPT", "Script", "style", "base", "object", "embed", "iframe", "frame",
            "frameset", "applet", "portal", "link", "meta", "noscript", "html", "head", "body",
            "title",
        ] {
            assert!(
                !is_allowed_element_tag(tag, "html"),
                "{tag} must be forbidden in html"
            );
            assert!(
                !is_allowed_element_tag(tag, "svg"),
                "{tag} must be forbidden in svg"
            );
        }
    }

    #[test]
    fn element_allowlist_accepts_corpus_html_and_svg_tags() {
        for (tag, namespace) in [
            ("a", "html"),
            ("article", "html"),
            ("aside", "html"),
            ("button", "html"),
            ("dd", "html"),
            ("div", "html"),
            ("dl", "html"),
            ("dt", "html"),
            ("em", "html"),
            ("form", "html"),
            ("h1", "html"),
            ("h2", "html"),
            ("header", "html"),
            ("input", "html"),
            ("label", "html"),
            ("li", "html"),
            ("main", "html"),
            ("nav", "html"),
            ("ol", "html"),
            ("p", "html"),
            ("section", "html"),
            ("span", "html"),
            ("strong", "html"),
            ("ul", "html"),
            ("img", "html"),
            ("table", "html"),
            ("template", "html"),
            ("svg", "svg"),
            ("circle", "svg"),
            ("path", "svg"),
            ("rect", "svg"),
            ("line", "svg"),
            ("polyline", "svg"),
            ("polygon", "svg"),
            ("ellipse", "svg"),
            ("g", "svg"),
            ("clipPath", "svg"),
            ("linearGradient", "svg"),
            ("feGaussianBlur", "svg"),
        ] {
            assert!(
                is_allowed_element_tag(tag, namespace),
                "{tag}/{namespace} must be allowed"
            );
        }
    }

    #[test]
    fn element_allowlist_rejects_unknown_tags_and_namespace_mismatches() {
        for (tag, namespace) in [
            ("div", "svg"),
            ("circle", "html"),
            ("clipPath", "html"),
            ("DIV", "html"),
            ("foo", "html"),
            ("my-element", "html"),
            ("my-element", "svg"),
            ("foreignObject", "svg"),
            ("animate", "svg"),
            ("div", "HTML"),
            ("div", ""),
            ("div", "math"),
            ("annotation-xml", "html"),
        ] {
            assert!(
                !is_allowed_element_tag(tag, namespace),
                "{tag:?}/{namespace:?} must be rejected"
            );
        }
    }

    #[test]
    fn custom_elements_require_explicit_policy_membership() {
        let policy = TagPolicy {
            custom_elements: std::collections::BTreeSet::from([String::from("my-widget")]),
        };
        assert!(is_allowed_element_tag_with_policy(
            "my-widget",
            "html",
            &policy
        ));
        assert!(!is_allowed_element_tag_with_policy(
            "my-other", "html", &policy
        ));
        assert!(!is_allowed_element_tag_with_policy(
            "my-widget",
            "svg",
            &policy
        ));
        assert!(!is_allowed_element_tag_with_policy(
            "MY-WIDGET",
            "html",
            &policy
        ));
        // Strict default rejects the same tag the policy allows.
        assert!(!is_allowed_element_tag("my-widget", "html"));

        // Forbidden tags cannot be re-enabled through the policy.
        let hostile = TagPolicy {
            custom_elements: std::collections::BTreeSet::from([
                String::from("script"),
                String::from("iframe"),
            ]),
        };
        assert!(!is_allowed_element_tag_with_policy(
            "script", "html", &hostile
        ));
        assert!(!is_allowed_element_tag_with_policy(
            "iframe", "html", &hostile
        ));
    }
}
