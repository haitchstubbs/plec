use super::doctor::{compile_application, resolve_graph, resolved_component, GraphResolution};
use super::repo::Repo;
use plec_ir::{ComponentApplication, ExecutableComponent, Node};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressReport {
    pub address: String,
    pub segments: Vec<AddressSegment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressSegment {
    pub kind: String,
    pub value: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub ok: bool,
    pub graph: String,
    pub expected: Vec<String>,
    pub actual: Vec<String>,
    pub problems: Vec<String>,
}

pub fn explain(address: &str) -> Result<AddressReport, String> {
    let segments = parse_address(address)?;
    Ok(AddressReport {
        address: address.into(),
        segments,
    })
}

pub fn print_explain(report: &AddressReport) {
    println!("{}", report.address);
    for segment in &report.segments {
        println!(
            "{}{}: {}",
            "  ".repeat(segment.depth),
            segment.kind,
            segment.value
        );
    }
}

pub fn validate(
    repo: &Repo,
    source: &Path,
    graph: &str,
    html: &Path,
) -> Result<ValidationReport, String> {
    let compiled = compile_application(repo, source)?;
    let resolution = resolve_graph(&compiled, graph);
    let Some(component) = resolved_component(&compiled, &resolution) else {
        return Err(format!("graph {graph:?} is not registered"));
    };
    let application = match resolution {
        GraphResolution::Direct {
            ref registry_key, ..
        }
        | GraphResolution::RegisteredComponent {
            ref registry_key, ..
        } => compiled.registry.get(registry_key).unwrap(),
        GraphResolution::Missing => unreachable!(),
    };
    let mut expected = Vec::new();
    walk(
        application,
        component,
        component.root_node,
        "root",
        &mut BTreeSet::new(),
        &mut expected,
    );
    let path = if html.is_absolute() {
        html.to_path_buf()
    } else {
        repo.root.join(html)
    };
    let body =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let actual = extract_markers(&body);
    let mut problems = Vec::new();
    for marker in &expected {
        if !actual
            .iter()
            .any(|candidate| marker_matches(marker, candidate))
        {
            let code = if marker.starts_with("component-end:") {
                "missing:ssr-component"
            } else if marker.starts_with("text:") {
                "missing:ssr-text"
            } else if marker.starts_with("conditional") {
                "mismatch:ssr-branch"
            } else {
                "missing:ssr-node"
            };
            problems.push(format!("{code}: {marker}; expected <!--plec:{marker}-->"));
            break;
        }
    }
    let mut seen = BTreeSet::new();
    for marker in &actual {
        if !seen.insert(marker) {
            problems.push(format!("duplicate:ssr-marker:{marker}"));
            break;
        }
        if let Some((kind, address)) = marker.split_once(':') {
            let valid = if kind == "text" {
                parse_text_address(address).is_ok()
            } else if matches!(
                kind,
                "component"
                    | "component-end"
                    | "conditional"
                    | "conditional-end"
                    | "slot"
                    | "slot-end"
            ) {
                parse_boundary_address(address).is_ok()
            } else {
                parse_address(address).is_ok()
            };
            if !valid {
                problems.push(format!("invalid:ssr-marker:{marker}"));
                break;
            }
        }
    }
    for (offset, marker) in marker_comments_with_offsets(&body) {
        if marker.starts_with("text:") {
            let after = &body[offset + marker.len() + 12..];
            let next = after.trim_start();
            if !next.starts_with("<!---->") && next.starts_with('<') {
                problems.push(format!("adjacency:ssr-text:{marker}"));
                break;
            }
        }
    }
    Ok(ValidationReport {
        ok: problems.is_empty(),
        graph: graph.into(),
        expected,
        actual,
        problems,
    })
}

pub fn print_validation(report: &ValidationReport) {
    println!("graph {}", report.graph);
    println!("  expected markers  {}", report.expected.len());
    println!("  actual markers    {}", report.actual.len());
    for problem in &report.problems {
        println!("  ✗ {problem}");
    }
    if report.ok {
        println!("✓ markers valid");
    } else {
        println!("✗ markers invalid");
    }
}

fn walk(
    app: &ComponentApplication,
    component: &ExecutableComponent,
    index: usize,
    path: &str,
    ancestors: &mut BTreeSet<usize>,
    out: &mut Vec<String>,
) {
    if !ancestors.insert(index) {
        return;
    }
    let Some(node) = component.nodes.get(index) else {
        ancestors.remove(&index);
        return;
    };
    match node {
        Node::Element { children, .. } => {
            out.push(format!("node:{path}/node:{index}"));
            for child in children {
                walk(app, component, *child, path, ancestors, out);
            }
        }
        Node::Text { .. } => out.push(format!("text:{path}:{index}")),
        Node::Conditional {
            consequent,
            alternate,
            ..
        } => {
            out.push(format!("conditional:{path}:{index}"));
            let _ = (consequent, alternate);
            out.push(format!("conditional-end:{path}:{index}"));
        }
        Node::Component {
            component: target,
            children,
            ..
        } => {
            let child_path = format!("{path}/component:{index}");
            out.push(format!("component:{path}:{index}"));
            if let Some(child) = app.components.get(*target) {
                walk(
                    app,
                    child,
                    child.root_node,
                    &child_path,
                    &mut BTreeSet::new(),
                    out,
                );
            }
            for child in children {
                walk(app, component, *child, path, ancestors, out);
            }
            out.push(format!("component-end:{path}:{index}"));
        }
        Node::Slot { .. } => {
            out.push(format!("slot:{path}:{index}"));
            out.push(format!("slot-end:{path}:{index}"));
        }
        Node::Loop { r#loop, .. } => {
            let row_path = format!("{path}/loop:{index}/key:*");
            out.push(format!("loop:{row_path}"));
            if let Some(info) = component.loops.get(*r#loop) {
                walk(app, component, info.row_template, &row_path, ancestors, out);
            }
            out.push(format!("loop-end:{row_path}"));
        }
        Node::DynamicComponent { .. } | Node::HostComponent { .. } => {
            out.push(format!("node:{path}/node:{index}"));
        }
    }
    ancestors.remove(&index);
}

fn marker_matches(expected: &str, actual: &str) -> bool {
    if let Some(prefix) = expected.strip_suffix('*') {
        actual.starts_with(prefix)
    } else {
        expected == actual
    }
}

fn parse_text_address(address: &str) -> Result<Vec<AddressSegment>, String> {
    let Some((path, index)) = address.rsplit_once(':') else {
        return Err("text marker needs a numeric node handle".into());
    };
    if index.parse::<usize>().is_err() {
        return Err("text marker needs a numeric node handle".into());
    }
    parse_address(&format!("{path}/node:{index}"))
}

fn parse_boundary_address(address: &str) -> Result<Vec<AddressSegment>, String> {
    let Some((path, index)) = address.rsplit_once(':') else {
        return Err("boundary marker needs a numeric node handle".into());
    };
    if index.parse::<usize>().is_err() {
        return Err("boundary marker needs a numeric node handle".into());
    }
    parse_address(path)
}

fn extract_markers(html: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<!--plec:") {
        let body = &rest[start + 9..];
        let Some(end) = body.find("-->") else {
            break;
        };
        result.push(body[..end].to_owned());
        rest = &body[end + 3..];
    }
    let needle = "data-plec-node=\"";
    let mut rest = html;
    while let Some(start) = rest.find(needle) {
        let value = &rest[start + needle.len()..];
        let Some(end) = value.find('"') else {
            break;
        };
        result.push(format!("node:{}", &value[..end]));
        rest = &value[end + 1..];
    }
    result
}

