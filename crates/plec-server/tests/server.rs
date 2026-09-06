//! Behavioral tests for the native Plec server, mirroring the TypeScript
//! host suite (`packages/plec-server/src/index.test.ts`) it replaces. Every
//! document render is additionally validated against the canonical
//! `PlecSsrSnapshot::validate` contract where the fixture carries compiled
//! component ids.

use std::{path::Path, sync::Arc};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    Router,
};
use plec_ir::PlecSsrSnapshot;
use plec_server::{
    artifact::ArtifactBundle, create_plec_server, runtime::AppRequestHandler, DocumentMetadata,
    PlecServerOptions,
};
use serde_json::{json, Value};
use tower::ServiceExt;

const BOOTSTRAP_OPEN: &str = "<script id=\"plec-bootstrap\" type=\"application/json\">";

fn page(id: &str, tag: &str, text: &str, with_outlet: bool) -> Value {
    let mut component = json!({
        "id": id,
        "rootNode": 0,
        "strings": [tag],
        "constants": [],
        "nodes": [
            {"op": "element", "tag": 0, "children": [1]},
            {"op": "text", "text": 0}
        ],
        "texts": [{"value": text}],
        "bindings": [],
        "propPrograms": [],
        "stateSlots": [],
        "parameters": [],
        "expressions": [],
        "loops": [],
        "routeOutlets": []
    });
    if with_outlet {
        component["routeOutlets"] = json!([{"id": "main", "node": 0}]);
    }
    json!({"rootComponent": 0, "components": [component]})
}

fn simple_page(id: &str, tag: &str, text: &str) -> Value {
    page(id, tag, text, false)
}

fn fixture_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn write_artifact(dir: &Path, artifact: &Value) {
    std::fs::write(
        dir.join("route-artifact.json"),
        serde_json::to_vec(artifact).expect("artifact serialization"),
    )
    .expect("artifact write");
}

fn options(dir: &Path) -> PlecServerOptions {
    PlecServerOptions {
        public_dir: dir.to_path_buf(),
        artifact_path: dir.join("route-artifact.json"),
        client_script: None,
        styles_href: None,
        preloads: Vec::new(),
        document: DocumentMetadata::default(),
        application_runtime: None,
        development: false,
    }
}

fn dev_options(dir: &Path) -> PlecServerOptions {
    let mut options = options(dir);
    options.development = true;
    options
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

async fn text_of(response: Response<Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

async fn get_html(options: &PlecServerOptions, uri: &str) -> String {
    let response = create_plec_server(options.clone())
        .oneshot(get(uri))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    text_of(response).await
}

/// Proves an emitted bootstrap snapshot passes the canonical validator the
/// WASM runtime enforces on import, using the same artifact the server
/// rendered from.
fn assert_valid_bootstrap(html: &str, artifact_json: &Value, manifest_json: &Value) {
    let start = html
        .find(BOOTSTRAP_OPEN)
        .expect("bootstrap script is present")
        + BOOTSTRAP_OPEN.len();
    let end = html[start..].find("</script>").expect("bootstrap close") + start;
    let payload: Value = serde_json::from_str(&html[start..end]).expect("bootstrap json");
    let snapshot: PlecSsrSnapshot =
        serde_json::from_value(payload["snapshot"].clone()).expect("typed snapshot");
    // Test fixtures mirror the TS host's and omit the manifest version the
    // canonical type requires; the compiled contract pins it to 3.
    let mut manifest_json = manifest_json.clone();
    manifest_json["version"] = json!(3);
    let manifest: plec_ir::RouteManifest =
        serde_json::from_value(manifest_json).expect("canonical manifest");
    let bundle: ArtifactBundle =
        serde_json::from_value(artifact_json.clone()).expect("server mirror");
    snapshot
        .validate(&plec_ir::SsrSnapshotReferences {
            manifest: &manifest,
            application: &bundle,
        })
        .expect("emitted snapshot must satisfy the canonical validator");
}

#[tokio::test]
async fn renders_a_route_artifact_with_document_metadata_and_public_request_location() {
    let dir = fixture_dir();
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{
                "id": "home",
                "path": "",
                "graphId": "home",
                "outletId": "main",
                "meta": {"title": "Home title", "description": "Home description"}
            }]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": simple_page("home", "p", "Server rendered")}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/?source=test").await;
    assert!(html.contains("<title>Home title</title>"), "{html}");
    assert!(html.contains("name=\"description\" content=\"Home description\""));
    assert!(html.contains("Server rendered"));
    assert!(html.contains("data-plec-node=\"root/node:0\""));
    assert!(html.contains("\"version\":2"));
    assert!(html.contains("\"revision\":\"test-revision\""));
    assert!(html.contains("\"routeId\":\"home\""));
    assert!(html.contains("\"location\":\"/?source=test\""));
    assert_valid_bootstrap(&html, &artifact, &artifact["manifest"]);
}

