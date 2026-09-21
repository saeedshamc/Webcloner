use clap::{Parser, Subcommand};
use std::path::PathBuf;
use webcloner::{downloader, serve, zipper};

#[derive(Parser)]
#[command(
    name = "webcloner",
    version,
    about = "Clone a website (HTML/CSS/JS/images/fonts) for fully offline use.",
    long_about = "webcloner downloads a website's pages and all front-end assets \
(HTML, CSS, JS, images, fonts) referenced by them, rewrites every reference to a \
local relative path, and stores the result in a self-contained folder."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download a website: pages + all front-end assets, rewritten for offline use
    Download {
        url: String,
        #[arg(short, long, default_value = "cloned-site")]
        out: PathBuf,
        #[arg(long, default_value_t = 40)]
        max_pages: usize,
        #[arg(long, default_value_t = 3)]
        max_depth: usize,
        #[arg(long, default_value_t = true)]
        include_external_assets: bool,
        #[arg(long, default_value_t = false)]
        follow_external_pages: bool,
        #[arg(long, default_value_t = 8)]
        concurrency: usize,
        #[arg(long, default_value_t = 20)]
        timeout: u64,
        #[arg(long, default_value_t = false)]
        zip: bool,
        #[arg(long, default_value_t = true)]
        block_tracking: bool,
        #[arg(long, default_value_t = true)]
        report_broken_links: bool,
        #[arg(long, default_value = "webcloner/1.0 (+offline mirror tool)")]
        user_agent: String,
    },

    /// Serve a previously downloaded site folder over HTTP
    Serve {
        dir: PathBuf,
        #[arg(short, long, default_value_t = 8787)]
        port: u16,
        /// If the preferred port is busy, pick the next free one
        #[arg(long, default_value_t = false)]
        auto_port: bool,
    },

    /// Zip an existing downloaded folder
    Zip {
        dir: PathBuf,
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
            block_tracking,
            report_broken_links,
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
                block_tracking,
                report_broken_links,
                planned_pages: None,
                resume: false,
                on_progress: None,
                on_progress_event: None,
                cancel_flag: None,
            };
            downloader::run(opts)?;

            if zip {
                let zip_path = out.with_extension("zip");
                zipper::zip_dir(&out, &zip_path)?;
                println!("\nZip created: {}", zip_path.display());
            }
        }

        Commands::Serve {
            dir,
            port,
            auto_port,
        } => {
            serve::run_with_options(dir, port, auto_port)?;
        }

        Commands::Zip { dir, out } => {
            let out_path = out.unwrap_or_else(|| dir.with_extension("zip"));
            zipper::zip_dir(&dir, &out_path)?;
            println!("Zip created: {}", out_path.display());
        }
    }

    Ok(())
}
