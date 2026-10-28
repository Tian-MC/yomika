//! VNDB Kana API client (<https://api.vndb.org/kana>).
//!
//! Verified against the published docs and live responses: `POST /vn` with
//! `{"filters": [...], "fields": "...", "sort": "searchrank", "results": N}`;
//! rate limit 200 requests / 5 min (HTTP 429 when throttled).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use vnl_core::{AppError, ErrorCode, Game, GameId, GameMetadata, now_secs};

use crate::Library;
use crate::types::VndbSearchResult;

pub const DEFAULT_BASE_URL: &str = "https://api.vndb.org/kana";
/// Environment override for the API base URL (development / mirrors).
pub const BASE_URL_ENV: &str = "VNL_VNDB_URL";
pub const USER_AGENT: &str =
    concat!("vn-launcher/", env!("CARGO_PKG_VERSION"), " (+https://example.invalid/vn-launcher)");

const SEARCH_FIELDS: &str = "title,alttitle,released,developers{name},image{url,sexual},rating";
const DETAIL_FIELDS: &str = "title,alttitle,titles{lang,title,latin,main},aliases,olang,released,description,\
     developers{name,original},image{id,url,sexual},rating,length,length_minutes,tags{name,rating,spoiler,lie,category}";
pub const MAX_TAGS: usize = 15;
const MAX_COVER_BYTES: usize = 15 * 1024 * 1024;
/// Longest `Retry-After` we are willing to wait for transparently.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(5);

