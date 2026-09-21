use crate::net_util;
use anyhow::{Context, Result, bail};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;

pub fn run(dir: PathBuf, port: u16) -> Result<()> {
    run_with_options(dir, port, false)
}

pub fn run_with_options(dir: PathBuf, mut port: u16, auto_port: bool) -> Result<()> {
    if !dir.exists() {
        bail!("پوشه وجود ندارد: {}", dir.display());
    }

    if auto_port {
        port = net_util::find_free_port(port)?;
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("ساخت runtime ناموفق بود.")?;

    rt.block_on(async move {
        let serve_dir = ServeDir::new(&dir).append_index_html_on_directories(true);
        let app = axum::Router::new().nest_service("/", serve_dir);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = net_util::bind_or_explain(port)?;
        listener.set_nonblocking(true)?;

        println!("Serving {} at http://{}", dir.display(), addr);
        println!("Press Ctrl+C to stop.");

        axum::Server::from_tcp(listener)?
            .serve(app.into_make_service())
            .await?;
        Ok::<(), anyhow::Error>(())
    })
}
