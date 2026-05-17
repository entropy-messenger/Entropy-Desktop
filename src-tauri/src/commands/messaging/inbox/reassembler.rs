use serde_json::json;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

use crate::app_state::{FragmentAssembly, NetworkState};

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
    if header.frame_type == 0x02 {
        return Ok((false, None));
    }

    if header.total == 0 || header.index >= header.total {
        return Err("Invalid fragment header".into());
    }

    let transfer_key = format!("{}:{}:{}", sender, header.transfer_id, header.frame_type);

    let (is_complete, complete_data) = {
        let mut assemblers = net_state
            .fragment_assembler
            .lock()
            .map_err(|_| "Network state poisoned")?;

        let entry = assemblers
            .entry(transfer_key.clone())
            .or_insert_with(|| FragmentAssembly::new(header.total));

        // Track maximum chunks to prevent OOM
        if entry.chunks.len() >= 250_000 {
            assemblers.remove(&transfer_key);
            return Err("Fragment count exceeds limit".into());
        }

        if !entry.chunks.contains_key(&header.index) {
            entry.chunks.insert(header.index, chunk_data.to_vec());
        }
        entry.last_activity = Instant::now();

        let received = entry.chunks.len() as u32;
        let complete = received == entry.total;

        let progress_step = (header.total / 20).max(1);
        if header.index % progress_step == 0 || complete {
            let _ = app.emit(
                "transfer://progress",
                json!({
                    "transfer_id": header.transfer_id,
                    "sender": sender,
                    "current": received,
                    "total": header.total,
                    "direction": "download"
                }),
            );
        }

        if complete {
            let total_size: usize = entry.chunks.values().map(|v| v.len()).sum();
            let mut data = Vec::with_capacity(total_size);
            for i in 0..entry.total {
                if let Some(c) = entry.chunks.remove(&i) {
                    data.extend(c);
                }
            }
            assemblers.remove(&transfer_key);
            (true, Some(data))
        } else {
            (false, None)
        }
    };

    Ok((is_complete, complete_data))
}