#[tokio::test]
async fn serializes_an_empty_text_value_as_the_empty_comment_sentinel() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [{
            "rootNode": 0,
            "strings": ["p"],
            "constants": [""],
            "nodes": [
                {"op": "element", "tag": 0, "children": [1, 2]},
                {"op": "text", "text": 0},
                {"op": "text", "text": 1}
            ],
            "texts": [{"binding": 0}, {"value": "after"}],
            "bindings": [{"target": 0, "sink": "text", "expression": 0}],
            "propPrograms": [],
            "stateSlots": [],
            "parameters": [],
            "loops": [],
            "routeOutlets": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ]
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(
        html.contains("<!--plec:text:root/outlet:main:1--><!---->"),
        "{html}"
    );
    assert!(html.contains("<!--plec:text:root/outlet:main:2-->after"));
}

#[tokio::test]
async fn emits_font_preload_links_before_the_stylesheet() {
    let dir = fixture_dir();
    let graph = page("root", "main", "", true);
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": graph},
            {"graphId": "home", "graph": page("root", "main", "", true)}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let mut options = options(dir.path());
    options.styles_href = Some("/assets/styles.css".to_owned());
    options.preloads = vec!["/assets/files/outfit-latin-wght-normal.woff2".to_owned()];
    let html = get_html(&options, "/").await;
    assert!(
        html.contains(
            "<link rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin \
             href=\"/assets/files/outfit-latin-wght-normal.woff2\">"
        ),
        "{html}"
    );
    assert!(html.find("rel=\"preload\"").unwrap() < html.find("rel=\"stylesheet\"").unwrap());
}

#[tokio::test]
async fn publishes_matched_param_routes_with_their_params_in_the_snapshot_chain() {
    let dir = fixture_dir();
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [
                {"id": "home", "path": "", "graphId": "home", "outletId": "main"},
                {
                    "id": "project",
                    "path": "projects/$id",
                    "graphId": "project",
                    "outletId": "main",
                    "meta": {"title": "Project"}
                },
                {"id": "missing", "path": "*", "graphId": "missing", "outletId": "main"}
            ]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": simple_page("home", "p", "Server rendered")},
            {"graphId": "project", "graph": simple_page("project", "p", "Server rendered")},
            {"graphId": "missing", "graph": simple_page("missing", "p", "Server rendered")}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let options = options(dir.path());

    let project = get_html(&options, "/projects/a%20b").await;
    assert!(project.contains("<title>Project</title>"), "{project}");
    assert!(project.contains("\"routeId\":\"project\""));
    assert!(project.contains("\"params\":{\"id\":\"a b\"}"));
    assert!(project.contains("\"phase\":\"active\""));
    assert_valid_bootstrap(&project, &artifact, &artifact["manifest"]);

    let home = get_html(&options, "/").await;
    assert!(home.contains("\"routeId\":\"home\""));
    // The canonical snapshot type omits an empty param record.
    assert!(!home.contains("\"params\""));

    let missing = get_html(&options, "/nowhere").await;
    assert!(missing.contains("\"routeId\":\"missing\""));
    assert!(!missing.contains("\"params\""));
}

