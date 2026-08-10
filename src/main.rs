mod downloader;
mod rewriter;
mod zipper;
mod serve;
mod utils;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "webcloner",
    version,
    about = "Clone a website (HTML/CSS/JS/images/fonts) for fully offline use.",
    long_about = "webcloner downloads a website's pages and all front-end assets \
(HTML, CSS, JS, images, fonts) referenced by them, rewrites every reference to a \
local relative path, and stores the result in a self-contained folder. \
The folder can then be zipped, served locally, or opened directly (index.html \
works with file:// with no internet access required)."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download a website: pages + all front-end assets, rewritten for offline use
    Download {
        /// Starting URL, e.g. https://example.com
        url: String,

        /// Output directory for the cloned site
        #[arg(short, long, default_value = "cloned-site")]
        out: PathBuf,

        /// Maximum number of HTML pages to crawl (same domain only)
        #[arg(long, default_value_t = 40)]
        max_pages: usize,

        /// Maximum link-following depth from the start URL
        #[arg(long, default_value_t = 3)]
        max_depth: usize,

        /// Also download assets hosted on other domains (CDNs, Google Fonts, etc.)
        #[arg(long, default_value_t = true)]
        include_external_assets: bool,

        /// Also follow and clone links that point to other domains (off by default)
        #[arg(long, default_value_t = false)]
        follow_external_pages: bool,

        /// Number of concurrent download workers
        #[arg(long, default_value_t = 8)]
        concurrency: usize,

        /// Request timeout in seconds
        #[arg(long, default_value_t = 20)]
        timeout: u64,

        /// After downloading, also produce a .zip of the output directory
        #[arg(long, default_value_t = false)]
        zip: bool,

        /// User-Agent header to send
        #[arg(long, default_value = "webcloner/0.1 (+offline mirror tool)")]
        user_agent: String,
    },

    /// Serve a previously downloaded site folder over HTTP (for local browsing/testing)
    Serve {
        /// Path to a folder produced by `webcloner download`
        dir: PathBuf,

        /// Port to listen on
        #[arg(short, long, default_value_t = 8787)]
        port: u16,
    },

    /// Zip an existing downloaded folder
    Zip {
        /// Path to a folder produced by `webcloner download`
        dir: PathBuf,

        /// Output .zip file path
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Download {
            url,
            out,
            max_pages,
            max_depth,
            include_external_assets,
            follow_external_pages,
            concurrency,
            timeout,
            zip,
            user_agent,
        } => {
            let opts = downloader::CrawlOptions {
                start_url: url,
                out_dir: out.clone(),
                max_pages,
                max_depth,
                include_external_assets,
                follow_external_pages,
                concurrency,
                timeout_secs: timeout,
                user_agent,
            };
            downloader::run(opts)?;

            if zip {
                let zip_path = out.with_extension("zip");
                zipper::zip_dir(&out, &zip_path)?;
                println!("\n📦 Zip created: {}", zip_path.display());
            }
        }

        Commands::Serve { dir, port } => {
            serve::run(dir, port)?;
        }

        Commands::Zip { dir, out } => {
            let out_path = out.unwrap_or_else(|| dir.with_extension("zip"));
            zipper::zip_dir(&dir, &out_path)?;
            println!("📦 Zip created: {}", out_path.display());
        }
    }

    Ok(())
}
