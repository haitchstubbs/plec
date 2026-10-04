use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn fixture_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini-repo/apps/mini-app/src")
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn install_test_package(project: &Path) {
    use sha2::{Digest, Sha256};
    let package = project.join("node_modules/@plec/core");
    fs::create_dir_all(package.join("dist/runtime")).unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"name":"@plec/core","type":"module","exports":{".":"./dist/browser.js","./server-context":"./dist/server-context.js"}}"#,
    )
    .unwrap();
    fs::write(
        package.join("dist/server-context.js"),
        "export function withRequestContext(_context, run) { return run(); }\n",
    )
    .unwrap();
    fs::write(
        package.join("dist/browser.js"),
        "export function createRouter() { return {}; }\nexport function createRootRoute() { return {}; }\n",
    )
    .unwrap();
    let runtime = b"test-runtime";
    let wasm = b"\0asm\x01\0\0\0";
    fs::write(package.join("dist/runtime/runtime.js"), runtime).unwrap();
    fs::write(package.join("dist/runtime/runtime_bg.wasm"), wasm).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    fs::write(
        package.join("dist/runtime/provenance.json"),
        format!(
            r#"{{"jsSha256":"{}","wasmSha256":"{}"}}"#,
            digest(runtime),
            digest(wasm)
        ),
    )
    .unwrap();
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn drain<R: Read + Send + 'static>(
    mut pipe: R,
    output: Arc<Mutex<String>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut bytes = [0; 2048];
        loop {
            match pipe.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => output
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .push_str(&String::from_utf8_lossy(&bytes[..count])),
            }
        }
    })
}

fn http(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(300)))
        .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

fn compiled_asset_url(html: &str) -> String {
    html.split("src=\"")
        .nth(1)
        .and_then(|value| value.split('\"').next())
        .expect("SSR document should include the imported source asset")
        .to_owned()
}

struct DevChild {
    child: Child,
    app: PathBuf,
    port: u16,
    output: Arc<Mutex<String>>,
    readers: Vec<thread::JoinHandle<()>>,
}

