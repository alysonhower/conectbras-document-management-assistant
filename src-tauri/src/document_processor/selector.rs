use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use log;
use lopdf::Document;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_shell::ShellExt;

const IMAGE_DENSITY: &str = "150";
const IMAGE_RESIZE: &str = "1000x1000";
const IMAGE_FORMAT: &str = "webp";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    CommandError(#[from] anyhow::Error),
}

impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

#[derive(Debug, Clone, Serialize)]
struct ImageLoaded {
    page_number: u16,
    path: String,
    data: Vec<u8>,
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub fn select_document(app: AppHandle) -> Result<PathBuf, Error> {
    let path = app.dialog()
        .file()
        .blocking_pick_file()
        .map(|selection| selection.path);

    match path {
        Some(path) => Ok(path),
        None => Err(Error::CommandError(anyhow!("No document selected"))),
    }
}

#[tauri::command]
pub async fn prepare_document(app: AppHandle, path: PathBuf) -> Result<String, Error> {
    preparation(app, path).await.map_err(Error::CommandError)
}

async fn preparation(app: tauri::AppHandle, path: PathBuf) -> Result<String> {
    log::info!("Preparing document: {}", path.display());
    let (data_dir, _output_file_name) = create_output_paths(&path)?;
    let page_count = get_page_count(&path)?;
    let input = path.to_string_lossy();
    
    if data_dir.exists() {
        handle_existing_data_dir(&data_dir, page_count, &app, &input).await?;
    } else {
        fs::create_dir(&data_dir).context("Failed to create data directory")?;
        process_pages(&app, &input, &data_dir, page_count).await?;
    }
    
    Ok(path.display().to_string())
}

fn create_output_paths(path: &Path) -> Result<(PathBuf, PathBuf)> {
    let path_without_ext = path.with_extension("");
    let file_name = path_without_ext.file_name().unwrap().to_string_lossy();
    let data_dir = path_without_ext.with_file_name(format!("{}_data", &file_name));
    let output_file_name = data_dir.join("page").with_extension(IMAGE_FORMAT);
    Ok((data_dir, output_file_name))
}

fn get_page_count(path: &Path) -> Result<usize> {
    Document::load(path)
        .map(|doc| doc.get_pages().len())
        .context("Failed to load PDF document")
}

fn create_magick_args<'a>(input: &'a str, output: &'a str) -> Vec<&'a str> {
    vec![
        "-density",
        IMAGE_DENSITY,
        input,
        "-resize",
        IMAGE_RESIZE,
        "-scene",
        "1",
        "+adjoin",
        output,
    ]
}

async fn handle_existing_data_dir(
    data_dir: &Path,
    page_count: usize,
    app: &AppHandle,
    input: &str,
) -> Result<()> {
    log::info!("Data dir already exists. Verifying...");
    let webp_file_count = count_webp_files(data_dir)?;

    if webp_file_count == page_count {
        log::info!("All pages are already processed. Emitting existing images.");
        emit_existing_images(app, data_dir, page_count)?;
    } else {
        log::warn!(
            "Mismatch in page count. PDF has {} pages, but found {} webp files.",
            page_count,
            webp_file_count
        );
        remove_existing_webp_files(data_dir)?;
        process_pages(app, input, data_dir, page_count).await?;
    }
    Ok(())
}

fn emit_existing_images(app: &AppHandle, data_dir: &Path, page_count: usize) -> Result<()> {
    for page in 1..=page_count {
        let file_path = data_dir.join(format!("{}.{}", page, IMAGE_FORMAT));
        send_webp_image(app, &file_path, page)?;
    }
    Ok(())
}

async fn process_pages(app: &AppHandle, input: &str, data_dir: &Path, page_count: usize) -> Result<()> {
    for page in 0..page_count {
        let output = data_dir.join(format!("{}.{}", page + 1, IMAGE_FORMAT));
        let page_arg = format!("{}[{}]", input, page);
        let args = create_magick_args(&page_arg, output.to_str().unwrap());
        run_magick(app, &args).await?;
        send_webp_image(app, &output, page + 1)?;
    }
    Ok(())
}

fn count_webp_files(dir: &Path) -> Result<usize> {
    Ok(fs::read_dir(dir)
        .context("Failed to read data directory")?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension() == Some(OsStr::new(IMAGE_FORMAT)))
        .count())
}

fn remove_existing_webp_files(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).context("Failed to read data directory")? {
        let path = entry?.path();
        if path.extension() == Some(OsStr::new(IMAGE_FORMAT)) {
            log::info!("Removing {}", path.display());
            fs::remove_file(&path).context("Failed to remove existing webp file")?;
        }
    }
    Ok(())
}

async fn run_magick(app: &AppHandle, args: &[&str]) -> Result<()> {
    let output = app
        .shell()
        .command("magick.exe")
        .args(args)
        .output()
        .await
        .context("Failed to run magick command")?;

    if output.status.success() {
        log::info!(
            "Magick command succeeded: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        Ok(())
    } else {
        Err(anyhow!(
            "Magick command failed with exit code {}, stderr: {}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn send_webp_image(app: &AppHandle, path: &Path, page_number: usize) -> Result<()> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    log::info!("Sending image: {}", path.display());
    log::info!("Sending page number: {}", page_number);

    app.emit(
        "image",
        ImageLoaded {
            page_number: page_number as u16,
            path: path.display().to_string(),
            data: buffer,
        },
    )?;
    
    Ok(())
}
