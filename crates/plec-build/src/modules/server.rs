use std::path::Path;

use super::api_routes::{ApiRoute, Segment};
use super::build::{BuildError, Stage};
use super::esbuild;

/// Generate and bundle the application server entry. Routes run before the
/// optional application-owned fallback handler.
pub fn bundle(
    app_dir: &Path,
    fallback_entry: Option<&Path>,
    routes: &[ApiRoute],
    outfile: &Path,
    optimize: bool,
) -> Result<(), BuildError> {
    let stage = Stage::ServerBundle;
    let esbuild_bin = esbuild::resolve(app_dir).map_err(|error| BuildError::new(stage, error))?;
    let source = generated_source(app_dir, fallback_entry, routes);
    let mut args = vec![
        esbuild_bin.to_string_lossy().into_owned(),
        "--bundle".into(),
        "--format=esm".into(),
        "--tree-shaking=true".into(),
        "--legal-comments=none".into(),
        "--platform=node".into(),
        "--target=node20".into(),
        "--packages=external".into(),
        "--loader:.ts=ts".into(),
        format!(
            "--sourcefile={}",
            app_dir.join("plec-generated-server.ts").display()
        ),
        format!("--outfile={}", outfile.display()),
    ];
    if optimize {
        args.push("--minify".into());
    }
    return esbuild::run_with_stdin(&args, &source, app_dir)
        .map_err(|error| BuildError::new(stage, error));
}

fn generated_source(app_dir: &Path, fallback_entry: Option<&Path>, routes: &[ApiRoute]) -> String {
    let mut source = String::new();
    for (index, route) in routes.iter().enumerate() {
        source.push_str(&format!(
            "import * as route{index} from \"{}\";\n",
            route.import_path
        ));
    }
    if fallback_entry.is_some() {
        source.push_str("import * as fallbackModule from \"");
        source.push_str(&relative_import(app_dir, fallback_entry.expect("checked")));
        source.push_str("\";\nconst fallbackHandleRequest = fallbackModule.handleRequest;\n");
    } else {
        source.push_str("const fallbackHandleRequest = undefined;\n");
    }
    source.push_str("\nconst routes = [\n");
    for (index, route) in routes.iter().enumerate() {
        source.push_str(&format!(
            "  {{ module: route{index}, segments: {}, methods: {} }},\n",
            segments_source(route),
            methods_source()
        ));
    }
    source.push_str(r#"];

function matchRoute(pathname) {
  const path = pathname === '/api' ? [] : pathname.replace(/^\/api\//, '').split('/');
  if (pathname !== '/api' && !pathname.startsWith('/api/')) return undefined;
  for (const route of routes) {
    if (route.segments.length !== path.length) continue;
    const params = {};
    let matched = true;
    for (let index = 0; index < route.segments.length; index += 1) {
      const segment = route.segments[index];
      if (segment[0] === 'static' && segment[1] !== path[index]) {
        matched = false;
        break;
      }
      if (segment[0] === 'dynamic') {
        try {
          params[segment[1]] = decodeURIComponent(path[index]);
        } catch {
          matched = false;
          break;
        }
      }
    }
    if (matched) return { route, params };
  }
  return undefined;
}

export async function handleRequest(request, context) {
  const matched = matchRoute(context.pathname);
  if (matched) {
    const routeContext = { ...context, params: matched.params };
    const method = request.method.toUpperCase();
    const handler = matched.route.module[method];
    const allow = matched.route.methods.filter((name) =>
      typeof matched.route.module[name] === 'function',
    );
    if (method === 'OPTIONS' && typeof handler !== 'function') {
      return new Response(null, { status: 204, headers: { Allow: [...allow, 'OPTIONS'].join(', ') } });
    }
    if (typeof handler !== 'function') {
      return new Response(null, { status: 405, headers: { Allow: [...allow, 'OPTIONS'].join(', ') } });
    }
    return handler(request, routeContext);
  }

  return typeof fallbackHandleRequest === 'function'
    ? fallbackHandleRequest(request, context)
    : undefined;
}
"#);

    source
}

fn relative_import(app_dir: &Path, entry: &Path) -> String {
    let relative = entry.strip_prefix(app_dir).unwrap_or(entry);
    format!(
        "./{}",
        relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
    )
}

fn segments_source(route: &ApiRoute) -> String {
    let values = route
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Static(value) => format!("['static', '{}']", js_string(value)),
            Segment::Dynamic(value) => format!("['dynamic', '{}']", js_string(value)),
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(", "))
}

fn methods_source() -> &'static str {
    "['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']"
}

fn js_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_dispatch_decodes_params_and_handles_method_rules() {
        let route = ApiRoute {
            source: "api/todos/[id].ts".into(),
            import_path: "./api/todos/[id].ts".into(),
            segments: vec![
                Segment::Static("todos".into()),
                Segment::Dynamic("id".into()),
            ],
            middleware: Vec::new(),
        };
        let source = generated_source(
            Path::new("/app"),
            Some(Path::new("/app/src/server.ts")),
            &[route],
        );
        assert!(source.contains("params[segment[1]] = decodeURIComponent(path[index])"));
        assert!(source.contains("fallbackModule.handleRequest"));
        assert!(source.contains("status: 405"));
        assert!(source.contains("status: 204"));
    }
}
