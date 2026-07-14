use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use reqwest::Client;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

mod client;
mod downloader;
mod metadata;
mod pdf;
mod utils;

// Shared state — single HTTP client + cancellation flag
struct AppState {
    cancel_flag: Arc<AtomicBool>,
    http_client: Client,
}

#[tauri::command]
async fn fetch_book_info(app: AppHandle, url: String) -> Result<metadata::BookInfo, String> {
    if !url.contains("rekhta.org/ebooks/") {
        return Err("Invalid URL. Must be a Rekhta ebook URL.".into());
    }

    let state = app.state::<AppState>();
    metadata::extract_book_data(&state.http_client, &url)
        .await
        .map_err(|e| format!("Failed to fetch book info: {}", e))
}

#[tauri::command]
async fn download_book(app: AppHandle, url: String) -> Result<String, String> {
    if !url.contains("rekhta.org/ebooks/") {
        return Err("Invalid URL. Must be a Rekhta ebook URL.".into());
    }

    // Reset cancel flag
    let state = app.state::<AppState>();
    state.cancel_flag.store(false, Ordering::Relaxed);
    let cancel_flag = state.cancel_flag.clone();
    let client = state.http_client.clone();

    // Configure rayon thread pool (safe to call multiple times)
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(num_cpus::get())
        .stack_size(2 * 1024 * 1024)
        .build_global();

    // 1. Fetch metadata
    let _ = app.emit(
        "download-progress",
        downloader::DownloadProgress {
            completed: 0,
            total: 0,
            percentage: 0.0,
            message: "Fetching book information...".into(),
            phase: "metadata".into(),
        },
    );

    let book_info = metadata::extract_book_data(&client, &url)
        .await
        .map_err(|e| format!("Failed to fetch book info: {}", e))?;

    let _ = app.emit(
        "download-progress",
        downloader::DownloadProgress {
            completed: 0,
            total: book_info.total_pages,
            percentage: 0.0,
            message: format!(
                "Found: {} ({} pages)",
                book_info.title, book_info.total_pages
            ),
            phase: "metadata".into(),
        },
    );

    // 2. Set up cache directory in app data
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let cache_dir = app_data.join("cache").join(&book_info.book_id);

    // 3. Download all pages
    let downloaded_files =
        downloader::download_book(&app, &client, &book_info, &cache_dir, cancel_flag.clone())
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

    if cancel_flag.load(Ordering::Relaxed) {
        return Err("Download cancelled by user".into());
    }

    // 4. Ask user where to save the PDF
    let sanitized = utils::sanitize_filename(&book_info.title);
    let default_name = format!("{}.pdf", sanitized);

    let save_path = tauri_plugin_dialog::FileDialogBuilder::new(app.dialog().clone())
        .set_file_name(&default_name)
        .add_filter("PDF Files", &["pdf"])
        .set_title("Save Book As PDF")
        .blocking_save_file();

    let output_path = match save_path {
        Some(path) => path.as_path().unwrap().to_path_buf(),
        None => return Err("Save cancelled by user".into()),
    };

    // 5. Generate PDF
    let _ = app.emit(
        "download-progress",
        downloader::DownloadProgress {
            completed: book_info.total_pages,
            total: book_info.total_pages,
            percentage: 100.0,
            message: "Creating PDF...".into(),
            phase: "creating_pdf".into(),
        },
    );

    pdf::create_pdf(&app, &downloaded_files, &output_path, book_info.total_pages)
        .map_err(|e| format!("PDF creation failed: {}", e))?;

    // 6. Clean up cache for this book
    let _ = std::fs::remove_dir_all(&cache_dir);

    let _ = app.emit(
        "download-progress",
        downloader::DownloadProgress {
            completed: book_info.total_pages,
            total: book_info.total_pages,
            percentage: 100.0,
            message: format!("Saved to {}", output_path.display()),
            phase: "complete".into(),
        },
    );

    Ok(format!(
        "Successfully saved {} ({} pages) to {}",
        book_info.title,
        downloaded_files.len(),
        output_path.display()
    ))
}

#[tauri::command]
fn cancel_download(app: AppHandle) {
    let state = app.state::<AppState>();
    state.cancel_flag.store(true, Ordering::Relaxed);
    let _ = app.emit(
        "download-progress",
        downloader::DownloadProgress {
            completed: 0,
            total: 0,
            percentage: 0.0,
            message: "Cancelling download...".into(),
            phase: "cancelled".into(),
        },
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let http_client = client::create_client().expect("Failed to create HTTP client");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            http_client,
        })
        .invoke_handler(tauri::generate_handler![
            fetch_book_info,
            download_book,
            cancel_download,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
