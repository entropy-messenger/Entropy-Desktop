use hex;
use num_bigint::BigUint;
use serde_json::json;

#[tauri::command]
pub async fn crypto_mine_pow(
    seed: String,
    d: u32,
    i: String,
    modulus: String,
) -> Result<serde_json::Value, String> {
    Ok(internal_mine_pow(seed, d, i, Some(modulus)).await)
}

pub async fn internal_mine_pow(
    seed: String,
    difficulty: u32,
    _context: String,
    modulus: Option<String>,
) -> serde_json::Value {
    let n_str = modulus.as_ref().map(|s| s.to_string()).unwrap_or_else(|| "16924353219721975706619304977087776638210692887418153614822570947993460098757637997153620390534205323940422136903855515357288961635893026503845398062994157546242993897432842505612884614045940034466012450686593767189610225378750810792439341873585245840091628083670434049768166724299902688993164080731321559365156036266700853190146043193271501897793442680973988812797807962731521024848426255262545103363066538288771520973709300521207461949980255896180578618344539304776270176040513674389484251916722619230508579403099751552290930600171147372478499901544032334923289379116695422056004175570276337468297686269307727794059".to_string());

    let seed_clone = seed.clone();
    // Offload CPU-heavy VDF calculation to a thread pool to prevent async runtime starvation/UI lag
    let result_hex = tauri::async_runtime::spawn_blocking(move || {
        let n = BigUint::parse_bytes(n_str.as_bytes(), 10).expect("Valid modulus");
        let x_bytes = hex::decode(&seed_clone).unwrap_or_default();
        let mut x = BigUint::from_bytes_be(&x_bytes) % &n;

        // Exact 1:1 VDF logic (y = x^(2^T) mod N)
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
        "id_hash": _context,
        "modulus": modulus.unwrap_or_default(),
        "difficulty": difficulty
    })
}
