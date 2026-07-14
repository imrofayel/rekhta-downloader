use anyhow::{Context, Result};
use lopdf::{Document, Object, Stream};
use lopdf::dictionary;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

use crate::downloader::DownloadProgress;

/// Parse JPEG header to extract width and height from SOF marker.
/// Only reads a few hundred bytes — no full image decode.
fn jpeg_dimensions(data: &[u8]) -> Result<(u32, u32)> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        anyhow::bail!("Not a valid JPEG file");
    }
    let mut i = 2;

    while i + 1 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }

        let marker = data[i + 1];
        i += 2;

        // SOF markers (0xC0-0xCF except 0xC4 and 0xCC)
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xCC {
            if i + 7 <= data.len() {
                let height = ((data[i + 3] as u32) << 8) | (data[i + 4] as u32);
                let width = ((data[i + 5] as u32) << 8) | (data[i + 6] as u32);
                return Ok((width, height));
            }
        }

        // Skip to next marker
        if i + 1 < data.len() {
            let len = ((data[i] as usize) << 8) | (data[i + 1] as usize);
            i += len;
        } else {
            break;
        }
    }

    anyhow::bail!("Could not find JPEG dimensions");
}

/// Convert any image to JPEG bytes. Used for non-JPEG formats (PNG etc.)
/// so everything goes through the fast DCTDecode path.
fn to_jpeg_bytes(data: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(data)?;
    let width = img.width();
    let height = img.height();
    let rgb = img.to_rgb8();

    let mut jpeg_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_buf);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 95);
    encoder.encode(
        rgb.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;

    Ok((jpeg_buf, width, height))
}

pub fn create_pdf(
    app: &AppHandle,
    files: &[PathBuf],
    output_path: &Path,
    _total_book_pages: usize,
) -> Result<()> {
    if files.is_empty() {
        anyhow::bail!("No pages to compile into PDF");
    }

    let total = files.len();

    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            completed: 0,
            total,
            percentage: 100.0,
            message: "Creating PDF...".into(),
            phase: "creating_pdf".into(),
        },
    );

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::with_capacity(total);

    for (i, path) in files.iter().enumerate() {
        let data = std::fs::read(path)
            .with_context(|| format!("Failed to read image {:?}", path))?;

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_jpeg = ext == "jpg" || ext == "jpeg";

        // Get JPEG bytes + dimensions — for JPEGs just parse header, for
        // anything else convert to JPEG so everything uses DCTDecode (fast).
        let (jpeg_data, width, height) = if is_jpeg {
            let (w, h) = jpeg_dimensions(&data)?;
            (data, w, h)
        } else {
            to_jpeg_bytes(&data)?
        };

        // Create the XObject image stream — embed raw JPEG bytes directly
        let img_stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width as i64,
                "Height" => height as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8_i64,
                "Filter" => "DCTDecode",
            },
            jpeg_data,
        );

        let img_id = doc.add_object(img_stream);

        // Page dimensions in points (1px = 1pt)
        let page_width = width as f32;
        let page_height = height as f32;

        // Content stream: draw image scaled to fill page
        let content = format!(
            "q {} 0 0 {} 0 0 cm /Img Do Q",
            page_width, page_height
        );

        let content_id = doc.add_object(Stream::new(
            dictionary! {},
            content.into_bytes(),
        ));

        // Page object
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(page_width), Object::Real(page_height)],
            "Contents" => content_id,
            "Resources" => dictionary! {
                "XObject" => dictionary! {
                    "Img" => img_id,
                },
            },
        });

        page_ids.push(page_id);

        // Progress every 20 pages or at end
        if (i + 1) % 20 == 0 || i + 1 == total {
            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    completed: i + 1,
                    total,
                    percentage: 100.0,
                    message: format!("Added page {}/{} to PDF", i + 1, total),
                    phase: "creating_pdf".into(),
                },
            );
        }
    }

    // Pages node
    let pages_object = dictionary! {
        "Type" => "Pages",
        "Count" => page_ids.len() as i64,
        "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages_object));

    // Catalog
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            completed: total,
            total,
            percentage: 100.0,
            message: "Writing PDF to disk...".into(),
            phase: "creating_pdf".into(),
        },
    );

    // Save — no doc.compress() since JPEG is already compressed
    doc.save(output_path)
        .with_context(|| format!("Failed to save PDF to {:?}", output_path))?;

    Ok(())
}
