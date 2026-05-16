use crate::app_state::DbState;
use crate::commands::vault::media::get_media_dir;
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use std::net::SocketAddr;
use tauri::Manager;
use warp::Filter;
use warp::http::{Response, StatusCode};
use warp::hyper::Body;

pub fn start_media_server(app: tauri::AppHandle) {
    let app_handle = app.clone();
    let app_handle2 = app.clone();

    tauri::async_runtime::spawn(async move {
        let media_route = warp::path!("media" / String)
            .and(warp::header::optional::<String>("range"))
            .and(warp::query::<std::collections::HashMap<String, String>>())
            .and_then(move |id, range, query| {
                let app = app_handle.clone();
                async move { handle_media_request(app, id, range, query).await }
            });

        let local_route = warp::path!("local")
            .and(warp::query::<std::collections::HashMap<String, String>>())
            .and(warp::header::optional::<String>("range"))
            .and_then(
                |params: std::collections::HashMap<String, String>,
                 range: Option<String>| async move {
                    let path = params.get("path").cloned().unwrap_or_default();
                    let path = percent_decode(&path);
                    handle_local_file_request(path, range).await
                },
            );

        let routes = media_route.or(local_route);

        // Bind to port 0 to let the OS assign any available port
        let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
        let (addr, server) = warp::serve(routes).bind_ephemeral(addr);

        if let Ok(mut port_lock) = app_handle2.state::<DbState>().media_proxy_port.lock() {
            *port_lock = Some(addr.port());
        }

        server.await;
    });
}

async fn handle_local_file_request(
    path: String,
    range: Option<String>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let path_buf = std::path::PathBuf::from(&path);
    let canonical = std::fs::canonicalize(&path_buf).map_err(|_| warp::reject())?;

    let home = std::env::var("HOME").unwrap_or_default();
    let allowed = if !home.is_empty() {
        let home_path = std::path::PathBuf::from(&home);
        let home_canonical = std::fs::canonicalize(&home_path).unwrap_or(home_path);
        canonical.starts_with(&home_canonical)
    } else {
        canonical.starts_with("/home/")
            || canonical.starts_with("/Users/")
            || canonical.starts_with("/tmp/")
    };
    if !allowed {
        return Err(warp::reject());
    }

    let total_size = tokio::fs::metadata(&canonical)
        .await
        .map_err(|_| warp::reject())?
        .len();

    let (start, end) = if let Some(r) = range {
        if let Some(stripped) = r.strip_prefix("bytes=") {
            let parts: Vec<&str> = stripped.split('-').collect();
            let s = parts[0].parse::<u64>().unwrap_or(0);
            let e = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse::<u64>().unwrap_or(total_size - 1)
            } else {
                total_size - 1
            };
            (s, e)
        } else {
            (0, total_size - 1)
        }
    } else {
        (0, total_size - 1)
    };

    let content_length = end - start + 1;
    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let content_type = mime_from_ext(&ext);

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(16);

    tauri::async_runtime::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        if let Ok(mut file) = tokio::fs::File::open(&canonical).await {
            let _ = file.seek(std::io::SeekFrom::Start(start)).await;
            let mut remaining = content_length as usize;
            let mut buf = vec![0u8; 65536];
            while remaining > 0 {
                let to_read = remaining.min(buf.len());
                match file.read(&mut buf[..to_read]).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx
                            .send(Ok(bytes::Bytes::copy_from_slice(&buf[..n])))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        remaining -= n;
                    }
                    Err(_) => break,
                }
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let status = if start > 0 || end < total_size - 1 {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut resp = Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .header("Content-Length", content_length);
    if start > 0 || end < total_size - 1 {
        resp = resp.header(
            "Content-Range",
            format!("bytes {}-{}/{}", start, end, total_size),
        );
    }
    Ok(resp.body(Body::wrap_stream(stream)).unwrap())
}

