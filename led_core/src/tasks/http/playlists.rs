//! Named playlist library: `GET|POST|DELETE /api/playlists`.
//!
//! Playlists live in the `playlists/` directory of the platform file store
//! (littlefs on flash, `tmp/host_board/playlists/` on the sim), one JSON
//! document per playlist holding `[{animation, duration_ms, label?}]`.
//! `?name=` addresses one; without it GET lists the whole library and POST
//! creates/overwrites. The library is pure storage — putting a playlist on
//! the panel stays a WebSocket `Playlist` message (with `save`), so the
//! "what is live right now" story keeps exactly one source (the display
//! worker), and an upload can never interrupt the show.
//!
//! Registered as a single `MethodHandlerService`: the wifi image sits ~20 KB
//! from the 508 K slot ceiling, and three `*_service` registrations would
//! monomorphise this whole (serde-heavy) handler once per method.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use animations::PlaylistItem;
use core::str::from_utf8;
use embedded_tools::local_fs::{DirEntry, FileType, FsError, LocalFsTrait};
use embedded_tools::{Debug2Format, info, warn};
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter, StatusCode},
    routing::MethodHandlerService,
};

/// Bodies are parsed whole (they are small), so cap the damage a stray POST
/// can do to the heap.
const MAX_PLAYLIST_BYTES: usize = 64 * 1024;

/// Directory (relative to the file-store root) and extension of the library.
const LIBRARY_DIR: &str = "playlists";
const EXT: &str = ".json";

pub struct PlaylistLibraryHandler<FS: LocalFsTrait + Clone> {
    local_fs: FS,
}

impl<FS: LocalFsTrait + Clone> PlaylistLibraryHandler<FS> {
    pub fn new(local_fs: FS) -> Self {
        Self { local_fs }
    }
}

impl<FS: LocalFsTrait + Clone> MethodHandlerService<()> for PlaylistLibraryHandler<FS> {
    async fn call_method_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        (): (),
        method: &str,
        mut req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let query = req.parts.query().map(|q| q.0).unwrap_or("");
        let name = query_param(query, "name").and_then(sanitize_name);

        match (method, name) {
            ("GET", None) => {
                let entries = match self.local_fs.list_dir(LIBRARY_DIR).await {
                    Ok(entries) => entries,
                    // No directory yet = empty library, not an error.
                    Err(_) => Vec::new(),
                };

                let json = library_listing(&entries);

                json_response!(req, writer, &json)
            }
            ("GET", Some(name)) => match self.local_fs.read_text_file(&path(&name)).await {
                Ok(json) => json_response!(req, writer, &json),
                Err(_) => (StatusCode::NOT_FOUND, format!("No playlist named {name}\n"))
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await,
            },
            ("POST", Some(name)) => {
                let file_size = req.body_connection.body().content_length();

                if file_size == 0 || file_size as usize > MAX_PLAYLIST_BYTES {
                    return format_response!(req, writer, "Playlist body must be 1..{} bytes\n", MAX_PLAYLIST_BYTES);
                }

                let buffer = read_request_to_buffer!(req, writer);

                let body = match from_utf8(&buffer) {
                    Ok(body) => body,
                    Err(_) => {
                        return (StatusCode::BAD_REQUEST, "Body is not UTF-8 JSON")
                            .write_to(req.body_connection.finalize().await?, writer)
                            .await
                    }
                };

                // Validate before touching the store — the UI gets a clean
                // 400 on a malformed playlist instead of a corrupt file.
                let items: Vec<PlaylistItem> = match serde_json::from_str(body) {
                    Ok(items) => items,
                    Err(err) => {
                        warn!("Playlist {} rejected: {}", name.as_str(), Debug2Format(&err));
                        return format_response!(req, writer, "Invalid playlist JSON: {:?}\n", Debug2Format(&err));
                    }
                };

                if items.is_empty() {
                    return (StatusCode::BAD_REQUEST, "Empty playlist — delete it instead")
                        .write_to(req.body_connection.finalize().await?, writer)
                        .await;
                }

                // Re-serialise through the typed model: normalises tuple-form
                // uploads and clamps durations to the playlist bounds.
                let json = serde_json::to_string(&items).unwrap_or_else(|_| body.into());

                if let Err(err) = self.local_fs.mkdir(LIBRARY_DIR).await
                    && !matches!(err, FsError::AlreadyExists)
                {
                    return format_response!(req, writer, "Could not create library dir: {:?}\n", Debug2Format(&err));
                }

                match self.local_fs.write_text_file(&path(&name), json).await {
                    Ok(()) => {
                        info!("Playlist {} saved ({} items)", name.as_str(), items.len());
                        format_response!(req, writer, "Saved {name}\n")
                    }
                    Err(err) => format_response!(req, writer, "Could not write playlist: {:?}\n", Debug2Format(&err)),
                }
            }
            ("DELETE", Some(name)) => match self.local_fs.delete(&path(&name)).await {
                Ok(()) => {
                    info!("Playlist {} deleted", name.as_str());
                    format_response!(req, writer, "Deleted {name}\n")
                }
                Err(_) => (StatusCode::NOT_FOUND, format!("No playlist named {name}\n"))
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await,
            },
            (_, None) => (StatusCode::BAD_REQUEST, "Missing or invalid ?name= (POST/DELETE)")
                .write_to(req.body_connection.finalize().await?, writer)
                .await,
            _ => (StatusCode::METHOD_NOT_ALLOWED, "GET, POST or DELETE only")
                .write_to(req.body_connection.finalize().await?, writer)
                .await,
        }
    }
}

/// `[{"name":"…","size":n},…]` for GET without a name; only the library's
/// own files. Built with `format!` — a `serde_json::Value` pipeline for this
/// one line is several KB of code we cannot afford.
fn library_listing(entries: &[DirEntry]) -> String {
    let mut out = String::from("[");
    let mut first = true;

    for entry in entries {
        if entry.file_type != FileType::File || !entry.name.ends_with(EXT) {
            continue;
        }

        let name = &entry.name[..entry.name.len() - EXT.len()];

        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_')
        {
            continue; // foreign file — not ours, not JSON-escapable-safe
        }

        if first {
            first = false;
        } else {
            out.push(',');
        }

        out.push_str(&format!("{{\"name\":\"{name}\",\"size\":{}}}", entry.size));
    }

    out.push(']');
    out
}

fn path(name: &str) -> String {
    format!("{LIBRARY_DIR}/{name}{EXT}")
}

/// Library names are filesystem names: keep them boring (the UI never
/// generates anything else) so no escaping is needed anywhere downstream.
fn sanitize_name(raw: &str) -> Option<String> {
    let name = percent_decode(raw);

    if name.is_empty() || name.len() > 40 {
        return None;
    }

    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_') {
        return None;
    }

    if name.starts_with('.') || name.contains("..") {
        return None;
    }

    Some(name)
}

/// Minimal percent-decoding for a single query value (`+` stays a literal —
/// the UI encodes spaces as `%20`).
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }

        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(out).unwrap_or_default()
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;

        if k == key { Some(v) } else { None }
    })
}
