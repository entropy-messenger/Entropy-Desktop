use crate::app_state::{DbState, MediaTransferState, NetworkState};
use serde_json::json;
use std::io::{Seek, SeekFrom, Write};
use tauri::{AppHandle, Emitter, Manager};

pub struct FragmentHeader {
    pub frame_type: u8,
    pub transfer_id: u32,
    pub index: u32,
    pub total: u32,
}

pub async fn internal_process_fragments(
    app: AppHandle,
    net_state: &NetworkState,
    sender: &str,
    header: FragmentHeader,
    chunk_data: &[u8],
) -> Result<(bool, Option<Vec<u8>>), String> {
    let (is_complete, complete_data) = {
        let mut assemblers = net_state
            .media_assembler
            .lock()
            .map_err(|_| "Network state poisoned")?;
        let transfer_key = format!("{}:{}:{}", sender, header.transfer_id, header.frame_type);
        let assembler =
            assemblers
                .entry(transfer_key.clone())
                .or_insert_with(|| MediaTransferState {
                    total: header.total,
                    received_chunks: vec![false; header.total as usize],
                    last_activity: std::time::Instant::now(),
                    file_handle: None,
                    received_count: 0,
                });

        if (header.index as usize) < assembler.received_chunks.len()
            && !assembler.received_chunks[header.index as usize]
        {
            if assembler.file_handle.is_none() {
                let db_state = app.state::<DbState>();
                let media_dir = crate::commands::vault::get_media_dir(&app, &db_state)?;
                let type_suffix = if header.frame_type == 0x02 {
                    "media"
                } else {
                    "sig"
                };
                let temp_filename = format!(
                    "transfer_{}_{}_{}.bin",
                    sender, header.transfer_id, type_suffix
                );
                let file_path = media_dir.join(&temp_filename);
                let f = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&file_path)
                    .map_err(|e| format!("Failed to create reassembly file: {}", e))?;
                assembler.file_handle = Some(f);
            }

            if let Some(ref mut f) = assembler.file_handle {
                let mut retries = 0;
                let max_retries = 3;
                loop {
                    let res = f
                        .seek(SeekFrom::Start(header.index as u64 * 1319))
                        .and_then(|_| f.write_all(chunk_data));

                    match res {
                        Ok(_) => break,
                        Err(_e) if retries < max_retries => {
                            retries += 1;
                            std::thread::sleep(std::time::Duration::from_millis(10 * retries));
                            continue;
                        }
                        Err(e) => {
                            return Err(format!(
                                "Persistent I/O error during fragment reassembly: {}",
                                e
                            ));
                        }
                    }
                }
            } else {
                return Err("Internal Error: Media file handle lost during reassembly".into());
            }

            assembler.received_chunks[header.index as usize] = true;
            assembler.received_count += 1;
            assembler.last_activity = std::time::Instant::now();
        }

        let current_count = assembler.received_count;
        let complete = current_count >= assembler.total;

        let progress_step = (header.total / 20).max(1);
        if header.index.is_multiple_of(progress_step) || complete {
            let _ = app.emit(
                "transfer://progress",
                json!({
                    "transfer_id": header.transfer_id,
                    "sender": sender,
                    "current": current_count,
                    "total": header.total,
                    "direction": "download"
                }),
            );
        }

        if complete {
            if header.frame_type == 0x01 || header.frame_type == 0x04 {
                let db_state = app.state::<DbState>();
                let type_suffix = "sig";
                let temp_filename = format!(
                    "transfer_{}_{}_{}.bin",
                    sender, header.transfer_id, type_suffix
                );
                if let Ok(media_dir) = crate::commands::vault::get_media_dir(&app, &db_state) {
                    let file_path = media_dir.join(&temp_filename);
                    if let Ok(data) = std::fs::read(&file_path) {
                        let _ = std::fs::remove_file(file_path);
                        assemblers.remove(&transfer_key);
                        (true, Some(data))
                    } else {
                        (false, None)
                    }
                } else {
                    (false, None)
                }
            } else {
                assemblers.remove(&transfer_key);
                (true, None)
            }
        } else {
            (false, None)
        }
    };

    Ok((is_complete, complete_data))
}
