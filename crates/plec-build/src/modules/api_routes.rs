use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Static(String),
    Dynamic(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRoute {
    pub source: PathBuf,
    pub import_path: String,
    pub segments: Vec<Segment>,
    pub middleware: Vec<ApiMiddleware>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiMiddleware {
    pub source: PathBuf,
    pub import_path: String,
}

impl ApiRoute {
    pub fn matcher(&self) -> String {
        if self.segments.is_empty() {
            "/api".into()
        } else {
            format!(
                "/api/{}",
                self.segments
                    .iter()
                    .map(|segment| match segment {
                        Segment::Static(value) => value.clone(),
                        Segment::Dynamic(_) => ":param".into(),
                    })
                    .collect::<Vec<_>>()
                    .join("/")
            )
        }
    }
}

pub fn discover(app_dir: &Path) -> Result<Vec<ApiRoute>, String> {
    let root = app_dir.join("api");
    if !root.is_dir() {
        return Ok(Vec::new());
    }

    let mut routes = Vec::new();
    discover_directory(&root, &root, &mut routes, &[])?;

    let mut collisions = BTreeMap::<String, ApiRoute>::new();
    for route in &routes {
        let key = route
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Static(value) => format!("s:{value}"),
                Segment::Dynamic(_) => "d".to_owned(),
            })
            .collect::<Vec<_>>()
            .join("/");
        if let Some(previous) = collisions.insert(key, route.clone()) {
            return Err(format!(
                "API route collision: {} and {} both match {}",
                display_source(&previous.source, app_dir),
                display_source(&route.source, app_dir),
                route.matcher()
            ));
        }
    }

    routes.sort_by(|left, right| {
        for (left_segment, right_segment) in left.segments.iter().zip(&right.segments) {
            let ordering = match (left_segment, right_segment) {
                (Segment::Static(left), Segment::Static(right)) => left.cmp(right),
                (Segment::Static(_), Segment::Dynamic(_)) => Ordering::Less,
                (Segment::Dynamic(_), Segment::Static(_)) => Ordering::Greater,
                (Segment::Dynamic(left), Segment::Dynamic(right)) => left.cmp(right),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        left.segments
            .len()
            .cmp(&right.segments.len())
            .then_with(|| left.source.cmp(&right.source))
    });
    Ok(routes)
}

fn discover_directory(
    root: &Path,
    directory: &Path,
    routes: &mut Vec<ApiRoute>,
    inherited_middleware: &[ApiMiddleware],
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot scan API directory {}: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot read API directory {}: {error}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    let middleware = entries
        .iter()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.path().is_file() && is_middleware_file(&name) {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if middleware.len() > 1 {
        return Err(format!(
            "API directory {} defines both _middleware.ts and _middleware.js",
            directory.display()
        ));
    }

    let mut middleware_scope = inherited_middleware.to_vec();
    if let Some(path) = middleware.first() {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("cannot relativize API middleware {}", path.display()))?;
        middleware_scope.push(ApiMiddleware {
            source: path.clone(),
            import_path: format!(
                "./api/{}",
                relative
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            ),
        });
    }

    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('_') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            discover_directory(root, &path, routes, &middleware_scope)?;
            continue;
        }
        if !path.is_file() || ignored_file(&name) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("cannot relativize API route {}", path.display()))?
            .to_path_buf();
        let mut components = relative.components().collect::<Vec<_>>();
        let file = components
            .pop()
            .expect("route file has a parent")
            .as_os_str()
            .to_string_lossy();
        let stem = file
            .strip_suffix(".ts")
            .or_else(|| file.strip_suffix(".js"))
            .expect("ignored non-route extension");

        let mut segments = components
            .into_iter()
            .map(|component| parse_segment(&component.as_os_str().to_string_lossy()))
            .collect::<Result<Vec<_>, _>>()?;
        if stem != "index" {
            segments.push(parse_segment(stem)?);
        }
        routes.push(ApiRoute {
            source: path,
            import_path: format!(
                "./api/{}",
                relative
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            ),
            segments,
            middleware: middleware_scope.clone(),
        });
    }
    Ok(())
}

fn is_middleware_file(name: &str) -> bool {
    name == "_middleware.ts" || name == "_middleware.js"
}

