use anyhow::{Context, Result, bail};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;

pub fn run(dir: PathBuf, port: u16) -> Result<()> {
    if !dir.exists() {
        bail!("directory does not exist: {}", dir.display());
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?;

    rt.block_on(async move {
        let serve_dir = ServeDir::new(&dir).append_index_html_on_directories(true);
        let app = axum::Router::new().nest_service("/", serve_dir);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = std::net::TcpListener::bind(addr).with_context(|| {
            format!(
                "پورت {port} در دسترس نیست (احتمالاً اشغال است). پورت دیگری انتخاب کنید."
            )
        })?;
        listener.set_nonblocking(true)?;

        println!("🚀 Serving {} at http://{}", dir.display(), addr);
        println!("   Press Ctrl+C to stop.");

        axum::Server::from_tcp(listener)?
            .serve(app.into_make_service())
            .await?;
        Ok::<(), anyhow::Error>(())
    })
}
