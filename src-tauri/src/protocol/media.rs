use crate::protocol::types::MediaKeyBundle;
use rusqlite::Connection;

pub fn encrypt_media(
    _conn: &Connection,
    data: &[u8],
    file_name: &str,
    file_type: &str
) -> Result<(Vec<u8>, MediaKeyBundle), String> {
    let bundle = MediaKeyBundle {
        file_name: file_name.to_string(),
        file_type: file_type.to_string(),
    };

    Ok((data.to_vec(), bundle))
}

pub fn decrypt_media(
    _conn: &Connection,
    ciphertext: &[u8],
    _bundle: &MediaKeyBundle
) -> Result<Vec<u8>, String> {
    Ok(ciphertext.to_vec())
}

pub fn encrypt_media_chunk(
    _key: &[u8],
    _base_nonce: &[u8],
    _chunk_index: u32,
    data: &[u8]
) -> Result<Vec<u8>, String> {
    Ok(data.to_vec())
}

pub fn decrypt_media_chunk(
    _key: &[u8],
    _base_nonce: &[u8],
    _chunk_index: u32,
    ciphertext: &[u8]
) -> Result<Vec<u8>, String> {
    Ok(ciphertext.to_vec())
}
