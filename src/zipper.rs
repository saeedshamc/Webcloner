use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use walkdir::WalkDir;
use zip::write::FileOptions;

pub fn zip_dir(src_dir: &Path, zip_path: &Path) -> Result<()> {
    if !src_dir.exists() {
        anyhow::bail!("directory does not exist: {}", src_dir.display());
    }

    let file = File::create(zip_path)
        .with_context(|| format!("failed to create {}", zip_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: FileOptions = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut buffer = Vec::new();
    for entry in WalkDir::new(src_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let rel = path.strip_prefix(src_dir).unwrap();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let name = rel.to_string_lossy().replace('\\', "/");

        if path.is_dir() {
            zip.add_directory(format!("{name}/"), options)?;
        } else {
            zip.start_file(name, options)?;
            let mut f = File::open(path)?;
            buffer.clear();
            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
        }
    }

    zip.finish()?;
    Ok(())
}
