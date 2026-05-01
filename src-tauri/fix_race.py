import sys
import re

def process_app_state():
    with open('src/app_state.rs', 'r') as f:
        content = f.read()
    
    if 'pub disk_flush_rx: Option<tokio::sync::oneshot::Receiver<()>>' not in content:
        content = content.replace(
            'pub disk_tx: Option<tokio::sync::mpsc::UnboundedSender<(u64, Vec<u8>)>>,\n',
            'pub disk_tx: Option<tokio::sync::mpsc::UnboundedSender<(u64, Vec<u8>)>>,\n    pub disk_flush_rx: Option<tokio::sync::oneshot::Receiver<()>>,\n'
        )
        # also fix places where MediaTransferState is created if any inside app_state.rs
        with open('src/app_state.rs', 'w') as f:
            f.write(content)
        print("Updated app_state.rs")

def process_inbox():
    with open('src/commands/messaging/inbox.rs', 'r') as f:
        content = f.read()

    # 1. Update MediaTransferState constructor
    content = content.replace(
        '''last_activity: std::time::Instant::now(), disk_tx: None, received_count: 0,''',
        '''last_activity: std::time::Instant::now(), disk_tx: None, disk_flush_rx: None, received_count: 0,'''
    )

    # 2. Update channel creation
    old_channel = '''let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Vec<u8>)>();
                            assembler.disk_tx = Some(tx);'''
    new_channel = '''let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Vec<u8>)>();
                            let (flush_tx, flush_rx) = tokio::sync::oneshot::channel::<()>();
                            assembler.disk_tx = Some(tx);
                            assembler.disk_flush_rx = Some(flush_rx);'''
    content = content.replace(old_channel, new_channel)

    # 3. Update tokio task
    old_task_end = '''let _ = f.sync_all().await;
                                }
                            });'''
    new_task_end = '''let _ = f.sync_all().await;
                                }
                                let _ = flush_tx.send(());
                            });'''
    content = content.replace(old_task_end, new_task_end)

    # 4. Update complete logic (the messy part)
    # We need to extract disk_flush_rx and file_path to wait for it outside the lock
    
    old_complete_block = '''            if complete {
                println!("[INBOX] Reassembly complete for TID: {}. FrameType: {}", transfer_id, frame_type);
                if frame_type == 0x01 || frame_type == 0x04 {
                    let db_state = app.state::<DbState>();
                    let type_suffix = if frame_type == 0x02 { "media" } else { "sig" };
                    let temp_filename = format!("transfer_{}_{}_{}.bin", sender, transfer_id, type_suffix);
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
                    // For media (0x02), we don't read into RAM here.
                    // The specialized handler will handle the file.
                    assemblers.remove(&transfer_key);
                    (true, None)
                }
            } else {
                (false, None)
            }
        };

        if is_complete {'''
        
    new_complete_block = '''            if complete {
                println!("[INBOX] Reassembly complete for TID: {}. FrameType: {}", transfer_id, frame_type);
                assembler.disk_tx.take(); // Close channel
                let flush_rx = assembler.disk_flush_rx.take();
                assemblers.remove(&transfer_key);
                (true, flush_rx)
            } else {
                (false, None)
            }
        };

        if is_complete {
            // Wait for disk writer to finish flushing to SSD before we read the file
            if let Some(flush_rx) = complete_data {
                let _ = flush_rx.await;
            }

            let db_state = app.state::<crate::app_state::DbState>();
            let type_suffix = if frame_type == 0x02 { "media" } else { "sig" };
            let temp_filename = format!("transfer_{}_{}_{}.bin", sender, transfer_id, type_suffix);
            let media_dir = crate::commands::vault::get_media_dir(&app, &db_state).unwrap_or_default();
            let file_path = media_dir.join(&temp_filename);

            if frame_type == 0x01 || frame_type == 0x04 {
                let complete_data_bytes = std::fs::read(&file_path).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(&file_path);
                
                // Trim trailing zeros from fixed chunk sizing before parsing JSON
                let actual_len = complete_data_bytes.iter().rposition(|&x| x != 0).map(|p| p + 1).unwrap_or(0);
                let clean_data = &complete_data_bytes[..actual_len];

                let complete_data = clean_data;'''
                
    content = content.replace(old_complete_block, new_complete_block)
    
    # 5. Fix the missing matching brackets for complete_data
    # In original:
    #        if is_complete {
    #            if frame_type == 0x01 || frame_type == 0x04 {
    #                let complete_data = complete_data.ok_or("Failed to load reassembled data")?;
    #                let envelope: serde_json::Value = ...
    
    old_if_block = '''            if frame_type == 0x01 || frame_type == 0x04 {
                let complete_data = complete_data.ok_or("Failed to load reassembled data")?;
                let envelope: serde_json::Value = serde_json::from_slice(&complete_data)'''
                
    new_if_block = '''                let envelope: serde_json::Value = serde_json::from_slice(complete_data)'''
    content = content.replace(old_if_block, new_if_block)

    # 6. We also need to fix `complete_data` variable usage mismatch
    # Replace `let (is_complete, complete_data) = {` with `let (is_complete, complete_data) = {` (unchanged)
    # But `complete_data` is now `Option<Receiver<()>>`
    
    with open('src/commands/messaging/inbox.rs', 'w') as f:
        f.write(content)
    print("Updated inbox.rs")

process_app_state()
process_inbox()