// ------------------------------------------------------------- raw API types

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    #[serde(default = "Vec::new")]
    results: Vec<T>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawTitle {
    pub lang: Option<String>,
    pub title: String,
    pub latin: Option<String>,
    pub main: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawProducer {
    pub name: String,
    pub original: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawImage {
    pub id: Option<String>,
    pub url: Option<String>,
    pub sexual: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawTag {
    pub name: String,
    pub rating: f64,
    pub spoiler: i64,
    pub lie: bool,
    pub category: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawVn {
    pub id: String,
    pub title: String,
    pub alttitle: Option<String>,
    pub titles: Vec<RawTitle>,
    pub aliases: Vec<String>,
    pub olang: Option<String>,
    pub released: Option<String>,
    pub description: Option<String>,
    pub developers: Vec<RawProducer>,
    pub image: Option<RawImage>,
    pub rating: Option<f64>,
    pub length: Option<i64>,
    pub length_minutes: Option<i64>,
    pub tags: Vec<RawTag>,
}

// ------------------------------------------------------------------ parsing

pub fn parse_search(json: &str) -> Result<Vec<VndbSearchResult>, AppError> {
    let resp: ApiResponse<RawVn> = serde_json::from_str(json).map_err(bad_response)?;
    Ok(resp.results.into_iter().map(to_search_result).collect())
}

pub fn parse_vn(json: &str) -> Result<Option<RawVn>, AppError> {
    let resp: ApiResponse<RawVn> = serde_json::from_str(json).map_err(bad_response)?;
    Ok(resp.results.into_iter().next())
}

fn bad_response(e: serde_json::Error) -> AppError {
    AppError::new(ErrorCode::Network, "VNDB returned an unexpected response").with_details(e.to_string())
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn to_search_result(vn: RawVn) -> VndbSearchResult {
    let alt_title = non_empty(vn.alttitle).filter(|a| *a != vn.title);
    VndbSearchResult {
        id: vn.id,
        title: vn.title,
        alt_title,
        released: release_date(vn.released),
        developers: vn.developers.into_iter().map(|d| d.name).filter(|n| !n.is_empty()).collect(),
        image_url: vn.image.as_ref().and_then(|i| i.url.clone()),
        image_sexual: vn.image.as_ref().and_then(|i| i.sexual),
        rating: vn.rating,
    }
}

/// Only keep ISO-like dates (`YYYY[-MM[-DD]]`); drops `"TBA"`, `"unknown"`.
fn release_date(d: Option<String>) -> Option<String> {
    non_empty(d).filter(|d| d.len() >= 4 && d[..4].chars().all(|c| c.is_ascii_digit()))
}

/// Rough minutes for VNDB's 1–5 length class (used when nobody voted).
fn length_class_minutes(class: i64) -> Option<i64> {
    match class {
        1 => Some(60),
        2 => Some(6 * 60),
        3 => Some(20 * 60),
        4 => Some(40 * 60),
        5 => Some(60 * 60),
        _ => None,
    }
}

/// Original-script title: `alttitle`, else the main title entry, if it
/// differs from the display title.
pub fn original_title(vn: &RawVn) -> Option<String> {
    non_empty(vn.alttitle.clone())
        .or_else(|| vn.titles.iter().find(|t| t.main).map(|t| t.title.clone()))
        .filter(|t| !t.is_empty() && *t != vn.title)
}

/// Non-spoiler, non-sexual, non-"lie" tags ordered by rating.
pub fn safe_tags(tags: &[RawTag], max: usize) -> Vec<String> {
    let mut kept: Vec<&RawTag> = tags
        .iter()
        .filter(|t| t.spoiler == 0 && !t.lie && t.category.as_deref() != Some("ero") && !t.name.is_empty())
        .collect();
    kept.sort_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(std::cmp::Ordering::Equal));
    kept.into_iter().take(max).map(|t| t.name.clone()).collect()
}

/// Map a VNDB entry onto game metadata. Cover path is filled by the caller.
pub fn to_metadata(vn: &RawVn) -> GameMetadata {
    let mut aliases: Vec<String> = Vec::new();
    for a in vn.aliases.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        if !aliases.iter().any(|x| x == a) {
            aliases.push(a.to_string());
        }
    }
    // Other official titles (translations) help searching, e.g. 时空轮回.
    for t in &vn.titles {
        for candidate in [Some(t.title.as_str()), t.latin.as_deref()].into_iter().flatten() {
            let c = candidate.trim();
            if !c.is_empty() && c != vn.title && !aliases.iter().any(|x| x == c) {
                aliases.push(c.to_string());
            }
        }
    }
    GameMetadata {
        vndb_id: Some(vn.id.clone()),
        description: vn.description.as_deref().map(clean_bbcode).filter(|d| !d.is_empty()),
        developers: vn.developers.iter().map(|d| d.name.clone()).filter(|n| !n.is_empty()).collect(),
        release_date: release_date(vn.released.clone()),
        tags: safe_tags(&vn.tags, MAX_TAGS),
        aliases,
        rating: vn.rating,
        length_minutes: vn.length_minutes.filter(|m| *m > 0).or_else(|| vn.length.and_then(length_class_minutes)),
        cover_path: None,
        cover_url: vn.image.as_ref().and_then(|i| i.url.clone()),
        cover_sexual: vn.image.as_ref().and_then(|i| i.sexual),
        fetched_at: Some(now_secs()),
    }
}

/// Strip VNDB formatting codes: `[url=..]x[/url]` -> `x`, spoilers removed,
/// `[b] [i] [u] [s] [quote] [raw] [code]` unwrapped. Unknown brackets such as
/// `[From Wikipedia]` are kept verbatim.
pub fn clean_bbcode(input: &str) -> String {
    const SIMPLE: &[&str] = &["b", "i", "u", "s", "quote", "raw", "code"];
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = after[..end].trim().to_ascii_lowercase();
        let tail = &after[end + 1..];
        let name = tag.trim_start_matches('/').split('=').next().unwrap_or("");
        if tag == "spoiler" {
            rest = match find_ci(tail, "[/spoiler]") {
                Some(close) => &tail[close + "[/spoiler]".len()..],
                None => "",
            };
            continue;
        }
        if name == "url" || SIMPLE.contains(&name) || tag == "/spoiler" {
            rest = tail;
            continue;
        }
        out.push('[');
        rest = after;
    }
    out.push_str(rest);

    // Normalise whitespace: trim lines, at most one blank line in a row.
    let mut cleaned = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out.replace("\r\n", "\n").lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !cleaned.is_empty() {
            cleaned.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        cleaned.push_str(line.trim_start_matches(' '));
    }
    cleaned
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack.to_ascii_lowercase().find(needle)
}

/// Accept `v17`, `V17`, `17` and VNDB URLs such as `https://vndb.org/v17`.
pub fn normalize_vndb_id(raw: &str) -> Result<String, AppError> {
    let s = raw.trim().trim_end_matches('/');
    let s = s.rsplit('/').next().unwrap_or(s);
    let digits = s.strip_prefix(['v', 'V']).unwrap_or(s);
    if !digits.is_empty() && digits.len() <= 9 && digits.chars().all(|c| c.is_ascii_digit()) && digits != "0" {
        Ok(format!("v{}", digits.trim_start_matches('0')))
    } else {
        Err(AppError::invalid("Not a valid VNDB id (expected something like \"v17\")").with_details(raw.to_string()))
    }
}

// ------------------------------------------------------------------- client

#[derive(Clone)]
pub struct VndbClient {
    http: reqwest::Client,
    base_url: String,
}

impl std::fmt::Debug for VndbClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VndbClient").field("base_url", &self.base_url).finish()
    }
}

