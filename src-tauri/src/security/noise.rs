use serde_json::Value;

pub struct TrafficNormalizer;

impl TrafficNormalizer {
    pub const FIXED_FRAME_SIZE: usize = 1400;

    pub fn pad_json(val: &mut Value) {
        let current_str = serde_json::to_string(val).unwrap();
        let current_len = current_str.len();
        
        if current_len + 15 > Self::FIXED_FRAME_SIZE {
            return;
        }

        let padding_size = Self::FIXED_FRAME_SIZE - current_len - 15;
        let padding = " ".repeat(padding_size);
        
        if let Some(obj) = val.as_object_mut() {
            obj.insert("padding".to_string(), Value::String(padding));
        }
    }

    pub fn pad_binary(data: &mut Vec<u8>) {
        let padding_needed = (Self::FIXED_FRAME_SIZE - (data.len() % Self::FIXED_FRAME_SIZE)) % Self::FIXED_FRAME_SIZE;
        if padding_needed > 0 || data.is_empty() {
             let pad_to = if data.is_empty() { Self::FIXED_FRAME_SIZE } else { data.len() + padding_needed };
             data.resize(pad_to, 0);
        }
    }
}