fn marker_comments_with_offsets(html: &str) -> Vec<(usize, String)> {
    let mut result = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = html[cursor..].find("<!--plec:") {
        let start = cursor + relative;
        let body_start = start + "<!--plec:".len();
        let Some(end) = html[body_start..].find("-->") else {
            break;
        };
        result.push((start, html[body_start..body_start + end].to_owned()));
        cursor = body_start + end + 3;
    }
    result
}

fn parse_address(address: &str) -> Result<Vec<AddressSegment>, String> {
    let parts: Vec<_> = address.split('/').collect();
    if parts.first() != Some(&"root") {
        return Err("must start with 'root'".into());
    }
    let mut result = vec![AddressSegment {
        kind: "root".into(),
        value: "root".into(),
        depth: 0,
    }];
    for (depth, part) in parts.iter().enumerate().skip(1) {
        let Some((kind, value)) = part.split_once(':') else {
            return Err(format!("segment {part:?} is not kind:value"));
        };
        if value.is_empty() {
            return Err(format!("segment {part:?} has an empty value"));
        }
        match kind {
            "outlet" | "key" => {}
            "component" | "loop" | "node" if value.parse::<usize>().is_ok() => {}
            _ => return Err(format!("invalid segment {part:?}")),
        }
        result.push(AddressSegment {
            kind: kind.into(),
            value: value.into(),
            depth,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explains_address() {
        let r = explain("root/component:2/loop:1/key:a%2Fb/node:3").unwrap();
        assert_eq!(r.segments.len(), 5);
    }
    #[test]
    fn rejects_bad_address() {
        assert!(explain("page/node:1").is_err());
        assert!(explain("root/node:x").is_err());
    }
}
