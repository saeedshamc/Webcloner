use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;

pub fn run(dir: PathBuf, port: u16) -> Result<()> {
    if !dir.exists() {
        anyhow::bail!("directory does not exist: {}", dir.display());
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?;

    rt.block_on(async move {
        let serve_dir = ServeDir::new(&dir).append_index_html_on_directories(true);
        let app = axum::Router::new().nest_service("/", serve_dir);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        println!("🚀 Serving {} at http://{}", dir.display(), addr);
        println!("   Press Ctrl+C to stop.");

        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await?;
        Ok::<(), anyhow::Error>(())
    })
}