impl VndbClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, Duration::from_secs(20))
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10).min(timeout))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "falling back to default HTTP client");
                reqwest::Client::new()
            });
        Self { http, base_url: base_url.into().trim_end_matches('/').to_string() }
    }

    /// Default endpoint, overridable with `VNL_VNDB_URL`.
    pub fn from_env() -> Self {
        let url = std::env::var(BASE_URL_ENV).ok().filter(|v| !v.trim().is_empty());
        Self::new(url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()))
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn post_vn(&self, body: serde_json::Value) -> Result<String, AppError> {
        let url = format!("{}/vn", self.base_url);
        let mut retried = false;
        loop {
            let resp = self.http.post(&url).json(&body).send().await.map_err(network_error)?;
            let status = resp.status();
            if status.as_u16() == 429 {
                let wait = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(Duration::from_secs(2));
                if !retried && wait <= MAX_RETRY_WAIT {
                    retried = true;
                    tracing::info!(?wait, "VNDB rate limit hit, retrying once");
                    tokio::time::sleep(wait).await;
                    continue;
                }
                return Err(AppError::new(
                    ErrorCode::Network,
                    "VNDB is rate limiting requests. Please wait a minute and try again.",
                )
                .with_details(format!("HTTP 429 from {url}")));
            }
            let text = resp.text().await.map_err(network_error)?;
            if !status.is_success() {
                let message = match status.as_u16() {
                    400 => "VNDB rejected the request",
                    500..=599 => "VNDB is temporarily unavailable. Try again later.",
                    _ => "Unexpected response from VNDB",
                };
                let snippet: String = text.chars().take(300).collect();
                return Err(
                    AppError::new(ErrorCode::Network, message).with_details(format!("HTTP {status}: {snippet}"))
                );
            }
            return Ok(text);
        }
    }

    /// Search visual novels by title/alias. A bare VNDB id (`v17`) looks up that entry.
    pub async fn search(&self, query: &str) -> Result<Vec<VndbSearchResult>, AppError> {
        let q = query.trim();
        if q.is_empty() {
            return Err(AppError::invalid("Enter a title to search on VNDB"));
        }
        let looks_like_id = q.len() > 1 && q.starts_with(['v', 'V']) && q[1..].chars().all(|c| c.is_ascii_digit());
        let filters = match normalize_vndb_id(q) {
            Ok(id) if looks_like_id => json!(["id", "=", id]),
            _ => json!(["search", "=", q]),
        };
        let body = json!({ "filters": filters, "fields": SEARCH_FIELDS, "sort": "searchrank", "results": 20 });
        let body =
            if looks_like_id { json!({ "filters": filters, "fields": SEARCH_FIELDS, "results": 1 }) } else { body };
        parse_search(&self.post_vn(body).await?)
    }

    /// Full entry for one VN id.
    pub async fn fetch_vn(&self, vndb_id: &str) -> Result<RawVn, AppError> {
        let id = normalize_vndb_id(vndb_id)?;
        let body = json!({ "filters": ["id", "=", id], "fields": DETAIL_FIELDS, "results": 1 });
        parse_vn(&self.post_vn(body).await?)?
            .ok_or_else(|| AppError::not_found(format!("VNDB has no visual novel with id {id}")))
    }

    /// Download an image (cover). Validates size and image magic bytes.
    pub async fn download_image(&self, url: &str) -> Result<Vec<u8>, AppError> {
        let resp = self.http.get(url).send().await.map_err(network_error)?;
        if !resp.status().is_success() {
            return Err(AppError::new(ErrorCode::Network, "Cover download failed")
                .with_details(format!("HTTP {} for {url}", resp.status())));
        }
        if resp.content_length().is_some_and(|l| l as usize > MAX_COVER_BYTES) {
            return Err(AppError::new(ErrorCode::Network, "Cover image is too large"));
        }
        let bytes = resp.bytes().await.map_err(network_error)?;
        if bytes.len() > MAX_COVER_BYTES {
            return Err(AppError::new(ErrorCode::Network, "Cover image is too large"));
        }
        if image_extension(&bytes).is_none() {
            return Err(AppError::new(ErrorCode::Network, "Cover download did not return an image")
                .with_details(url.to_string()));
        }
        Ok(bytes.to_vec())
    }
}

