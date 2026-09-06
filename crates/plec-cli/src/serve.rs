//! The native host launcher shared by both CLI variants. Runtime
//! environment (port, bind address, development mode) belongs here, never in
//! the application; host configuration comes exclusively from the
//! build-emitted manifest.

use std::path::PathBuf;

use plec_server::{manifest::LoadedServerManifest, NodeRuntimeOptions};

pub struct ServeOptions {
    /// Build output directory containing `plec-server.json`.
    pub dir: PathBuf,
    /// Bind address. Defaults to 127.0.0.1.
    pub host: Option<String>,
    /// Port; falls back to `$PORT`, then 3000.
    pub port: Option<u16>,
    /// Development mode (also enabled unless NODE_ENV=production).
    pub development: bool,
}

pub fn serve(options: ServeOptions) -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::absolute(&options.dir)?;
    let loaded = LoadedServerManifest::load(&dir)?;

    let port = options
        .port
        .or_else(|| {
            std::env::var("PORT")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(3000);
    let host = options.host.unwrap_or_else(|| "127.0.0.1".to_owned());
    let development =
        options.development || std::env::var("NODE_ENV").as_deref() != Ok("production");

    // The sidecar has its own single-threaded runtime for startup; the host
    // itself runs on the main runtime below.
    let runtime = match (loaded.server_entry(), loaded.server_runtime()) {
        (Some(entry), Some(script)) => {
            let spawned = std::thread::spawn(move || {
                tokio::runtime::Runtime::new()?.block_on(
                    plec_server::NodeApplicationRuntime::spawn(NodeRuntimeOptions::new(
                        script, entry,
                    )),
                )
            });
            let runtime = Some(spawned.join().map_err(|_| -> Box<dyn std::error::Error> {
                "sidecar startup thread panicked".into()
            })??);
            println!("application runtime: node sidecar attached");
            runtime
        }
        // Without the runtime section, `/api/*` traffic has no executor and
        // 404s. That is correct for asset-only deployments — but an app that
        // authored server code must hear about it at startup, not discover
        // it as a wall of 404s.
        _ => {
            println!(
                "application runtime: disabled (manifest carries no server bundle; /api/* will 404)"
            );
            None
        }
    };

    let router = plec_server::create_plec_server(plec_server::PlecServerOptions {
        application_runtime: runtime.as_ref().map(|runtime| {
            std::sync::Arc::new(runtime.clone())
                as std::sync::Arc<dyn plec_server::ApplicationRuntime>
        }),
        ..loaded.options(development)
    });

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind((host.as_str(), port))
                .await
                .map_err(|error| format!("cannot bind {host}:{port}: {error}"))?;
            println!("Plec host listening on http://{host}:{port}");
            let shutdown = async {
                let _ = tokio::signal::ctrl_c().await;
                println!("shutting down");
            };
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown)
                .await
                .map_err(|error| format!("server failed: {error}"))?;
            if let Some(runtime) = runtime {
                runtime.shutdown().await;
            }
            Ok(())
        })
}