async fn handle_media_request(
    app: tauri::AppHandle,
    id: String,
    range: Option<String>,
    query: std::collections::HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mime_type = query.get("type").cloned().unwrap_or_default();
    let mime_type = percent_decode(&mime_type);
    let mime_type = if mime_type.is_empty() {
        mime_from_ext(
            std::path::Path::new(&id)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(""),
        )
    } else {
        &mime_type
    };

    let state = app.state::<DbState>();

    // 1. Get the media key
    let key_bytes = {
        let lock = state.media_key.lock().map_err(|_| warp::reject())?;
        lock.clone().ok_or_else(warp::reject)?
    };
    let key = Key::from_slice(&key_bytes);

    // 2. Locate the file with path traversal protection
    let media_dir = get_media_dir(&app, &state).map_err(|_| warp::reject())?;
    let media_dir_canonical = std::fs::canonicalize(&media_dir).map_err(|_| warp::reject())?;
    let file_path = media_dir.join(&id);
    let file_canonical = std::fs::canonicalize(&file_path).map_err(|_| warp::reject())?;
    if !file_canonical.starts_with(&media_dir_canonical) {
        return Err(warp::reject());
    }

    let mut file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|_| warp::reject())?;
    let metadata = file.metadata().await.map_err(|_| warp::reject())?;
    let total_vault_size = metadata.len();

    // Each block is 8,388,632 bytes (24B Nonce + 8,388,608B Data + 16B Tag)
    const BLOCK_SIZE_ENC: u64 = 8_388_648;
    const BLOCK_SIZE_PLAIN: u64 = 8_388_608;
    let num_blocks = total_vault_size / BLOCK_SIZE_ENC;
    let last_block_rem = total_vault_size % BLOCK_SIZE_ENC;

    // Total plaintext size
    let mut total_plain_size = num_blocks * BLOCK_SIZE_PLAIN;
    if last_block_rem > 40 {
        total_plain_size += last_block_rem - 40;
    }

    // 3. Handle Range Header
    let (start, end) = if let Some(r) = range.as_ref() {
        if let Some(stripped) = r.strip_prefix("bytes=") {
            let parts: Vec<&str> = stripped.split('-').collect();
            let start = parts[0].parse::<u64>().unwrap_or(0);
            let end = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse::<u64>().unwrap_or(total_plain_size - 1)
            } else {
                total_plain_size - 1
            };
            (start, end)
        } else {
            (0, total_plain_size - 1)
        }
    } else {
        (0, total_plain_size - 1)
    };

    if start >= total_plain_size {
        return Ok(Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .body(Body::from("Range Not Satisfiable"))
            .unwrap());
    }

    let content_length = end - start + 1;
    let cipher = XChaCha20Poly1305::new(key);

    // 4. Create the streaming body with small buffer to limit in-flight RAM
    let (tx, rx) = tokio::sync::mpsc::channel(4);

    tauri::async_runtime::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let mut current_offset = start;
        let mut first_run = true;

        while current_offset <= end {
            let block_index = current_offset / BLOCK_SIZE_PLAIN;
            let vault_pos = block_index * BLOCK_SIZE_ENC;
            let offset_in_block = (current_offset % BLOCK_SIZE_PLAIN) as usize;

            if first_run {
                if file
                    .seek(std::io::SeekFrom::Start(vault_pos))
                    .await
                    .is_err()
                {
                    break;
                }
                first_run = false;
            }

            let remaining_file = total_vault_size.saturating_sub(vault_pos);
            if remaining_file == 0 {
                break;
            }

            let to_read = if remaining_file >= BLOCK_SIZE_ENC as u64 {
                BLOCK_SIZE_ENC as usize
            } else {
                remaining_file as usize
            };

            let mut block_data = Vec::with_capacity(to_read);
            let mut read_buf = vec![0u8; 65536];
            while block_data.len() < to_read {
                let want = std::cmp::min(read_buf.len(), to_read - block_data.len());
                let n = file.read(&mut read_buf[..want]).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                block_data.extend_from_slice(&read_buf[..n]);
            }
            if block_data.len() < 40 {
                break;
            }

            let nonce = XNonce::from_slice(&block_data[..24]);
            let ciphertext = &block_data[24..];

            if let Ok(ptext) = cipher.decrypt(nonce, ciphertext) {
                if offset_in_block < ptext.len() {
                    let to_send = &ptext[offset_in_block..];
                    let remaining_in_range = (end - current_offset + 1) as usize;
                    let actual_send = std::cmp::min(to_send.len(), remaining_in_range);

                    if tx
                        .send(Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(
                            &to_send[..actual_send],
                        )))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    current_offset += actual_send as u64;
                } else {
                    // Safety break to prevent infinite loops if logic desyncs
                    break;
                }
            } else {
                // Decryption failed - could be a corrupted block.
                // We break to avoid sending garbage to the browser.
                break;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let is_range = range.is_some();
    let mut response = Response::builder()
        .status(if is_range {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header("Content-Type", mime_type.to_string())
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .header(
            "Access-Control-Expose-Headers",
            "Content-Range, Content-Length, Accept-Ranges",
        );

    if is_range {
        response = response.header(
            "Content-Range",
            format!("bytes {}-{}/{}", start, end, total_plain_size),
        );
    }

    response = response.header("Content-Length", content_length);

    Ok(response.body(Body::wrap_stream(stream)).unwrap())
}

fn mime_from_ext(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "ogg" => "video/ogg",
        "mkv" => "video/x-matroska",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte as char);
            i += 3;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
