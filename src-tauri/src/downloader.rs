use crate::metadata::BookInfo;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use image::{imageops, GenericImageView, ImageBuffer, RgbaImage};
use reqwest::Client;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::fs;

const API_BASE_URL: &str =
    "https://ebooksapi.rekhta.org/api_getebookpagebyid_websiteapp/?wref=from-site&&pgid=";
const MAX_RETRIES: u32 = 5;
const CONCURRENCY_LIMIT: usize = 20;

const TILE_SIZE: u32 = 50;
const TILE_PADDING: u32 = 16;

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct SubTile {
    X1: f64,
    Y1: f64,
    X2: f64,
    Y2: f64,
}

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct PageData {
    PageWidth: f64,
    PageHeight: f64,
    Sub: Option<Vec<SubTile>>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub completed: usize,
    pub total: usize,
    pub percentage: f64,
    pub message: String,
    pub phase: String,
}

/// Fast validation: just check file exists and has reasonable size.
/// Full image decode on every cached file was the main perf bottleneck.
async fn is_valid_cached_page(path: &Path) -> bool {
    match fs::metadata(path).await {
        Ok(meta) => meta.len() > 4096, // valid descrambled images are always > 4KB
        Err(_) => false,
    }
}

/// Download all pages of a book with resume support, retries, and progress reporting.
pub async fn download_book(
    app: &AppHandle,
    client: &Client,
    book_info: &BookInfo,
    cache_dir: &Path,
    cancel_flag: Arc<AtomicBool>,
) -> Result<Vec<PathBuf>> {
    // Ensure cache directory exists
    fs::create_dir_all(cache_dir).await?;

    let total = book_info.total_pages;

    // Emit initial progress
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            completed: 0,
            total,
            percentage: 0.0,
            message: format!("Starting download of {} pages...", total),
            phase: "downloading".into(),
        },
    );

    // Build task list
    let completed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let tasks: Vec<_> = book_info
        .pages
        .iter()
        .enumerate()
        .map(|(idx, page_name)| {
            let page_id = book_info.page_ids[idx].clone();
            let book_id = book_info.book_id.clone();
            let client = client.clone();
            let app = app.clone();
            let cancel = cancel_flag.clone();
            let completed = completed_count.clone();
            let cache = cache_dir.to_path_buf();
            let page = page_name.clone();

            async move {
                if cancel.load(Ordering::Relaxed) {
                    return Err(anyhow::anyhow!("Download cancelled"));
                }

                let file_name = format!("{:04}_{}", idx, page);
                let output_path = cache.join(&file_name);

                // Resume: skip if already downloaded (fast size check only)
                if is_valid_cached_page(&output_path).await {
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = app.emit(
                        "download-progress",
                        DownloadProgress {
                            completed: done,
                            total,
                            percentage: (done as f64 / total as f64) * 100.0,
                            message: format!("Page {} (cached)", idx + 1),
                            phase: "downloading".into(),
                        },
                    );
                    return Ok((idx, output_path));
                }

                // Download with retries
                match download_page_with_retry(
                    &client,
                    &book_id,
                    &page,
                    &page_id,
                    &output_path,
                    &cancel,
                )
                .await
                {
                    Ok(_) => {
                        let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        let _ = app.emit(
                            "download-progress",
                            DownloadProgress {
                                completed: done,
                                total,
                                percentage: (done as f64 / total as f64) * 100.0,
                                message: format!("Downloaded page {}/{}", done, total),
                                phase: "downloading".into(),
                            },
                        );
                        Ok((idx, output_path))
                    }
                    Err(e) => {
                        eprintln!("Failed page {} ({}): {:?}", idx, page, e);
                        Err(e)
                    }
                }
            }
        })
        .collect();

    // Execute with controlled concurrency
    let results: Vec<Result<(usize, PathBuf)>> = stream::iter(tasks)
        .buffer_unordered(CONCURRENCY_LIMIT)
        .collect()
        .await;

    // Separate successes and failures
    let mut successful: Vec<(usize, PathBuf)> = Vec::new();
    let mut failed_indices: Vec<usize> = Vec::new();

    for (idx, result) in results.into_iter().enumerate() {
        match result {
            Ok(pair) => successful.push(pair),
            Err(_) => failed_indices.push(idx),
        }
    }

    // SECOND PASS: Retry failed pages individually with longer backoff
    if !failed_indices.is_empty() && !cancel_flag.load(Ordering::Relaxed) {
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                completed: successful.len(),
                total,
                percentage: (successful.len() as f64 / total as f64) * 100.0,
                message: format!("Retrying {} failed pages...", failed_indices.len()),
                phase: "retrying".into(),
            },
        );

        for &idx in &failed_indices {
            if cancel_flag.load(Ordering::Relaxed) {
                break;
            }

            let page_name = &book_info.pages[idx];
            let page_id = &book_info.page_ids[idx];
            let file_name = format!("{:04}_{}", idx, page_name);
            let output_path = cache_dir.join(&file_name);

            for attempt in 1..=MAX_RETRIES {
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(2 * attempt as u64)).await;

                match download_page_attempt(
                    client,
                    &book_info.book_id,
                    page_name,
                    page_id,
                    &output_path,
                )
                .await
                {
                    Ok(_) => {
                        if is_valid_cached_page(&output_path).await {
                            successful.push((idx, output_path.clone()));
                            let done = successful.len();
                            let _ = app.emit(
                                "download-progress",
                                DownloadProgress {
                                    completed: done,
                                    total,
                                    percentage: (done as f64 / total as f64) * 100.0,
                                    message: format!(
                                        "Recovered page {} (attempt {})",
                                        idx + 1,
                                        attempt
                                    ),
                                    phase: "retrying".into(),
                                },
                            );
                            break;
                        }
                    }
                    Err(e) => {
                        if attempt == MAX_RETRIES {
                            eprintln!(
                                "Page {} permanently failed after {} retries: {:?}",
                                idx, MAX_RETRIES, e
                            );
                        }
                    }
                }
            }
        }
    }

    // THIRD PASS: Final verification — check for still-missing pages
    let successful_indices: std::collections::HashSet<usize> =
        successful.iter().map(|(idx, _)| *idx).collect();

    let still_missing: Vec<usize> = (0..total)
        .filter(|idx| !successful_indices.contains(idx))
        .collect();

    if !still_missing.is_empty() {
        eprintln!(
            "Warning: {} pages could not be downloaded: {:?}",
            still_missing.len(),
            still_missing
        );
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                completed: successful.len(),
                total,
                percentage: (successful.len() as f64 / total as f64) * 100.0,
                message: format!(
                    "Warning: {} pages missing, creating PDF with {} pages",
                    still_missing.len(),
                    successful.len()
                ),
                phase: "warning".into(),
            },
        );
    }

    // Sort by index to ensure correct page order
    successful.sort_by_key(|(idx, _)| *idx);
    let ordered_paths: Vec<PathBuf> = successful.into_iter().map(|(_, path)| path).collect();

    if ordered_paths.is_empty() {
        anyhow::bail!("No pages were downloaded successfully");
    }

    Ok(ordered_paths)
}

