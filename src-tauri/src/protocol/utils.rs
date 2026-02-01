use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

pub fn decode_b64(s: &str) -> Result<Vec<u8>, String> {
    BASE64.decode(s).map_err(|e| e.to_string())
}

pub fn encode_b64(b: &[u8]) -> String {
    BASE64.encode(b)
}
