

pub struct TrafficNormalizer;

impl TrafficNormalizer {

    pub fn pad_json_str(json_str: &mut String, target_size: usize) {
        let current_len = json_str.len();
        if current_len > target_size {
            json_str.truncate(target_size);
        } else if current_len < target_size {
            let padding_needed = target_size - current_len;
            json_str.push_str(&" ".repeat(padding_needed));
        }
    }

    pub fn pad_binary(data: &mut Vec<u8>, target_size: usize) {
        data.resize(target_size, 0);
    }
}