async fn download_page_with_retry(
    client: &Client,
    book_id: &str,
    image_name: &str,
    page_id: &str,
    output_path: &Path,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<()> {
    let mut last_error = anyhow::anyhow!("Unknown error");

    for attempt in 1..=MAX_RETRIES {
        if cancel_flag.load(Ordering::Relaxed) {
            anyhow::bail!("Download cancelled");
        }

        // Exponential backoff: 500ms, 1s, 2s, 4s, 8s
        if attempt > 1 {
            let delay = Duration::from_millis(500 * (1 << (attempt - 1)));
            tokio::time::sleep(delay).await;
        }

        match download_page_attempt(client, book_id, image_name, page_id, output_path).await {
            Ok(_) => {
                if is_valid_cached_page(output_path).await {
                    return Ok(());
                } else {
                    last_error = anyhow::anyhow!("Downloaded image is invalid/corrupt");
                    let _ = fs::remove_file(output_path).await;
                }
            }
            Err(e) => {
                last_error = e;
            }
        }
    }

    Err(last_error.context(format!("Failed after {} attempts", MAX_RETRIES)))
}

async fn download_page_attempt(
    client: &Client,
    book_id: &str,
    image_name: &str,
    page_id: &str,
    output_path: &Path,
) -> Result<()> {
    // 1. Fetch page data (tile mapping info)
    let page_data_url = format!("{}{}", API_BASE_URL, page_id);
    let page_resp = client.get(&page_data_url).send().await?;

    if !page_resp.status().is_success() {
        anyhow::bail!("Page data request failed: {}", page_resp.status());
    }

    let page_data: PageData = page_resp.json().await?;
    let sub_tiles = page_data
        .Sub
        .ok_or_else(|| anyhow::anyhow!("Invalid page data: no Sub tiles"))?;

    // 2. Fetch the scrambled image
    let image_url = format!(
        "https://ebooksapi.rekhta.org/images/{}/{}",
        book_id, image_name
    );
    let image_resp = client.get(&image_url).send().await?;

    if !image_resp.status().is_success() {
        anyhow::bail!("Image request failed: {}", image_resp.status());
    }

    let image_bytes = image_resp.bytes().await?;

    // 3. Decode the scrambled image
    let scrambled_img = tokio::task::spawn_blocking({
        let bytes = image_bytes.clone();
        move || image::load_from_memory(&bytes)
    })
    .await??;

    let width = page_data.PageWidth as u32;
    let height = page_data.PageHeight as u32;

    // 4. Descramble and save at maximum quality
    let output_path_owned = output_path.to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<()> {
        use rayon::prelude::*;

        let mut descrambled_img: RgbaImage = ImageBuffer::new(width, height);

        // Extract tiles in parallel (lock-free reads)
        let tile_data: Vec<_> = sub_tiles
            .par_iter()
            .filter_map(|tile| {
                let x1 = tile.X1 as u32;
                let y1 = tile.Y1 as u32;
                let x2 = tile.X2 as u32;
                let y2 = tile.Y2 as u32;

                let src_x = x1 * (TILE_SIZE + TILE_PADDING);
                let src_y = y1 * (TILE_SIZE + TILE_PADDING);
                let dest_x = x2 * TILE_SIZE;
                let dest_y = y2 * TILE_SIZE;

                if src_x + TILE_SIZE <= scrambled_img.width()
                    && src_y + TILE_SIZE <= scrambled_img.height()
                    && dest_x + TILE_SIZE <= width
                    && dest_y + TILE_SIZE <= height
                {
                    let tile_img = scrambled_img
                        .view(src_x, src_y, TILE_SIZE, TILE_SIZE)
                        .to_image();
                    Some((tile_img, dest_x, dest_y))
                } else {
                    None
                }
            })
            .collect();

        // Copy tiles to output (sequential — overlapping writes)
        for (tile_img, dest_x, dest_y) in tile_data {
            imageops::overlay(
                &mut descrambled_img,
                &tile_img,
                dest_x as i64,
                dest_y as i64,
            );
        }

        // 5. Save at MAXIMUM quality
        let ext = output_path_owned
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "jpg" || ext == "jpeg" {
            // Save JPEG at quality 100 (maximum)
            let rgb_img = image::DynamicImage::ImageRgba8(descrambled_img).to_rgb8();
            let file = std::fs::File::create(&output_path_owned)?;
            let mut buf = std::io::BufWriter::new(file);
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 100);
            encoder.encode(
                rgb_img.as_raw(),
                rgb_img.width(),
                rgb_img.height(),
                image::ExtendedColorType::Rgb8,
            )?;
        } else {
            // PNG is lossless by default — no quality loss
            descrambled_img.save(&output_path_owned)?;
        }

        Ok(())
    })
    .await??;

    Ok(())
}