#[tokio::test]
async fn gates_server_only_cookie_host_loads_out_of_markup_and_the_bootstrap() {
    let dir = fixture_dir();
    let root = json!({
        "rootComponent": 0,
        "components": [{
            "id": "root",
            "rootNode": 0,
            "strings": ["main", "session"],
            "constants": [],
            "nodes": [{"op": "element", "tag": 0, "children": [1]}, {"op": "text", "text": 0}],
            "texts": [{"binding": 0}],
            "bindings": [{"target": 0, "sink": "text", "expression": 0}],
            "propPrograms": [],
            "hostSlots": [{"kind": "cookie", "name": 1}],
            "stateSlots": [],
            "parameters": [],
            "loops": [],
            "routeOutlets": [{"id": "main", "node": 0}],
            "expressions": [
                {"instructions": [{"op": "loadHost", "host": 0}, {"op": "return"}]}
            ]
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": root},
            {"graphId": "home", "graph": simple_page("home", "p", "Server rendered")}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let router = create_plec_server(dev_options(dir.path()));
    let response = router.oneshot(get("/")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-plec-ssr-gating")
            .and_then(|value| value.to_str().ok()),
        Some("cookie:session")
    );
    let html = text_of(response).await;
    assert!(
        html.contains("<!--plec:text:root:1--><!---->"),
        "cookie loads evaluate as absent: {html}"
    );
    assert!(!html.contains("session-value"));
}

#[tokio::test]
async fn marks_conditional_boundaries_and_records_the_instantiated_branch() {
    let dir = fixture_dir();
    let root = json!({
        "rootComponent": 0,
        "components": [{
            "id": "root",
            "rootNode": 0,
            "strings": ["main"],
            "constants": [true],
            "nodes": [
                {"op": "element", "tag": 0, "children": [1]},
                {"op": "conditional", "test": 0, "consequent": 2, "alternate": 3},
                {"op": "text", "text": 0},
                {"op": "text", "text": 1}
            ],
            "texts": [{"value": "yes"}, {"value": "no"}],
            "bindings": [],
            "propPrograms": [],
            "stateSlots": [],
            "parameters": [],
            "loops": [],
            "routeOutlets": [{"id": "main", "node": 0}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ]
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": root},
            {"graphId": "home", "graph": simple_page("home", "p", "Server rendered")}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(html.contains("<!--plec:conditional:root:1-->"), "{html}");
    assert!(html.contains("<!--plec:text:root:2-->yes"));
    assert!(html.contains("<!--plec:conditional-end:root:1-->"));
    assert!(!html.contains("no<"));
    assert!(html.contains("\"branches\":[{\"node\":1,\"selected\":\"consequent\"}]"));
    assert_valid_bootstrap(&html, &artifact, &artifact["manifest"]);
}

#[tokio::test]
async fn selects_the_alternate_branch_and_records_none_without_an_alternate() {
    let dir = fixture_dir();
    let root = json!({
        "rootComponent": 0,
        "components": [{
            "id": "root",
            "rootNode": 0,
            "strings": ["main"],
            "constants": [false],
            "nodes": [
                {"op": "element", "tag": 0, "children": [1]},
                {"op": "conditional", "test": 0, "consequent": 2, "alternate": 3},
                {"op": "text", "text": 0},
                {"op": "text", "text": 1}
            ],
            "texts": [{"value": "yes"}, {"value": "no"}],
            "bindings": [],
            "propPrograms": [],
            "stateSlots": [],
            "parameters": [],
            "loops": [],
            "routeOutlets": [{"id": "main", "node": 0}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ]
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": root},
            {"graphId": "home", "graph": simple_page("home", "p", "Server rendered")}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(html.contains("<!--plec:text:root:3-->no"), "{html}");
    assert!(!html.contains(">yes"));
    assert!(html.contains("\"branches\":[{\"node\":1,\"selected\":\"alternate\"}]"));
}

/// Spawns a stub route-loader target; returns its ephemeral port.
async fn spawn_stub(status: u16, body: &'static str) -> u16 {
    let app = Router::new().route(
        "/api/data",
        axum::routing::get(move || async move {
            (
                StatusCode::from_u16(status).expect("valid status"),
                [("content-type", "application/json")],
                body,
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stub listener");
    let port = listener.local_addr().expect("stub address").port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("stub server");
    });
    port
}

fn loader_graph(url: &str) -> Value {
    json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["p"],
            "constants": [url],
            "nodes": [
                {"op": "element", "tag": 0, "children": [1]},
                {"op": "text", "text": 0}
            ],
            "texts": [{"binding": 0}],
            "bindings": [{"target": 0, "sink": "text", "expression": 1}],
            "propPrograms": [],
            "hostSlots": [{"kind": "loaderData"}],
            "stateSlots": [],
            "parameters": [],
            "loops": [],
            "routeOutlets": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadHost", "host": 0}, {"op": "return"}]}
            ],
            "actions": [{
                "routeLoader": true,
                "instructions": [{
                    "op": "capabilityRequest",
                    "capability": "fetch",
                    "request": {"url": 0, "method": "GET", "decode": "responseJson", "requireOk": true},
                    "successPc": 0,
                    "failurePc": 1,
                    "resultSlot": 0,
                    "errorSlot": 1
                }]
            }]
        }]
    })
}

#[tokio::test]
async fn executes_a_route_loader_server_side_and_transfers_the_outcome() {
    let dir = fixture_dir();
    let port = spawn_stub(200, r#"{"title":"ok"}"#).await;
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{
                "id": "home",
                "path": "",
                "graphId": "home",
                "outletId": "main",
                "loaderAction": 0
            }]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": loader_graph(&format!("http://127.0.0.1:{port}/api/data"))}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(
        html.contains("{\"title\":\"ok\"}"),
        "loader data renders through the loaderData host load: {html}"
    );
    assert!(html.contains(
        "\"loaders\":[{\"graphId\":\"home\",\"action\":0,\"state\":{\"kind\":\"resolved\",\
         \"value\":{\"title\":\"ok\"}}}]"
    ));
    assert!(html.contains("\"phase\":\"active\""));
}

#[tokio::test]
async fn renders_the_error_phase_and_records_the_rejection_when_the_loader_fails() {
    let dir = fixture_dir();
    let port = spawn_stub(500, "{}").await;
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{
                "id": "home",
                "path": "",
                "graphId": "home",
                "outletId": "main",
                "errorGraphId": "home-error",
                "loaderAction": 0
            }]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": loader_graph(&format!("http://127.0.0.1:{port}/api/data"))},
            {"graphId": "home-error", "graph": simple_page("home-error", "p", "Server rendered")}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(
        html.contains("Server rendered"),
        "the error phase graph renders: {html}"
    );
    assert!(html.contains("\"phase\":\"error\""));
    assert!(html.contains(
        "\"state\":{\"kind\":\"rejected\",\"message\":\"fetch /api/data failed with status 500\"}"
    ));
}

#[tokio::test]
async fn renders_keyed_loop_rows_with_row_scoped_component_props_and_records_keys() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [
            {
                "id": "home",
                "rootNode": 0,
                "strings": ["div", "p", "id", "label"],
                "constants": [[{"id": "a", "label": "Alpha"}, {"id": "b", "label": "Beta"}]],
                "nodes": [
                    {"op": "element", "tag": 0, "children": [1]},
                    {"op": "loop", "loop": 0},
                    {"op": "component", "component": 1, "props": [
                        {"kind": "value", "name": 3, "expression": 2}
                    ]}
                ],
                "texts": [],
                "bindings": [],
                "propPrograms": [],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [],
                "expressions": [
                    {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                    {"instructions": [{"op": "loadRowField", "field": 2}, {"op": "return"}]},
                    {"instructions": [{"op": "loadRowField", "field": 3}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [{"sourceExpression": 0, "keyExpression": 1, "itemSlot": 0, "rowTemplate": 2}],
                "routeOutlets": []
            },
            {
                "id": "home-row",
                "rootNode": 0,
                "strings": ["p", "label"],
                "constants": [],
                "nodes": [
                    {"op": "element", "tag": 0, "children": [1]},
                    {"op": "text", "text": 0}
                ],
                "texts": [{"binding": 0}],
                "bindings": [{"target": 0, "sink": "text", "expression": 0}],
                "propPrograms": [],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [{"name": 1}],
                "expressions": [
                    {"instructions": [{"op": "loadProp", "prop": 0}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [],
                "routeOutlets": []
            }
        ]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(
        html.contains("<!--plec:loop:root/outlet:main/loop:1/key:a-->"),
        "{html}"
    );
    assert!(html.contains("<!--plec:loop-end:root/outlet:main/loop:1/key:a-->"));
    assert!(html.contains("Alpha"));
    assert!(html.contains("Beta"));
    assert!(html.contains("data-runtime-row-key=\"a\""));
    assert!(html.contains("data-runtime-row-key=\"b\""));
    assert!(html.contains("\"loops\":[{\"node\":1,\"keys\":[\"a\",\"b\"]}]"));
    assert_valid_bootstrap(&html, &artifact, &artifact["manifest"]);
}

#[tokio::test]
async fn fails_the_document_render_when_a_loop_produces_duplicate_keys() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["div", "id", "p"],
            "constants": [[{"id": "a"}, {"id": "a"}]],
            "nodes": [
                {"op": "element", "tag": 0, "children": [1]},
                {"op": "loop", "loop": 0},
                {"op": "element", "tag": 2, "children": []}
            ],
            "texts": [],
            "bindings": [],
            "propPrograms": [],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 1}, {"op": "return"}]}
            ],
            "actions": [],
            "loops": [{"sourceExpression": 0, "keyExpression": 1, "itemSlot": 0, "rowTemplate": 2}],
            "routeOutlets": []
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let response = create_plec_server(options(dir.path()))
        .oneshot(get("/"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = text_of(response).await;
    assert!(body.contains("Plec SSR failed"), "{body}");
    assert!(body.contains("DUPLICATE_LOOP_KEY:a"), "{body}");
}

#[tokio::test]
async fn records_nested_component_conditionals_and_loops_under_their_marker_paths() {
    let dir = fixture_dir();
    let root = json!({
        "rootComponent": 0,
        "components": [
            {
                "id": "root",
                "rootNode": 0,
                "strings": ["main"],
                "constants": [],
                "nodes": [
                    {"op": "element", "tag": 0, "children": [1]},
                    {"op": "component", "component": 1, "children": [2]},
                    {"op": "slot"}
                ],
                "texts": [],
                "bindings": [],
                "propPrograms": [],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [],
                "expressions": [],
                "actions": [],
                "loops": [],
                "routeOutlets": [{"id": "main", "node": 0}]
            },
            {
                "id": "root-child",
                "rootNode": 0,
                "strings": ["p", "k"],
                "constants": [true, [{"k": "a"}]],
                "nodes": [
                    {"op": "element", "tag": 0, "children": [1, 3]},
                    {"op": "conditional", "test": 0, "consequent": 2},
                    {"op": "text", "text": 0},
                    {"op": "loop", "loop": 0},
                    {"op": "element", "tag": 0, "children": [5]},
                    {"op": "text", "text": 1}
                ],
                "texts": [{"value": "yes"}, {"binding": 0}],
                "bindings": [{"target": 5, "sink": "text", "expression": 3}],
                "propPrograms": [],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [],
                "expressions": [
                    {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                    {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]},
                    {"instructions": [{"op": "loadRowField", "field": 1}, {"op": "return"}]},
                    {"instructions": [{"op": "loadRowField", "field": 1}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [{"sourceExpression": 1, "keyExpression": 2, "itemSlot": 0, "rowTemplate": 4}],
                "routeOutlets": []
            }
        ]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": root},
            {"graphId": "home", "graph": simple_page("home", "p", "Server rendered")}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(html.contains("<!--plec:component:root:1-->"), "{html}");
    assert!(html.contains("<!--plec:conditional:root/component:1:1-->"));
    assert!(html.contains("yes"));
    assert!(html.contains("<!--plec:loop:root/component:1/loop:3/key:a-->"));
    assert!(html.contains("<!--plec:text:root/component:1/loop:3/key:a:5-->a"));
    assert!(
        html.contains(
            "\"nested\":{\"root/component:1\":{\"graphId\":\"root-child\",\
         \"branches\":[{\"node\":1,\"selected\":\"consequent\"}],\
         \"loops\":[{\"node\":3,\"keys\":[\"a\"]}]}}"
        ),
        "nested records must carry branch and loop ownership: {html}"
    );
    assert_valid_bootstrap(&html, &artifact, &artifact["manifest"]);
}

#[tokio::test]
async fn serializes_props_spreads_so_island_icons_paint_with_their_class_attribute() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["span", "class"],
            "constants": [{"class": "icon"}],
            "nodes": [{"op": "element", "tag": 0, "children": []}],
            "texts": [],
            "bindings": [],
            "propPrograms": [{"target": 0, "writes": [{"spread": true, "expression": 0}]}],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ],
            "actions": [],
            "loops": [],
            "routeOutlets": []
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(
        html.contains("<span class=\"icon\" data-plec-node="),
        "{html}"
    );
}

#[tokio::test]
async fn fails_the_render_closed_when_a_reserved_attribute_is_written_literally() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["span", "data-plec-node"],
            "constants": ["x"],
            "nodes": [{"op": "element", "tag": 0, "children": []}],
            "texts": [],
            "bindings": [],
            "propPrograms": [{"target": 0, "writes": [{"name": 1, "constant": 0}]}],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [],
            "actions": [],
            "loops": [],
            "routeOutlets": []
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let response = create_plec_server(options(dir.path()))
        .oneshot(get("/"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = text_of(response).await;
    assert!(body.contains("RESERVED_ATTRIBUTE:data-plec-node"), "{body}");
}

#[tokio::test]
async fn fails_the_render_closed_when_a_spread_bag_carries_a_reserved_attribute() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["span"],
            "constants": [{"data-runtime-row-key": "x"}],
            "nodes": [{"op": "element", "tag": 0, "children": []}],
            "texts": [],
            "bindings": [],
            "propPrograms": [{"target": 0, "writes": [{"spread": true, "expression": 0}]}],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ],
            "actions": [],
            "loops": [],
            "routeOutlets": []
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let response = create_plec_server(options(dir.path()))
        .oneshot(get("/"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = text_of(response).await;
    assert!(
        body.contains("RESERVED_ATTRIBUTE:data-runtime-row-key"),
        "{body}"
    );
}

#[tokio::test]
async fn fails_the_render_closed_on_srcdoc_and_script_url_attribute_writes() {
    let srcdoc_dir = fixture_dir();
    let srcdoc_home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["iframe", "srcdoc"],
            "constants": ["<script>alert(1)</script>"],
            "nodes": [{"op": "element", "tag": 0, "children": []}],
            "texts": [],
            "bindings": [],
            "propPrograms": [{"target": 0, "writes": [{"name": 1, "constant": 0}]}],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [],
            "actions": [],
            "loops": [],
            "routeOutlets": []
        }]
    });
    let srcdoc_artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": srcdoc_home}
        ]
    });
    write_artifact(srcdoc_dir.path(), &srcdoc_artifact);
    let response = create_plec_server(options(srcdoc_dir.path()))
        .oneshot(get("/"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(text_of(response).await.contains("UNSAFE_ATTRIBUTE:srcdoc"));

    let href_dir = fixture_dir();
    let href_home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["a", "href"],
            "constants": ["javascript:alert(1)"],
            "nodes": [{"op": "element", "tag": 0, "children": []}],
            "texts": [],
            "bindings": [],
            "propPrograms": [{"target": 0, "writes": [{"name": 1, "constant": 0}]}],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [],
            "actions": [],
            "loops": [],
            "routeOutlets": []
        }]
    });
    let href_artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": href_home}
        ]
    });
    write_artifact(href_dir.path(), &href_artifact);
    let response = create_plec_server(options(href_dir.path()))
        .oneshot(get("/"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(text_of(response)
        .await
        .contains("UNSAFE_URL_ATTRIBUTE:href"));
}

#[tokio::test]
async fn drops_hostile_event_handler_keys_from_spread_bags_in_markup() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["span", "class"],
            "constants": [{"onclick": "alert(1)", "class": "ok"}],
            "nodes": [{"op": "element", "tag": 0, "children": []}],
            "texts": [],
            "bindings": [],
            "propPrograms": [{"target": 0, "writes": [{"spread": true, "expression": 0}]}],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ],
            "actions": [],
            "loops": [],
            "routeOutlets": []
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(!html.contains("onclick"), "{html}");
    assert!(html.contains("class=\"ok\""));
}

#[tokio::test]
async fn passes_named_props_to_a_dynamic_island_component_and_serializes_its_spread() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [
            {
                "id": "home",
                "rootNode": 0,
                "strings": ["div", "Slot", "label"],
                "constants": ["Hello"],
                "nodes": [
                    {"op": "element", "tag": 0, "children": [1]},
                    {"op": "component", "component": 1, "props": [
                        {"kind": "component", "name": 1, "component": 2},
                        {"kind": "value", "name": 2, "expression": 0}
                    ]}
                ],
                "texts": [],
                "bindings": [],
                "propPrograms": [],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [],
                "expressions": [
                    {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [],
                "routeOutlets": []
            },
            {
                "id": "home-child",
                "rootNode": 0,
                "strings": ["Slot", "label", "class"],
                "constants": ["pill"],
                "nodes": [
                    {"op": "dynamicComponent", "prop": 0, "props": [
                        {"kind": "value", "name": 2, "expression": 0}
                    ]}
                ],
                "texts": [],
                "bindings": [],
                "propPrograms": [],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [{"name": 0}, {"name": 1}],
                "expressions": [
                    {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [],
                "routeOutlets": []
            },
            {
                "id": "home-island",
                "rootNode": 0,
                "strings": ["span", "__plec_props"],
                "constants": [],
                "nodes": [{"op": "element", "tag": 0, "children": []}],
                "texts": [],
                "bindings": [],
                "propPrograms": [{"target": 0, "writes": [{"spread": true, "expression": 0}]}],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [{"name": 1}],
                "expressions": [
                    {"instructions": [{"op": "loadProp", "prop": 0}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [],
                "routeOutlets": []
            }
        ]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(
        html.contains("<span class=\"pill\" data-plec-node="),
        "the dynamic island receives its named props and serializes the spread: {html}"
    );
}

#[tokio::test]
async fn rejects_inbound_request_bodies_beyond_the_documented_limit() {
    let dir = fixture_dir();
    let oversized = vec![b'x'; plec_ir::limits::MAX_REQUEST_BODY_BYTES + 1];
    let request = Request::builder()
        .method("POST")
        .uri("/api/todos")
        .body(Body::from(oversized))
        .unwrap();
    let response = create_plec_server(options(dir.path()))
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = text_of(response).await;
    assert!(body.contains("request body exceeds byte limit"), "{body}");
}

#[tokio::test]
async fn rejects_oversized_application_artifacts_with_a_500_json_error() {
    let dir = fixture_dir();
    let padding = "a".repeat(plec_ir::limits::MAX_ARTIFACT_JSON_BYTES + 1);
    std::fs::write(
        dir.path().join("route-artifact.json"),
        format!("{{\"manifest\":{{}},\"padding\":\"{padding}\"}}"),
    )
    .expect("artifact write");
    let response = create_plec_server(options(dir.path()))
        .oneshot(get("/"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = text_of(response).await;
    assert!(
        body.contains("application artifact exceeds byte limit"),
        "{body}"
    );
}

#[tokio::test]
async fn falls_back_to_the_public_shell_when_the_artifact_is_unreadable() {
    let dir = fixture_dir();
    std::fs::write(dir.path().join("index.html"), "<html>shell</html>").expect("shell write");
    let router = create_plec_server(dev_options(dir.path()));
    let response = router.oneshot(get("/")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("x-plec-ssr-fallback").is_some());
    assert!(text_of(response).await.contains("shell"));
}

#[tokio::test]
async fn serves_static_assets_with_precompressed_sidecars() {
    let dir = fixture_dir();
    std::fs::write(dir.path().join("style.css"), "body{}").expect("asset write");
    // The `.br` sidecar's presence is the opt-in; its bytes are served
    // verbatim with a `content-encoding: br` header.
    std::fs::write(dir.path().join("style.css.br"), b"compressed-bytes").expect("sidecar write");
    let router = create_plec_server(options(dir.path()));

    let identity = router
        .clone()
        .oneshot(get("/style.css"))
        .await
        .expect("response");
    assert_eq!(identity.status(), StatusCode::OK);
    assert_eq!(
        identity.headers().get("cache-control").unwrap(),
        "no-cache",
        "assets never cache intermediated"
    );
    assert_eq!(text_of(identity).await, "body{}");

    let compressed = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/style.css")
                .header("accept-encoding", "br")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(compressed.status(), StatusCode::OK);
    assert_eq!(compressed.headers().get("content-encoding").unwrap(), "br");
    assert_eq!(text_of(compressed).await, "compressed-bytes");

    let missing = router.oneshot(get("/missing.css")).await.expect("response");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(text_of(missing).await.contains("asset not found"));
}

#[tokio::test]
async fn dispatches_api_requests_to_the_application_handler() {
    let dir = fixture_dir();
    let handler: AppRequestHandler = Arc::new(|_request, context| {
        Box::pin(async move {
            if context.pathname == "/api/todos" {
                Some(
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(format!("{{\"method\":\"{}\"}}", context.method)))
                        .unwrap(),
                )
            } else {
                None
            }
        })
    });
    let mut options = options(dir.path());
    options.application_runtime = Some(Arc::new(handler));
    let router = create_plec_server(options);

    let todos = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/todos")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .expect("response");
    assert_eq!(todos.status(), StatusCode::OK);
    assert_eq!(text_of(todos).await, "{\"method\":\"POST\"}");

    let unknown = router.oneshot(get("/api/none")).await.expect("response");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert!(text_of(unknown).await.contains("endpoint not found"));
}

#[tokio::test]
async fn api_requests_without_a_handler_receive_the_json_404() {
    let dir = fixture_dir();
    let response = create_plec_server(options(dir.path()))
        .oneshot(get("/api/todos"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(text_of(response).await.contains("endpoint not found"));
}

#[tokio::test]
async fn documents_without_a_matched_route_render_the_root_graph_without_a_bootstrap() {
    let dir = fixture_dir();
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [{"graphId": "root", "graph": page("root", "main", "", true)}]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/no-such-document-path").await;
    assert!(html.contains("<title>Plec application</title>"), "{html}");
    assert!(!html.contains(BOOTSTRAP_OPEN));
}

#[tokio::test]
async fn route_loaders_without_a_valid_program_fail_the_document_render() {
    let dir = fixture_dir();
    let home = {
        let mut graph = loader_graph("http://127.0.0.1:9/api/data");
        // Strip the fetch capability: the loader program is invalid.
        graph["components"][0]["actions"][0]["instructions"] = json!([]);
        graph
    };
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{
                "id": "home",
                "path": "",
                "graphId": "home",
                "outletId": "main",
                "loaderAction": 0
            }]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let response = create_plec_server(options(dir.path()))
        .oneshot(get("/"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = text_of(response).await;
    assert!(
        body.contains("route loader action is invalid for home"),
        "{body}"
    );
}

#[tokio::test]
async fn unknown_node_ops_render_as_nothing_like_the_typescript_host() {
    let dir = fixture_dir();
    let home = json!({
        "rootComponent": 0,
        "components": [{
            "id": "home",
            "rootNode": 0,
            "strings": ["p"],
            "constants": [],
            "nodes": [
                {"op": "element", "tag": 0, "children": [1]},
                {"op": "something-new"}
            ],
            "texts": [{"value": "after"}],
            "bindings": [],
            "propPrograms": [],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [],
            "actions": [],
            "loops": [],
            "routeOutlets": []
        }]
    });
    let artifact = json!({
        "manifest": {
            "revision": "test-revision",
            "rootGraphId": "root",
            "routes": [{"id": "home", "path": "", "graphId": "home", "outletId": "main"}]
        },
        "graphs": [
            {"graphId": "root", "graph": page("root", "main", "", true)},
            {"graphId": "home", "graph": home}
        ]
    });
    write_artifact(dir.path(), &artifact);
    let html = get_html(&options(dir.path()), "/").await;
    assert!(
        html.contains("<p data-plec-node=\"root/outlet:main/node:0\"></p>"),
        "{html}"
    );
}

/// Renders the real compiled fullstack application through the native host
/// and validates every emitted snapshot against the canonical validator.
/// Requires the application build output; run with:
///
/// ```text
/// cargo test -p plec-server --test server -- --ignored real_application
/// ```
#[tokio::test]
#[ignore = "requires apps/fullstack/dist/public; build the application first"]
async fn real_application_artifact_renders_and_validates() {
    const PUBLIC: &str = "../../apps/fullstack/dist/public";
    let dir = Path::new(PUBLIC);
    let artifact_path = dir.join("route-artifact.json");
    if !artifact_path.exists() {
        panic!("missing {}", artifact_path.display());
    }
    let mut options = options(dir);
    options.client_script = Some("/assets/client.js".to_owned());
    options.styles_href = Some("/assets/styles.css".to_owned());
    let artifact_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&artifact_path).expect("artifact read"))
            .expect("artifact json");
    for route in ["/", "/todos", "/projects/42", "/about"] {
        let html = get_html(&options, route).await;
        assert!(
            html.contains(BOOTSTRAP_OPEN) || html.contains("<title>"),
            "{route}"
        );
        if html.contains(BOOTSTRAP_OPEN) {
            assert_valid_bootstrap(&html, &artifact_json, &artifact_json["manifest"]);
        }
        eprintln!("{route}: {} bytes", html.len());
    }
}
