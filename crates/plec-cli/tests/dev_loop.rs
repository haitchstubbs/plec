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

fn install_test_packages(project: &Path) {
    use sha2::{Digest, Sha256};
    let packages = project.join("node_modules/@plec");
    let core = packages.join("core");
    fs::create_dir_all(core.join("dist/runtime")).unwrap();
    fs::create_dir_all(core.join("scripts")).unwrap();
    fs::write(
        core.join("package.json"),
        r#"{"name":"@plec/core","type":"module","dependencies":{"@plec/vite":"^0.1.0"},"exports":{".":"./dist/browser.js","./server-context":"./dist/server-context.js","./vite-build":"./scripts/vite-build.mjs"}}"#,
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/plec/scripts/vite-build.mjs"),
        core.join("scripts/vite-build.mjs"),
    )
    .unwrap();
    fs::write(
        core.join("dist/server-context.js"),
        "export function withRequestContext(_context, run) { return run(); }\n",
    )
    .unwrap();
    fs::write(core.join("dist/browser.js"), "export function createRouter() { return {}; }\nexport function createRootRoute() { return {}; }\n").unwrap();
    let runtime = b"test-runtime";
    let wasm = b"\0asm\x01\0\0\0";
    fs::write(core.join("dist/runtime/runtime.js"), runtime).unwrap();
    fs::write(core.join("dist/runtime/runtime_bg.wasm"), wasm).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    fs::write(
        core.join("dist/runtime/provenance.json"),
        format!(
            r#"{{"jsSha256":"{}","wasmSha256":"{}"}}"#,
            digest(runtime),
            digest(wasm)
        ),
    )
    .unwrap();

    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/plec-vite"),
        &packages.join("vite"),
    );
    let node_modules = project.join("node_modules");
    let vite = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node_modules/vite");
    #[cfg(unix)]
    std::os::unix::fs::symlink(vite, node_modules.join("vite")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(vite, node_modules.join("vite")).unwrap();
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
                    .unwrap()
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
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

fn compiled_asset(html: &str) -> String {
    html.split("src=\"")
        .nth(1)
        .and_then(|value| value.split('"').next())
        .unwrap()
        .to_owned()
}

struct DevChild {
    child: Child,
    port: u16,
    output: Arc<Mutex<String>>,
    readers: Vec<thread::JoinHandle<()>>,
}

impl DevChild {
    fn start(app: &Path, port: u16) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_plec"))
            .current_dir(app)
            .args(["dev", "--port", &port.to_string()])
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
            port,
            output,
            readers,
        }
    }

    fn output(&self) -> String {
        self.output.lock().unwrap().clone()
    }

    fn host_pid(&self) -> u32 {
        let output = self.output();
        let client = output
            .rfind("native host pid ")
            .map(|index| (index, "native host pid ".len()));
        let restarted = output
            .rfind("native host restarted (pid ")
            .map(|index| (index, "native host restarted (pid ".len()));
        let (index, length) = client
            .into_iter()
            .chain(restarted)
            .max_by_key(|(index, _)| *index)
            .expect("native host pid is logged");
        output[index + length..]
            .split_whitespace()
            .next()
            .unwrap()
            .trim_end_matches(')')
            .parse()
            .unwrap()
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
                "plec dev exited: {}",
                self.output()
            );
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}; output: {}",
                self.output()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_output(&mut self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.output().contains(needle) {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "plec dev exited: {}",
                self.output()
            );
            assert!(
                Instant::now() < deadline,
                "missing output {needle:?}: {}",
                self.output()
            );
            thread::sleep(Duration::from_millis(50));
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
        }
        self.child.wait().unwrap();
        // The coordinator/host shutdown closes inherited stdio shortly after
        // exit. Detach capture readers here rather than making cleanup itself
        // unbounded if a failing child retained a descriptor.
        self.readers.clear();
    }
}

impl Drop for DevChild {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn vite_dev_keeps_client_host_live_watches_external_assets_and_restarts_for_server_changes() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("plec-vite-dev-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let app = root.join("app");
    copy_tree(&fixture_source(), &app.join("src"));
    install_test_packages(&root);
    fs::write(
        app.join("src/app.tsx"),
        "export { router } from './router';\n",
    )
    .unwrap();
    fs::write(app.join("shared.svg"), "<svg><text>external A</text></svg>").unwrap();
    fs::write(app.join("src/home.tsx"), "import logo from '../shared.svg'; export function Home() { return <div>Version A<img src={logo} /></div>; }\n").unwrap();
    fs::write(
        app.join("src/server.ts"),
        "export const handleRequest = async () => new Response('API A');\n",
    )
    .unwrap();
    let port = free_port();
    let mut dev = DevChild::start(&app, port);
    dev.wait_for("/", "Version A", Duration::from_secs(90));
    dev.wait_output("native host pid");
    let initial_host = dev.host_pid();

    fs::write(app.join("src/home.tsx"), "import logo from '../shared.svg'; export function Home() { return <div>Version B<img src={logo} /></div>; }\n").unwrap();
    let version_b = dev.wait_for("/", "Version B", Duration::from_secs(60));
    dev.wait_output("client artifacts updated");
    assert_eq!(
        dev.host_pid(),
        initial_host,
        "client-only change keeps native host process"
    );
    let old_asset = compiled_asset(&version_b);

    fs::write(app.join("shared.svg"), "<svg><text>external B</text></svg>").unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let new_asset = loop {
        let html = http(port, "/").unwrap_or_default();
        let url = compiled_asset(&html);
        if url != old_asset && http(port, &url).is_some_and(|asset| asset.contains("external B")) {
            break url;
        }
        assert!(
            Instant::now() < deadline,
            "external source dependency did not invalidate Plec build: {}",
            dev.output()
        );
        thread::sleep(Duration::from_millis(50));
    };
    assert_ne!(new_asset, old_asset);
    assert_eq!(
        dev.host_pid(),
        initial_host,
        "compiled external asset change is client-only"
    );

    fs::write(
        app.join("src/server.ts"),
        "export const handleRequest = async () => new Response('API B');\n",
    )
    .unwrap();
    dev.wait_for("/api/version", "API B", Duration::from_secs(60));
    dev.wait_output("native host restarted");
    let replacement_host = dev.host_pid();
    assert_ne!(
        replacement_host,
        initial_host,
        "server bundle change restarts native host; logs: {}",
        dev.output()
    );
    dev.stop();
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "Vite public port released"
    );
    assert!(dev.child.try_wait().unwrap().is_some());
    let _ = fs::remove_dir_all(root);
}