fn network_error(e: reqwest::Error) -> AppError {
    let message = if e.is_timeout() {
        "VNDB did not respond in time. Check your internet connection and try again."
    } else if e.is_connect() {
        "Cannot reach VNDB. You may be offline."
    } else if e.is_decode() || e.is_body() {
        "The connection to VNDB was interrupted"
    } else {
        "Network error while contacting VNDB"
    };
    AppError::new(ErrorCode::Network, message).with_details(e.to_string())
}

/// File extension from image magic bytes.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && (&bytes[8..12] == b"avif" || &bytes[8..12] == b"avis") {
        Some("avif")
    } else {
        None
    }
}

/// Write via a temp file in the same directory + rename, so readers never see
/// a partially written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| AppError::internal("cover path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        std::process::id(),
        vnl_core::now_ms()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(Into::into)
}

fn cover_file_name(vndb_id: &str, image: Option<&RawImage>, ext: &str) -> String {
    let image_part = image
        .and_then(|i| i.id.clone().or_else(|| i.url.as_ref().and_then(|u| u.rsplit('/').next().map(str::to_string))))
        .map(|s| s.split('.').next().unwrap_or("").to_string())
        .unwrap_or_default();
    let safe: String = format!("{vndb_id}-{image_part}")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!("{}.{ext}", safe.trim_end_matches('-'))
}

// ------------------------------------------------------- Library integration

fn ensure_enabled(lib: &Library) -> Result<(), AppError> {
    if lib.get_app_settings()?.vndb.enabled {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::Unsupported, "VNDB lookups are disabled in Settings")
            .with_details("enable \"VNDB metadata\" (AppSettings.vndb.enabled) to allow requests to api.vndb.org"))
    }
}

impl Library {
    pub async fn vndb_search(&self, query: &str) -> Result<Vec<VndbSearchResult>, AppError> {
        ensure_enabled(self)?;
        self.vndb.search(query).await
    }

