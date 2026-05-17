//! Proof-of-Work: Verifiable Delay Function (VDF) Implementation
//!
//! Entropy utilize a VDF-based Proof-of-Work to impose an intentional computational cost
//! on message dispatch, mitigating network saturation and spam.
//!
//! Specification:
//! - Primitive: Sequential Squaring mod N.
//! - Algorithm: y = x^(2^T) mod N, where T is the difficulty and N is the modulus.
//! - Properties: Computation is non-parallelizable, ensuring a reliable time delay
//!   proportional to hardware clock speeds. Verification is $O(1)$ given the solution.

use hex;
use num_bigint::BigUint;
use serde_json::json;

pub async fn internal_mine_pow(
    seed: String,
    difficulty: u32,
    modulus: Option<String>,
) -> serde_json::Value {
    let n_str = match modulus.as_ref() {
        Some(s) => s.clone(),
        None => {
            return json!({
                "seed": seed,
                "error": "No PoW modulus configured",
            })
        }
    };

    let n_str_block = n_str.clone();
    let seed_clone = seed.clone();
    let result_hex = tauri::async_runtime::spawn_blocking(move || {
        let n = BigUint::parse_bytes(n_str_block.as_bytes(), 10).expect("Valid modulus");
        let x_bytes = hex::decode(&seed_clone).unwrap_or_default();
        let mut x = BigUint::from_bytes_be(&x_bytes) % &n;

        for _ in 0..difficulty {
            x = (&x * &x) % &n;
        }
        hex::encode(x.to_bytes_be())
    })
    .await
    .unwrap_or_default();

    json!({
        "seed": seed,
        "nonce": result_hex,
        "modulus": n_str,
        "difficulty": difficulty
    })
}
