//! Failure-boundary tests for the Node application runtime. These run the
//! real `packages/plec-node-runtime` sidecar script against fixture bundles
//! and pin the contract where it is easiest to get wrong: startup, death,
//! the unhandled sentinel, and token enforcement.

use std::{path::Path, path::PathBuf, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use plec_server::{
    create_plec_server, request::RequestContext, runtime::NodeRuntimeOptions, ApplicationRuntime,
    DocumentMetadata, NodeApplicationRuntime, PlecServerOptions, ServerError,
};
use tower::ServiceExt;

/// Compiles the real `packages/plec-node-runtime` TypeScript source once per
/// test process with the workspace's own esbuild, so the failure-boundary
/// suite always exercises the artifact the build pipeline ships.
fn runtime_script() -> PathBuf {
    static COMPILED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    COMPILED
        .get_or_init(|| {
            let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let repo = repo.canonicalize().expect("repo root");
            let esbuild = repo
                .ancestors()
                .find_map(|ancestor| {
                    let candidate = ancestor
                        .join("node_modules")
                        .join("esbuild")
                        .join("bin")
                        .join("esbuild");
                    candidate.is_file().then_some(candidate)
                })
                .expect("esbuild not found: install workspace dependencies");
            let dir =
                std::env::temp_dir().join(format!("plec-node-runtime-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("test runtime dir");
            let outfile = dir.join("runtime.mjs");
            let output = std::process::Command::new("node")
                .arg(&esbuild)
                .args([
                    "--bundle",
                    "--format=esm",
                    "--platform=node",
                    "--target=node20",
                ])
                .arg(format!("--outfile={}", outfile.display()))
                .arg(repo.join("packages/plec-node-runtime/src/runtime.ts"))
                .output()
                .expect("invoke esbuild for the sidecar script");
            assert!(
                output.status.success(),
                "sidecar script bundling failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            outfile
        })
        .clone()
}

/// Writes a fixture bundle and spawns the real sidecar against it.
async fn spawn_runtime(bundle_source: &str) -> (NodeApplicationRuntime, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("fixture dir");
    std::fs::write(dir.path().join("app.mjs"), bundle_source).expect("bundle write");
    let runtime = NodeApplicationRuntime::spawn(NodeRuntimeOptions::new(
        runtime_script(),
        dir.path().join("app.mjs"),
    ))
    .await
    .expect("sidecar spawn");
    (runtime, dir)
}

fn test_context(method: &str, uri: &str) -> RequestContext {
    let url = format!("http://127.0.0.1:3000{uri}");
    RequestContext {
        url: url.clone(),
        pathname: uri.split('?').next().unwrap_or("/").to_owned(),
        method: method.parse().expect("valid method"),
        headers: Default::default(),
        cookies: Default::default(),
        params: Default::default(),
        query: Default::default(),
    }
}

async fn dispatch(
    runtime: &NodeApplicationRuntime,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> Result<(StatusCode, String), ServerError> {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = runtime
        .dispatch(request, test_context(method, uri))
        .await?
        .expect("fixture handler always responds");
    let status = response.status();
    let body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    Ok((status, body))
}

#[tokio::test]
async fn dispatches_requests_with_bodies_and_request_state() {
    let (runtime, _dir) = spawn_runtime(
        r#"
        export async function handleRequest(request, context) {
          const body = await request.json();
          return Response.json({
            echoed: body,
            method: context.method,
            query: context.query,
          });
        }
      "#,
    )
    .await;

    let (status, body) = dispatch(
        &runtime,
        "POST",
        "/api/todos?active=true",
        Some(r#"{"title":"Ship Plec"}"#),
    )
    .await
    .expect("dispatch");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""echoed":{"title":"Ship Plec"}"#), "{body}");
    assert!(body.contains(r#""method":"POST""#));
    assert!(body.contains(r#""query":{"active":"true"}"#));

    runtime.shutdown().await;
}

#[tokio::test]
async fn application_responses_pass_through_verbatim() {
    let (runtime, _dir) = spawn_runtime(
        r#"
        export async function handleRequest() {
          return new Response('app payload', {
            status: 404,
            headers: { 'content-type': 'text/plain', 'cache-control': 'no-store' },
          });
        }
      "#,
    )
    .await;

    let (status, body) = dispatch(&runtime, "GET", "/api/whatever", None)
        .await
        .expect("dispatch");
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, "app payload");
    runtime.shutdown().await;
}

#[tokio::test]
async fn undefined_handler_results_map_to_the_canonical_404() {
    let (runtime, _dir) = spawn_runtime("export async function handleRequest() {}").await;
    let router = create_plec_server(options(runtime.clone()));

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/none")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(body, r#"{"error":"endpoint not found"}"#);
    runtime.shutdown().await;
}

#[tokio::test]
async fn oversized_bodies_fail_closed_before_reaching_the_sidecar() {
    let (runtime, _dir) =
        spawn_runtime("export async function handleRequest() { return new Response('ok'); }").await;

    let error = runtime
        .dispatch(
            Request::builder()
                .method("POST")
                .uri("/api/todos")
                .body(Body::from(vec![
                    b'x';
                    plec_ir::limits::MAX_REQUEST_BODY_BYTES + 1
                ]))
                .unwrap(),
            test_context("POST", "/api/todos"),
        )
        .await
        .expect_err("oversized body must fail");
    assert!(
        error
            .to_string()
            .contains("request body exceeds byte limit"),
        "{error}"
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn a_bundle_without_handle_request_fails_startup_precisely() {
    let dir = tempfile::tempdir().expect("fixture dir");
    std::fs::write(dir.path().join("app.mjs"), "export const wrong = 1;").expect("bundle write");
    let error = NodeApplicationRuntime::spawn(NodeRuntimeOptions::new(
        runtime_script(),
        dir.path().join("app.mjs"),
    ))
    .await
    .expect_err("spawn must fail");
    assert!(
        error
            .to_string()
            .contains("server entry does not export handleRequest(request, context)"),
        "{error}"
    );
}

#[tokio::test]
async fn a_bundle_that_fails_to_import_fails_startup_with_the_reason() {
    let dir = tempfile::tempdir().expect("fixture dir");
    std::fs::write(dir.path().join("app.mjs"), "import 'missing-module';").expect("bundle write");
    let error = NodeApplicationRuntime::spawn(NodeRuntimeOptions::new(
        runtime_script(),
        dir.path().join("app.mjs"),
    ))
    .await
    .expect_err("spawn must fail");
    assert!(
        error.to_string().contains("server bundle failed to import"),
        "{error}"
    );
}

#[tokio::test]
async fn a_sidecar_that_exits_early_reports_the_startup_failure() {
    let dir = tempfile::tempdir().expect("fixture dir");
    let script = dir.path().join("exit.mjs");
    std::fs::write(&script, "process.exit(0);").expect("script write");
    let error = NodeApplicationRuntime::spawn(NodeRuntimeOptions::new(
        &script,
        Path::new("/nonexistent/bundle.mjs"),
    ))
    .await
    .expect_err("spawn must fail");
    assert!(
        error
            .to_string()
            .contains("sidecar exited before reporting readiness"),
        "{error}"
    );
}

#[tokio::test]
async fn a_missing_sidecar_script_reports_the_startup_failure() {
    // Node spawns fine and exits with MODULE_NOT_FOUND; the supervisor
    // reports the readiness failure with the child's own diagnostics.
    let error = NodeApplicationRuntime::spawn(NodeRuntimeOptions::new(
        "/nonexistent/plec-runtime.mjs",
        "/nonexistent/bundle.mjs",
    ))
    .await
    .expect_err("spawn must fail");
    assert!(
        error
            .to_string()
            .contains("sidecar exited before reporting readiness"),
        "{error}"
    );
}

#[tokio::test]
async fn a_missing_node_binary_fails_spawn() {
    let error = NodeApplicationRuntime::spawn(NodeRuntimeOptions {
        node: "/nonexistent/node-binary".into(),
        script: "/nonexistent/plec-runtime.mjs".into(),
        bundle: "/nonexistent/bundle.mjs".into(),
    })
    .await
    .expect_err("spawn must fail");
    assert!(
        error
            .to_string()
            .contains("cannot spawn node application runtime"),
        "{error}"
    );
}

#[tokio::test]
async fn a_chatty_bundle_still_reaches_readiness() {
    // Application modules log freely during import; the supervisor must find
    // the structured READY line in the noise and keep draining afterwards.
    let (runtime, _dir) = spawn_runtime(
        r#"
        for (let index = 0; index < 500; index += 1) {
          console.log(`connecting to postgres... ${index}`);
        }
        export async function handleRequest() {
          return new Response('ready after noise');
        }
      "#,
    )
    .await;

    let (status, body) = dispatch(&runtime, "GET", "/api/x", None)
        .await
        .expect("dispatch");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ready after noise");
    runtime.shutdown().await;
}

#[tokio::test]
async fn a_sidecar_that_dies_mid_request_fails_the_dispatch() {
    let (runtime, _dir) = spawn_runtime(
        r#"
        export async function handleRequest() {
          await new Promise((resolve) => setTimeout(resolve, 30_000));
          return new Response('never');
        }
      "#,
    )
    .await;

    let dispatching = tokio::spawn({
        let runtime = runtime.clone();
        async move {
            runtime
                .dispatch(
                    Request::builder()
                        .method("GET")
                        .uri("/api/slow")
                        .body(Body::empty())
                        .unwrap(),
                    test_context("GET", "/api/slow"),
                )
                .await
        }
    });
    // Let the request reach the sidecar, then kill it mid-flight.
    tokio::time::sleep(Duration::from_millis(300)).await;
    runtime.shutdown().await;

    let result = tokio::time::timeout(Duration::from_secs(5), dispatching)
        .await
        .expect("dispatch must not hang")
        .expect("task join");
    let error = result.expect_err("dead sidecar must fail the dispatch");
    assert!(
        error
            .to_string()
            .contains("application runtime unavailable"),
        "{error}"
    );
}

#[tokio::test]
async fn shutdown_cleans_up_and_dispatch_fails_fast_afterwards() {
    let (runtime, _dir) = spawn_runtime("export async function handleRequest() {}").await;
    runtime.shutdown().await;

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        runtime
            .dispatch(
                Request::builder()
                    .method("GET")
                    .uri("/api/x")
                    .body(Body::empty())
                    .unwrap(),
                test_context("GET", "/api/x"),
            )
            .await
    })
    .await
    .expect("dispatch must not hang");
    assert!(result.is_err(), "dispatch after shutdown must fail");
}

fn options(runtime: NodeApplicationRuntime) -> PlecServerOptions {
    PlecServerOptions {
        public_dir: PathBuf::from("nonexistent-public"),
        artifact_path: PathBuf::from("nonexistent-artifact.json"),
        client_script: None,
        styles_href: None,
        preloads: Vec::new(),
        document: DocumentMetadata::default(),
        application_runtime: Some(std::sync::Arc::new(runtime)),
        development: false,
    }
}
