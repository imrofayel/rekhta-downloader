use anyhow::{Context, Result};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookInfo {
    pub book_id: String,
    pub book_slug: String,
    pub title: String,
    pub total_pages: usize,
    pub pages: Vec<String>,
    pub page_ids: Vec<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<String>,
    pub cover_url: Option<String>,
}

pub async fn extract_book_data(client: &Client, url: &str) -> Result<BookInfo> {
    let body = client.get(url).send().await?.text().await?;
    parse_book_data(&body)
}

fn parse_book_data(body: &str) -> Result<BookInfo> {
    let book_slug_re = Regex::new(r#"bookslug\s*=\s*["']([^"']+)["']"#)?;
    let book_id_re = Regex::new(r#"bookId\s*=\s*["']([^"']+)["']"#)?;
    let pages_re = Regex::new(r#"pages\s*=\s*(\[[^\]]+\])"#)?;
    let page_ids_re = Regex::new(r#"pageIds\s*=\s*(\[[^\]]+\])"#)?;

    let book_slug = book_slug_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .context("Could not find bookSlug")?;

    let book_id = book_id_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .context("Could not find bookId")?;

    let pages_json = pages_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().replace("'", "\""))
        .context("Could not find pages array")?;

    let page_ids_json = page_ids_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().replace("'", "\""))
        .context("Could not find pageIds array")?;

    let pages: Vec<String> = serde_json::from_str(&pages_json)?;
    let page_ids: Vec<String> = serde_json::from_str(&page_ids_json)?;

    if pages.len() != page_ids.len() {
        anyhow::bail!(
            "Page count mismatch: {} vs {}",
            pages.len(),
            page_ids.len()
        );
    }

    // Extract title from HTML
    let mut title = format_book_title(&book_slug);
    if let Some(cap) =
        Regex::new(r#"<div class="B-descript">\s*<h5>([^<]+)</h5>"#)?.captures(body)
    {
        if let Some(m) = cap.get(1) {
            title = m.as_str().trim().to_string();
        }
    }

    // Extract author
    let mut author = None;
    if let Some(cap) =
        Regex::new(r#"AUTHOR<span><a[^>]*>([^<]+)</a></span>"#)?.captures(body)
    {
        if let Some(m) = cap.get(1) {
            author = Some(m.as_str().trim().to_string());
        }
    }

    // Extract publisher
    let mut publisher = None;
    if let Some(cap) =
        Regex::new(r#"PUBLISHER<span>\s*([^<]+)\s*</span>"#)?.captures(body)
    {
        if let Some(m) = cap.get(1) {
            publisher = Some(m.as_str().trim().to_string());
        }
    }

    // Extract year
    let mut year = None;
    if let Some(cap) = Regex::new(r#"YEAR<span>([^<]+)</span>"#)?.captures(body) {
        if let Some(m) = cap.get(1) {
            year = Some(m.as_str().trim().to_string());
        }
    }

    // Extract cover URL from og:image meta tag (unscrambled thumbnail)
    let mut cover_url = None;
    if let Some(cap) =
        Regex::new(r#"<meta\s+property="og:image"\s+content="([^"]+)""#)?.captures(body)
    {
        if let Some(m) = cap.get(1) {
            cover_url = Some(m.as_str().trim().to_string());
        }
    }
    // Fallback: try content before property order
    if cover_url.is_none() {
        if let Some(cap) =
            Regex::new(r#"<meta\s+content="([^"]+)"\s+property="og:image""#)?.captures(body)
        {
            if let Some(m) = cap.get(1) {
                cover_url = Some(m.as_str().trim().to_string());
            }
        }
    }
    // Last resort fallback to Rekhta's thumbnail pattern
    if cover_url.is_none() {
        cover_url = Some(format!(
            "https://www.rekhta.org/Images/ebook_thumbnails/{}.jpg",
            book_slug
        ));
    }

    Ok(BookInfo {
        book_id,
        book_slug,
        title,
        total_pages: pages.len(),
        pages,
        page_ids,
        author,
        publisher,
        year,
        cover_url,
    })
}

fn format_book_title(slug: &str) -> String {
    slug.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