    /// Fetch metadata + cover for `vndb_id` and store them on the game. The
    /// game row is only written after everything was fetched; a failed cover
    /// download keeps the previous cover and still applies the metadata.
    pub async fn vndb_apply(&self, game_id: GameId, vndb_id: &str) -> Result<Game, AppError> {
        ensure_enabled(self)?;
        let id = normalize_vndb_id(vndb_id)?;
        let previous = self.get_game(game_id)?;
        let vn = self.vndb.fetch_vn(&id).await?;
        let mut metadata = to_metadata(&vn);

        if let Some(url) = vn.image.as_ref().and_then(|i| i.url.clone()) {
            match self.vndb.download_image(&url).await {
                Ok(bytes) => {
                    let ext = image_extension(&bytes).unwrap_or("jpg");
                    let path = self.paths.covers_dir().join(cover_file_name(&vn.id, vn.image.as_ref(), ext));
                    write_atomic(&path, &bytes)?;
                    metadata.cover_path = Some(path);
                }
                Err(e) => {
                    tracing::warn!(game_id, error = %e, details = ?e.details, "cover download failed");
                    if previous.metadata.vndb_id.as_deref() == Some(id.as_str()) {
                        metadata.cover_path = previous.metadata.cover_path.clone().filter(|p: &PathBuf| p.is_file());
                    }
                }
            }
        }

        let original = original_title(&vn);
        let title = Some(vn.title.trim()).filter(|t| !t.is_empty());
        self.store_metadata(game_id, title, original.as_deref(), &metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = include_str!("../tests/fixtures/vndb_search_ever17.json");
    const V17: &str = include_str!("../tests/fixtures/vndb_vn_v17.json");

    #[test]
    fn parses_search_fixture() {
        let results = parse_search(SEARCH).unwrap();
        assert_eq!(results.len(), 3);
        let v17 = results.iter().find(|r| r.id == "v17").unwrap();
        assert_eq!(v17.title, "Ever17 -the out of infinity-");
        assert_eq!(v17.released.as_deref(), Some("2002-08-29"));
        assert_eq!(v17.developers, vec!["KID", "SDR Project"]);
        assert!(v17.image_url.as_deref().unwrap().starts_with("https://t.vndb.org/cv/"));
        assert_eq!(v17.image_sexual, Some(0.0));
        assert!(v17.rating.unwrap() > 50.0);
        assert_eq!(v17.alt_title, None);
    }

    #[test]
    fn maps_v17_fixture_to_metadata() {
        let vn = parse_vn(V17).unwrap().unwrap();
        let m = to_metadata(&vn);
        assert_eq!(m.vndb_id.as_deref(), Some("v17"));
        assert_eq!(m.developers, vec!["KID", "SDR Project"]);
        assert_eq!(m.release_date.as_deref(), Some("2002-08-29"));
        assert_eq!(m.length_minutes, Some(1984));
        assert!(m.aliases.contains(&"E17".to_string()));
        assert!(m.aliases.contains(&"时空轮回".to_string()));
        assert!(!m.tags.is_empty() && m.tags.len() <= MAX_TAGS);
        // spoiler / ero / lie tags are dropped
        for bad in ["Time Jump", "Reunion", "Off Screen Sex Only", "A Lie Tag", "One True End"] {
            assert!(!m.tags.contains(&bad.to_string()), "{bad} should be filtered: {:?}", m.tags);
        }
        assert_eq!(m.tags[0], "Confinement");
        let d = m.description.unwrap();
        assert!(d.starts_with("Ever17 is the tale"));
        assert!(!d.contains("[url") && !d.contains("[/url]") && !d.contains("[b]"));
        assert!(!d.contains("third viewpoint"), "spoiler must be removed");
        assert!(d.contains("Visit Tsugumi and enjoy."));
        assert!(d.ends_with("[From Wikipedia]"));
        // Main title equals the display title: no separate original title.
        assert_eq!(original_title(&vn), None);
    }

    #[test]
    fn bbcode_edge_cases() {
        assert_eq!(clean_bbcode("a [SPOILER]x[/Spoiler] b"), "a  b");
        assert_eq!(clean_bbcode("unclosed [spoiler] secret"), "unclosed");
        assert_eq!(clean_bbcode("x [y"), "x [y");
        assert_eq!(clean_bbcode("[url=https://a]A[/url]\n\n\n\nB"), "A\n\nB");
        assert_eq!(clean_bbcode("「日本語」[i]テスト[/i]"), "「日本語」テスト");
    }

    #[test]
    fn original_title_from_titles_or_alttitle() {
        let vn = RawVn {
            id: "v1".into(),
            title: "Natsuiro".into(),
            titles: vec![RawTitle { lang: Some("ja".into()), title: "夏色".into(), latin: None, main: true }],
            ..Default::default()
        };
        assert_eq!(original_title(&vn).as_deref(), Some("夏色"));
        let vn = RawVn { alttitle: Some("ナツイロ".into()), ..vn };
        assert_eq!(original_title(&vn).as_deref(), Some("ナツイロ"));
    }

    #[test]
    fn length_fallback_and_dates() {
        let vn = RawVn { id: "v2".into(), length: Some(3), released: Some("TBA".into()), ..Default::default() };
        let m = to_metadata(&vn);
        assert_eq!(m.length_minutes, Some(1200));
        assert_eq!(m.release_date, None);
    }

    #[test]
    fn vndb_ids() {
        assert_eq!(normalize_vndb_id("v17").unwrap(), "v17");
        assert_eq!(normalize_vndb_id(" V017 ").unwrap(), "v17");
        assert_eq!(normalize_vndb_id("17").unwrap(), "v17");
        assert_eq!(normalize_vndb_id("https://vndb.org/v17/").unwrap(), "v17");
        assert!(normalize_vndb_id("p17").is_err());
        assert!(normalize_vndb_id("v").is_err());
        assert!(normalize_vndb_id("v0").is_err());
    }

    #[test]
    fn image_magic() {
        assert_eq!(image_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_extension(b"<html>"), None);
        assert_eq!(
            cover_file_name("v17", Some(&RawImage { id: Some("cv79412".into()), ..Default::default() }), "jpg"),
            "v17-cv79412.jpg"
        );
        assert_eq!(
            cover_file_name(
                "v17",
                Some(&RawImage { url: Some("https://t/cv/12/79412.jpg".into()), ..Default::default() }),
                "jpg"
            ),
            "v17-79412.jpg"
        );
    }
}