fn parse_segment(value: &str) -> Result<Segment, String> {
    if let Some(parameter) = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        if is_identifier(parameter) {
            return Ok(Segment::Dynamic(parameter.to_owned()));
        }
    }
    if value.contains('[') || value.contains(']') {
        return Err(format!(
            "invalid API route segment `{value}`: only [param] dynamic segments are supported; catch-all routes are not supported"
        ));
    }
    Ok(Segment::Static(value.to_owned()))
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn ignored_file(name: &str) -> bool {
    name.ends_with(".d.ts")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.js")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.js")
        || !(name.ends_with(".ts") || name.ends_with(".js"))
}

fn display_source(path: &Path, app_dir: &Path) -> String {
    path.strip_prefix(app_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(files: &[&str]) -> Result<Vec<ApiRoute>, String> {
        let dir = tempfile::tempdir().expect("temp dir");
        for file in files {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().expect("parent")).expect("parent");
            fs::write(path, "export function GET() { return new Response() }").expect("file");
        }
        discover(dir.path())
    }

    #[test]
    fn maps_index_nested_and_dynamic_routes() {
        let routes = scan(&[
            "api/index.ts",
            "api/users/me.ts",
            "api/users/[id].ts",
            "api/orgs/[orgId]/users.ts",
        ])
        .expect("routes");
        assert_eq!(routes[0].matcher(), "/api");
        assert_eq!(routes[1].matcher(), "/api/orgs/:param/users");
        assert_eq!(routes[2].matcher(), "/api/users/me");
        assert_eq!(routes[3].matcher(), "/api/users/:param");
        assert!(routes.iter().all(|route| route.middleware.is_empty()));
    }

    #[test]
    fn static_routes_sort_before_dynamic_routes() {
        let routes = scan(&["api/users/[id].ts", "api/users/me.ts"]).expect("routes");
        assert_eq!(routes[0].matcher(), "/api/users/me");
    }

    #[test]
    fn collisions_and_invalid_brackets_are_errors() {
        for files in [
            ["api/todos.ts", "api/todos/index.ts"],
            ["api/users/[id].ts", "api/users/[slug].ts"],
            ["api/todos.ts", "api/todos.js"],
        ] {
            let error = scan(&files).expect_err("collision");
            assert!(error.contains("api/") && error.contains("/api/"));
        }
        let error = scan(&["api/[...slug].ts"]).expect_err("catch all");
        assert!(error.contains("catch-all"));
    }

    #[test]
    fn ignores_private_and_test_files() {
        let routes = scan(&[
            "api/_private.ts",
            "api/_lib/helper.ts",
            "api/todos.test.ts",
            "api/todos.spec.js",
            "api/types.d.ts",
            "api/todos.ts",
        ])
        .expect("routes");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].matcher(), "/api/todos");
    }

    #[test]
    fn resolves_outer_to_inner_middleware_scopes() {
        let routes = scan(&[
            "api/_middleware.ts",
            "api/index.ts",
            "api/admin/_middleware.js",
            "api/admin/projects.ts",
            "api/other.ts",
        ])
        .expect("routes");

        let root = routes
            .iter()
            .find(|route| route.matcher() == "/api")
            .expect("root route");
        assert_eq!(root.middleware.len(), 1);
        assert_eq!(root.middleware[0].import_path, "./api/_middleware.ts");

        let admin = routes
            .iter()
            .find(|route| route.matcher() == "/api/admin/projects")
            .expect("admin route");
        assert_eq!(
            admin
                .middleware
                .iter()
                .map(|middleware| middleware.import_path.as_str())
                .collect::<Vec<_>>(),
            ["./api/_middleware.ts", "./api/admin/_middleware.js"]
        );

        let other = routes
            .iter()
            .find(|route| route.matcher() == "/api/other")
            .expect("sibling route");
        assert_eq!(other.middleware.len(), 1);
    }

    #[test]
    fn rejects_duplicate_middleware_files() {
        let error = scan(&["api/_middleware.ts", "api/_middleware.js", "api/todos.ts"])
            .expect_err("duplicate middleware");
        assert!(error.contains("both _middleware.ts and _middleware.js"));
    }

    #[test]
    fn middleware_files_are_not_routes_and_private_helpers_stay_ignored() {
        let routes = scan(&[
            "api/_middleware.ts",
            "api/_private.ts",
            "api/_lib/helper.ts",
        ]);
        let routes = routes.expect("routes");
        assert!(routes.is_empty());
    }
}
