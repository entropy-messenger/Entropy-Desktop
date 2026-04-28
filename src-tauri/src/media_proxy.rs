use tauri::Manager;
use warp::Filter;
use std::net::SocketAddr;
use std::io::{Read, Seek, SeekFrom};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, Key, XNonce,
};
use crate::app_state::DbState;
use crate::commands::vault::media::get_media_dir;
use warp::http::{HeaderValue, StatusCode, Response};
use warp::hyper::Body;

pub fn start_media_server(app: tauri::AppHandle) {
    let app_handle = app.clone();
    
    tauri::async_runtime::spawn(async move {
        let media_route = warp::path!("media" / String)
            .and(warp::header::optional::<String>("range"))
            .and_then(move |id, range| {
                let app = app_handle.clone();
                async move {
                    handle_media_request(app, id, range).await
                }
            });

        // Use a fixed port for now, or we could pass it to the UI
        let addr: SocketAddr = ([127, 0, 0, 1], 51761).into();
        println!("[MEDIA-PROXY] Server listening on http://{}", addr);
        warp::serve(media_route).run(addr).await;
    });
}

async fn handle_media_request(
    app: tauri::AppHandle,
    id: String,
    range: Option<String>
) -> Result<impl warp::Reply, warp::Rejection> {
    let state = app.state::<DbState>();
    
    // 1. Get the media key
    let key_bytes = {
        let lock = state.media_key.lock().map_err(|_| warp::reject())?;
        lock.clone().ok_or_else(|| warp::reject())?
    };
    let key = Key::from_slice(&key_bytes);
    
    // 2. Locate the file
    let media_dir = get_media_dir(&app, &state).map_err(|_| warp::reject())?;
    let file_path = media_dir.join(&id);
    
    if !file_path.exists() {
        return Err(warp::reject());
    }

    let mut file = std::fs::File::open(&file_path).map_err(|_| warp::reject())?;
    let total_vault_size = file.metadata().map_err(|_| warp::reject())?.len();
    
    // Each block is 1319 bytes (24B Nonce + 1279B Data + 16B Tag)
    let num_blocks = total_vault_size / 1319;
    let last_block_rem = total_vault_size % 1319;
    
    // Total plaintext size
    let mut total_plain_size = num_blocks * 1279;
    if last_block_rem > 40 {
        total_plain_size += last_block_rem - 40;
    }

    // 3. Handle Range Header
    let (start, end) = if let Some(r) = range {
        if r.starts_with("bytes=") {
            let parts: Vec<&str> = r["bytes=".len()..].split('-').collect();
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

    // 4. Create the streaming body
    let (mut tx, rx) = tokio::sync::mpsc::channel(10);
    
    tauri::async_runtime::spawn(async move {
        let mut current_offset = start;
        let mut buffer = vec![0u8; 1319];
        
        while current_offset <= end {
            let block_index = current_offset / 1279;
            let offset_in_block = (current_offset % 1279) as usize;
            
            let vault_pos = block_index * 1319;
            if let Err(_) = file.seek(SeekFrom::Start(vault_pos)) { break; }
            
            let n = match file.read(&mut buffer) {
                Ok(n) if n > 40 => n,
                _ => break,
            };

            let nonce = XNonce::from_slice(&buffer[..24]);
            let ciphertext = &buffer[24..n];
            
            if let Ok(ptext) = cipher.decrypt(nonce, ciphertext) {
                let to_send = &ptext[offset_in_block..];
                let remaining_in_range = (end - current_offset + 1) as usize;
                let actual_send = std::cmp::min(to_send.len(), remaining_in_range);
                
                if tx.send(Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&to_send[..actual_send]))).await.is_err() {
                    break;
                }
                current_offset += actual_send as u64;
            } else {
                break;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    
    let mut response = Response::builder()
        .status(if start > 0 || end < total_plain_size - 1 { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK })
        .header("Content-Type", "video/mp4") // TODO: Detect from metadata if possible
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*");

    if start > 0 || end < total_plain_size - 1 {
        response = response.header("Content-Range", format!("bytes {}-{}/{}", start, end, total_plain_size));
    }
    
    response = response.header("Content-Length", content_length);

    Ok(response.body(Body::wrap_stream(stream)).unwrap())
}