impl DevChild {
    fn start(app: PathBuf, port: u16) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_plec"))
            .current_dir(&app)
            .args(["dev", "src/router.tsx", "--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("plec dev starts");
        let output = Arc::new(Mutex::new(String::new()));
        let readers = vec![
            drain(child.stdout.take().unwrap(), output.clone()),
            drain(child.stderr.take().unwrap(), output.clone()),
        ];
        Self {
            child,
            app,
            port,
            output,
            readers,
        }
    }

    fn wait_for(&mut self, path: &str, needle: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(response) = http(self.port, path) {
                if response.contains(needle) {
                    return response;
                }
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "plec dev exited early: {}",
                self.output.lock().unwrap()
            );
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}; dev output: {}",
                self.output.lock().unwrap()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn generation(&self) -> u64 {
        let marker = self
            .app
            .join(format!(".plec-dev-state-{}.json", self.child.id()));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(value) =
                fs::read(&marker).map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes))
            {
                if let Ok(value) = value {
                    if let Some(generation) = value["generation"].as_u64() {
                        return generation;
                    }
                }
            }
            assert!(
                Instant::now() < deadline,
                "dev generation marker unavailable"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn output_contains(&self, needle: &str) -> bool {
        self.output.lock().unwrap().contains(needle)
    }

    fn wait_generation(&mut self, expected: u64) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.generation() != expected {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "plec dev exited before generation {expected}"
            );
            assert!(
                Instant::now() < deadline,
                "generation did not reach {expected}"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn stop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            #[cfg(unix)]
            let _ = Command::new("kill")
                .args(["-TERM", &self.child.id().to_string()])
                .status();
            #[cfg(windows)]
            self.child.kill().unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while self.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
        }
        self.child.wait().unwrap();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

impl Drop for DevChild {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn spawned_dev_session_rebuilds_preserves_failures_recovers_restarts_and_cleans_up() {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("plec-dev-it-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let app = root.join("app");
    copy_tree(&fixture_source(), &app.join("src"));
    install_test_package(&root);
    fs::create_dir_all(app.join("public")).unwrap();
    fs::write(app.join("src/home.tsx"), "import logo from './logo.svg'; export function Home() { return <div>Version A<img src={logo} /></div>; }\n").unwrap();
    fs::write(
        app.join("src/server.ts"),
        "export const handleRequest = async () => new Response('API A');\n",
    )
    .unwrap();
    fs::write(app.join("src/logo.svg"), "<svg><text>A</text></svg>").unwrap();
    let port = free_port();
    let mut dev = DevChild::start(app.clone(), port);
    dev.wait_for("/", "Version A", Duration::from_secs(60));
    assert_eq!(dev.generation(), 0);

    fs::write(
        app.join("src/home.tsx"),
        "import logo from './logo.svg'; export function Home() { return <div>Version B<img src={logo} /></div>; }\n",
    )
    .unwrap();
    fs::write(
        app.join("src/home.tsx"),
        "import logo from './logo.svg'; export function Home() { return <div>Version C settled<img src={logo} /></div>; }\n",
    )
    .unwrap();
    dev.wait_for("/", "Version C settled", Duration::from_secs(12));
    assert_eq!(
        dev.generation(),
        1,
        "one settled burst commits one generation"
    );
    let old_asset = compiled_asset_url(&http(port, "/").unwrap());

    fs::write(app.join("src/logo.svg"), "<svg><text>B</text></svg>").unwrap();
    let asset_deadline = Instant::now() + Duration::from_secs(60);
    while dev.generation() != 2 {
        assert!(
            Instant::now() < asset_deadline,
            "compiled source asset did not trigger rebuild"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let asset_html = dev.wait_for("/", "Version C settled", Duration::from_secs(10));
    assert!(asset_html.contains("/assets/compiled/"));
    let new_asset = compiled_asset_url(&asset_html);
    assert_ne!(
        old_asset, new_asset,
        "changed source asset receives a new fingerprint"
    );
    assert!(http(port, &new_asset).unwrap().contains("<text>B</text>"));

    fs::write(app.join("src/home.tsx"), "export function Home( {\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dev.output_contains("build failed") {
        assert!(
            dev.child.try_wait().unwrap().is_none(),
            "dev loop died on failed build"
        );
        assert!(Instant::now() < deadline, "invalid candidate did not fail");
        thread::sleep(Duration::from_millis(50));
    }
    assert!(http(port, "/").is_some_and(|response| response.contains("Version C settled")));
    assert_eq!(
        dev.generation(),
        2,
        "failed candidate must not advance generation"
    );
    fs::write(
        app.join("src/home.tsx"),
        "import logo from './logo.svg'; export function Home() { return <div>Version D<img src={logo} /></div>; }\n",
    )
    .unwrap();
    dev.wait_for("/", "Version D", Duration::from_secs(60));
    assert_eq!(dev.generation(), 3);

    fs::write(
        app.join("src/server.ts"),
        "export const handleRequest = async () => new Response('API B');\n",
    )
    .unwrap();
    dev.wait_for("/api/version", "API B", Duration::from_secs(60));
    dev.wait_generation(4);
    assert_eq!(dev.generation(), 4);
    assert!(dev.child.try_wait().unwrap().is_none());

    fs::write(
        app.join("src/server.ts"),
        "import { missing } from 'plec-no-such-package'; export const handleRequest = async () => new Response(String(missing));\n",
    )
    .unwrap();
    let rollback_deadline = Instant::now() + Duration::from_secs(30);
    while !dev.output_contains("replacement host failed readiness") {
        assert!(
            Instant::now() < rollback_deadline,
            "bad runtime did not exercise rollback"
        );
        assert!(
            dev.child.try_wait().unwrap().is_none(),
            "dev parent exited during rollback"
        );
        thread::sleep(Duration::from_millis(50));
    }
    dev.wait_for("/api/version", "API B", Duration::from_secs(15));
    assert_eq!(
        dev.generation(),
        4,
        "readiness failure must not commit a generation"
    );

    fs::write(
        app.join("src/server.ts"),
        "export const handleRequest = async () => new Response('API C');\n",
    )
    .unwrap();
    dev.wait_for("/api/version", "API C", Duration::from_secs(60));
    dev.wait_generation(5);
    assert_eq!(dev.generation(), 5, "watching continues after rollback");

    let marker = app.join(format!(".plec-dev-state-{}.json", dev.child.id()));
    dev.stop();
    assert!(!marker.exists(), "dev marker is removed at shutdown");
    assert!(!app
        .join(format!("dist.previous-{}", dev.child.id()))
        .exists());
    assert!(!app.join("dist.next").exists());
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "dev listener is released"
    );
    let parent = app.parent().unwrap();
    assert!(!fs::read_dir(parent).unwrap().flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with(&format!(".plec-dev-{}-", dev.child.id()))
    }));
    let _ = fs::remove_dir_all(root);
}
