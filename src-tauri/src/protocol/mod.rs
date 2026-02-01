pub mod types;
pub mod crypto;
pub mod groups;
pub mod media;
pub mod utils;

pub use types::*;
pub use crypto::*;
pub use groups::*;
pub use media::*;
pub use utils::*;

use rusqlite::{params, Connection};
use std::collections::HashMap;

pub use x25519_dalek::{StaticSecret, PublicKey as X25519PublicKey};
use rand::{RngCore, thread_rng};
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
use hkdf::Hkdf;
use sha2::{Sha256, Digest};
use pqcrypto_kyber::kyber1024;
use pqcrypto_traits::kem::{PublicKey as PQPubKey, SecretKey as PQSecretKey};
use pqcrypto_traits::kem::{Ciphertext, SharedSecret};

pub fn establish_outbound_session(
    conn: &Connection,
    remote_hash: &str,
    bundle: &serde_json::Value
) -> Result<(), String> {
    let identity = ProtocolIdentity::load_from_db(conn)?.ok_or("No identity")?;
    
    let my_id_priv_bytes = decode_b64(&identity.identity_keys.private_key)?;
    let my_id_secret = ed25519_priv_to_x25519(&my_id_priv_bytes)?;

    let mut rng = thread_rng();
    let mut my_ephemeral_bytes = [0u8; 32];
    rng.fill_bytes(&mut my_ephemeral_bytes);
    let my_ephemeral_secret = StaticSecret::from(my_ephemeral_bytes);
    let my_ephemeral_public = X25519PublicKey::from(&my_ephemeral_secret);

    let remote_id_key_bytes = decode_b64(bundle["identityKey"].as_str().unwrap_or_default())?;
    let remote_spk_bytes = decode_b64(bundle["signedPreKey"]["publicKey"].as_str().unwrap_or_default())?;
    let remote_opk_bytes = if let Some(pk) = bundle["preKeys"].as_array().and_then(|a| a.first()) {
        Some(decode_b64(pk["publicKey"].as_str().unwrap_or_default())?)
    } else {
        None
    };

    let remote_id_public_bytes = ed25519_pub_to_x25519(&remote_id_key_bytes)?;
    let remote_id_public = X25519PublicKey::from(remote_id_public_bytes);
    let remote_spk_public = X25519PublicKey::from(<[u8; 32]>::try_from(remote_spk_bytes.as_slice()).map_err(|_| "Invalid Remote SPK")?);

    let dh1 = my_id_secret.diffie_hellman(&remote_spk_public);
    let dh2 = my_ephemeral_secret.diffie_hellman(&remote_id_public);
    let dh3 = my_ephemeral_secret.diffie_hellman(&remote_spk_public);
    
    let mut km = Vec::new();
    km.extend_from_slice(dh1.as_bytes());
    km.extend_from_slice(dh2.as_bytes());
    km.extend_from_slice(dh3.as_bytes());

    if let Some(opk_bytes) = remote_opk_bytes {
        let remote_opk_public = X25519PublicKey::from(<[u8; 32]>::try_from(opk_bytes.as_slice()).map_err(|_| "Invalid Remote OPK")?);
        let dh4 = my_ephemeral_secret.diffie_hellman(&remote_opk_public);
        km.extend_from_slice(dh4.as_bytes());
    }

    let remote_pq_id_pk = kyber1024::PublicKey::from_bytes(&decode_b64(bundle["pq_identityKey"].as_str().unwrap_or_default())?).map_err(|_| "Invalid PQ IK")?;
    let remote_pq_spk = kyber1024::PublicKey::from_bytes(&decode_b64(bundle["signedPreKey"]["pq_publicKey"].as_str().unwrap_or_default())?).map_err(|_| "Invalid PQ SPK")?;
    
    let (pq_ss1, pq_ct1) = kyber1024::encapsulate(&remote_pq_id_pk);
    let (pq_ss2, pq_ct2) = kyber1024::encapsulate(&remote_pq_spk);
    
    km.extend_from_slice(pq_ss1.as_bytes());
    km.extend_from_slice(pq_ss2.as_bytes());

    let hk = Hkdf::<Sha256>::new(None, &km);
    let mut root_key_bytes = [0u8; 32];
    hk.expand(b"EntropyV1 X3DH+PQ", &mut root_key_bytes).map_err(|e| e.to_string())?;

    let hk_gen = Hkdf::<Sha256>::new(None, &root_key_bytes);
    let mut hk_send = [0u8; 32];
    let mut hk_recv = [0u8; 32];
    hk_gen.expand(b"EntropyV1 HeaderSend", &mut hk_send).map_err(|e| e.to_string())?;
    hk_gen.expand(b"EntropyV1 HeaderRecv", &mut hk_recv).map_err(|e| e.to_string())?;

    let (rk_1, ck_1, _hk_ignored) = kdf_rk(&root_key_bytes, dh3.as_bytes())?;

    let state = SessionState {
        remote_identity_key: Some(encode_b64(remote_id_key_bytes.as_slice())),
        root_key: Some(encode_b64(&rk_1)),
        send_chain_key: Some(encode_b64(&ck_1)), 
        recv_chain_key: None, 
        send_ratchet_key_private: Some(encode_b64(my_ephemeral_secret.to_bytes().as_slice())),
        send_ratchet_key_public: Some(encode_b64(my_ephemeral_public.as_bytes())),
        recv_ratchet_key: Some(encode_b64(remote_spk_public.as_bytes())), 
        sequence_number_send: 0,
        sequence_number_recv: 0,
        prev_sequence_number_send: 0,
        send_header_key: Some(encode_b64(&hk_send)),
        recv_header_key: Some(encode_b64(&hk_recv)),
        next_send_header_key: None,
        next_recv_header_key: None,
        skipped_message_keys: HashMap::new(),
        is_verified: false,
        verified_identity_key: Some(encode_b64(remote_id_key_bytes.as_slice())),
        verification_timestamp: None,
        last_sent_hash: None,
        last_recv_hash: None,
        pq_ct1: Some(encode_b64(pq_ct1.as_bytes())),
        pq_ct2: Some(encode_b64(pq_ct2.as_bytes())),
        pq_shared_secret: {
            let mut combined = pq_ss1.as_bytes().to_vec();
            combined.extend_from_slice(pq_ss2.as_bytes());
            Some(encode_b64(&combined))
        },
    };

    state.save_to_db(conn, remote_hash)?;
    Ok(())
}

fn skip_message_keys(state: &mut SessionState, target_n: u32) -> Result<(), String> {
    if state.sequence_number_recv >= target_n { return Ok(()); }
    if target_n - state.sequence_number_recv > 100 {
        return Err("Too many messages to skip".to_string());
    }
    
    let ratchet_pub = state.recv_ratchet_key.clone().ok_or("No ratchet key")?;
    let mut current_ck = decode_b64(state.recv_chain_key.as_ref().ok_or("No recv chain")?)?;
    
    while state.sequence_number_recv < target_n {
        let (next_ck, mk) = kdf_ck(&current_ck)?;
        let key = format!("{}_{}", ratchet_pub, state.sequence_number_recv);
        state.skipped_message_keys.insert(key, encode_b64(&mk));
        current_ck = next_ck.to_vec();
        state.sequence_number_recv += 1;
    }
    
    state.recv_chain_key = Some(encode_b64(&current_ck));
    Ok(())
}

pub fn ratchet_encrypt(
    conn: &Connection,
    remote_hash: &str,
    plaintext: &str
) -> Result<serde_json::Value, String> {
    let mut state = SessionState::load_from_db(conn, remote_hash)?.ok_or("No session available")?;
    
    // Capture the header key to use for THIS message's header encryption.
    // If we ratchet below, we update the state's header key for the NEXT chain/message,
    // but the receiver expects THIS header to be encrypted with the CURRENT (old) key.
    let header_key_for_encryption = state.send_header_key.clone().ok_or("No header key")?;

    if state.send_chain_key.is_none() {
        let root_key = decode_b64(state.root_key.as_ref().ok_or("No root key")?)?;
        let remote_ratchet_bytes = decode_b64(state.recv_ratchet_key.as_ref().ok_or("No remote ratchet key")?)?;
        let remote_ratchet = X25519PublicKey::from(<[u8; 32]>::try_from(remote_ratchet_bytes).map_err(|_| "Invalid key size")?);

        let mut rng = thread_rng();
        let mut my_priv_bytes = [0u8; 32];
        rng.fill_bytes(&mut my_priv_bytes);
        let my_priv = StaticSecret::from(my_priv_bytes);
        let my_pub = X25519PublicKey::from(&my_priv);

        let dh = my_priv.diffie_hellman(&remote_ratchet);
        let (mut new_rk, ck, next_hk) = kdf_rk(&root_key, dh.as_bytes())?;

        if let Some(pq_ss_b64) = &state.pq_shared_secret {
            if let Ok(pq_ss) = decode_b64(pq_ss_b64) {
                new_rk = rk_mix_pq(&new_rk, &pq_ss)?;
            }
        }
        
        state.root_key = Some(encode_b64(&new_rk));
        state.send_chain_key = Some(encode_b64(&ck));
        state.send_header_key = Some(encode_b64(&next_hk));
        state.send_ratchet_key_private = Some(encode_b64(my_priv.to_bytes().as_slice()));
        state.send_ratchet_key_public = Some(encode_b64(my_pub.as_bytes()));
    }

    let current_ck_b64 = state.send_chain_key.clone().ok_or("No send chain key")?;
    let current_ck = decode_b64(&current_ck_b64)?;
    let (new_ck, mk) = kdf_ck(&current_ck)?;
    
    let padded_pt = pad_message(plaintext.as_bytes());

    let cipher = Aes256Gcm::new_from_slice(&mk).map_err(|e| e.to_string())?;
    let mut rng = thread_rng();
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, padded_pt.as_slice()).map_err(|e| e.to_string())?;
    
    let mut hasher = Sha256::new();
    hasher.update(&ciphertext);
    state.last_sent_hash = Some(hex::encode(hasher.finalize()));

    let lock_hash = state.last_recv_hash.clone().unwrap_or_default();

    state.send_chain_key = Some(encode_b64(&new_ck));
    let n = state.sequence_number_send;
    state.sequence_number_send += 1;
    state.save_to_db(conn, remote_hash)?;

    let ratchet_pub_bytes = decode_b64(&state.send_ratchet_key_public.clone().unwrap_or_default())?;
    let header_key_bytes = decode_b64(&header_key_for_encryption)?;

    let (header_enc, header_nonce) = encrypt_header(
        &header_key_bytes, 
        &ratchet_pub_bytes, 
        n, 
        state.prev_sequence_number_send
    )?;

    let mut msg_payload = serde_json::json!({
        "type": if n == 0 { 3 } else { 1 },
        "body": encode_b64(&ciphertext),
        "nonce": encode_b64(&nonce_bytes), 
        "header_enc": header_enc,
        "header_nonce": header_nonce,
        "lh": lock_hash 
    });

    if let Some(pq1) = state.pq_ct1.take() {
        msg_payload["pq1"] = serde_json::Value::String(pq1);
    }
    if let Some(pq2) = state.pq_ct2.take() {
        msg_payload["pq2"] = serde_json::Value::String(pq2);
    }

    if n == 0 {
        if let Ok(Some(me)) = ProtocolIdentity::load_from_db(conn) {
            msg_payload["ik"] = serde_json::Value::String(me.identity_keys.public_key);
            msg_payload["pq_ik"] = serde_json::Value::String(me.identity_keys.pq_public_key);
        }
    }
    msg_payload["ek"] = serde_json::Value::String(state.send_ratchet_key_public.clone().unwrap_or_default());

    state.save_to_db(conn, remote_hash)?;
    Ok(msg_payload)
}

pub fn ratchet_decrypt(
    conn: &Connection,
    remote_hash: &str,
    msg_obj: &serde_json::Value
) -> Result<String, String> {
    let mut state_opt = SessionState::load_from_db(conn, remote_hash)?;

    if state_opt.is_none() {
        let alice_ik_b64 = msg_obj.get("ik").and_then(|v| v.as_str()).ok_or("Missing IK in PreKey")?;
        let alice_ek_b64 = msg_obj.get("ek").and_then(|v| v.as_str()).ok_or("Missing EK in PreKey")?;

        let alice_ik_bytes = decode_b64(alice_ik_b64)?;
        let alice_ek_bytes = decode_b64(alice_ek_b64)?;
        
        let alice_ik = X25519PublicKey::from(ed25519_pub_to_x25519(&alice_ik_bytes)?);
        let alice_ek = X25519PublicKey::from(<[u8; 32]>::try_from(alice_ek_bytes).map_err(|_| "Invalid EK size")?);

        let identity = ProtocolIdentity::load_from_db(conn)?.ok_or("No identity")?;
        let bob_ik_priv = decode_b64(&identity.identity_keys.private_key)?;
        let bob_ik = ed25519_priv_to_x25519(&bob_ik_priv)?;
        let bob_spk_priv = decode_b64(&identity.signed_pre_key.private_key)?;
        let bob_spk = StaticSecret::from(<[u8; 32]>::try_from(bob_spk_priv).map_err(|_| "Invalid SPK size")?);

        let dh1 = bob_spk.diffie_hellman(&alice_ik);
        let dh2 = bob_ik.diffie_hellman(&alice_ek);
        let dh3 = bob_spk.diffie_hellman(&alice_ek);
        
        let mut km = Vec::new();
        km.extend_from_slice(dh1.as_bytes());
        km.extend_from_slice(dh2.as_bytes());
        km.extend_from_slice(dh3.as_bytes());

        let pq_ct1_b64 = msg_obj.get("pq1").and_then(|v| v.as_str()).ok_or("Missing PQ CT1")?;
        let pq_ct2_b64 = msg_obj.get("pq2").and_then(|v| v.as_str()).ok_or("Missing PQ CT2")?;
        
        let pq_ct1 = kyber1024::Ciphertext::from_bytes(&decode_b64(pq_ct1_b64)?).map_err(|_| "Invalid PQ CT1")?;
        let pq_ct2 = kyber1024::Ciphertext::from_bytes(&decode_b64(pq_ct2_b64)?).map_err(|_| "Invalid PQ CT2")?;
        
        let pq_id_sk = kyber1024::SecretKey::from_bytes(&decode_b64(&identity.identity_keys.pq_private_key)?).map_err(|_| "Invalid PQ ID SK")?;
        let pq_spk_sk = kyber1024::SecretKey::from_bytes(&decode_b64(&identity.signed_pre_key.pq_private_key)?).map_err(|_| "Invalid PQ SPK SK")?;
        
        let ss1 = kyber1024::decapsulate(&pq_ct1, &pq_id_sk);
        let ss2 = kyber1024::decapsulate(&pq_ct2, &pq_spk_sk);
        
        km.extend_from_slice(ss1.as_bytes());
        km.extend_from_slice(ss2.as_bytes());
        
        let hk = Hkdf::<Sha256>::new(None, &km);
        let mut root_key_bytes = [0u8; 32];
        hk.expand(b"EntropyV1 X3DH+PQ", &mut root_key_bytes).map_err(|e| e.to_string())?;

        let hk_gen = Hkdf::<Sha256>::new(None, &root_key_bytes);
        let mut hk_send = [0u8; 32];
        let mut hk_recv = [0u8; 32];
        
        hk_gen.expand(b"EntropyV1 HeaderSend", &mut hk_recv).map_err(|e| e.to_string())?;
        hk_gen.expand(b"EntropyV1 HeaderRecv", &mut hk_send).map_err(|e| e.to_string())?;

        let (rk_1, ck_1, _hk_ignored) = kdf_rk(&root_key_bytes, dh3.as_bytes())?;
        let new_state = SessionState {
            remote_identity_key: Some(alice_ik_b64.to_string()),
            root_key: Some(encode_b64(&rk_1)),
            send_chain_key: None, 
            recv_chain_key: Some(encode_b64(&ck_1)), 
            send_ratchet_key_private: Some(encode_b64(bob_spk.to_bytes().as_slice())),
            send_ratchet_key_public: Some(identity.signed_pre_key.public_key.clone()),
            recv_ratchet_key: Some(alice_ek_b64.to_string()), 
            sequence_number_send: 0,
            sequence_number_recv: 0,
            prev_sequence_number_send: 0,
            send_header_key: Some(encode_b64(&hk_send)), 
            recv_header_key: Some(encode_b64(&hk_recv)),
            next_send_header_key: None,
            next_recv_header_key: None,
            skipped_message_keys: HashMap::new(),
            is_verified: false,
            verified_identity_key: Some(alice_ik_b64.to_string()),
            verification_timestamp: None,
            last_sent_hash: None,
            last_recv_hash: None,
            pq_ct1: msg_obj.get("pq1").and_then(|v| v.as_str()).map(|s| s.to_string()),
            pq_ct2: msg_obj.get("pq2").and_then(|v| v.as_str()).map(|s| s.to_string()),
            pq_shared_secret: {
                let mut combined = ss1.as_bytes().to_vec();
                combined.extend_from_slice(ss2.as_bytes());
                Some(encode_b64(&combined))
            },
        };
        new_state.save_to_db(conn, remote_hash)?;
        state_opt = Some(new_state);
    }

    let mut state = state_opt.unwrap();
    let header_enc = msg_obj["header_enc"].as_str().ok_or("Missing header_enc")?;
    let header_nonce = msg_obj["header_nonce"].as_str().ok_or("Missing header_nonce")?;

    let recv_header_key = decode_b64(state.recv_header_key.as_ref().ok_or("No recv header key")?)?;
    let header = decrypt_header(&recv_header_key, header_enc, header_nonce).map_err(|e| format!("Header decrypt failed: {}", e))?;

    let n = header["n"].as_u64().ok_or("Missing n")? as u32;
    let pn = header["pn"].as_u64().ok_or("Missing pn")? as u32;
    let ratchet_pub_b64 = header["ratchet_key"].as_str().ok_or("Missing ratchet key")?;

    let lookup_key = format!("{}_{}", ratchet_pub_b64, n);
    if let Some(mk_b64) = state.skipped_message_keys.remove(&lookup_key) {
        state.save_to_db(conn, remote_hash)?; 
        
        let mk = decode_b64(&mk_b64)?;
        let cipher = Aes256Gcm::new_from_slice(&mk).map_err(|e| e.to_string())?;
    
        let ct_b64 = msg_obj["body"].as_str().ok_or("Missing body")?;
        let nonce_b64 = msg_obj["nonce"].as_str().ok_or("Missing nonce")?;
        let ct = decode_b64(ct_b64)?;
        let nonce_bytes = decode_b64(nonce_b64)?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        let pt_padded = cipher.decrypt(nonce, ct.as_slice()).map_err(|e| format!("Decrypt failed: {}", e))?;
        let plaintext = unpad_message(&pt_padded)?;
        return String::from_utf8(plaintext).map_err(|e| e.to_string());
    }

    let is_new_ratchet = if let Some(rk) = &state.recv_ratchet_key {
        rk != ratchet_pub_b64
    } else {
        true
    };

    if is_new_ratchet {
        skip_message_keys(&mut state, pn)?;
        
        let root_key = decode_b64(state.root_key.as_ref().ok_or("No root key")?)?;
        let remote_ratchet_bytes = decode_b64(ratchet_pub_b64)?;
        let remote_ratchet = X25519PublicKey::from(<[u8; 32]>::try_from(remote_ratchet_bytes).map_err(|_| "Invalid key size")?);
        
        let my_priv_bytes = decode_b64(state.send_ratchet_key_private.as_ref().ok_or("No send ratchet private")?)?;
        let my_priv_arr: [u8; 32] = my_priv_bytes.try_into().map_err(|_| "Invalid ratchet key size".to_string())?;
        let my_priv = StaticSecret::from(my_priv_arr);
        
        let dh = my_priv.diffie_hellman(&remote_ratchet);
        let (new_rk, ck, new_hk) = kdf_rk(&root_key, dh.as_bytes())?;

        state.root_key = Some(encode_b64(&new_rk));
        state.recv_chain_key = Some(encode_b64(&ck));
        state.next_recv_header_key = Some(encode_b64(&new_hk));
        state.recv_ratchet_key = Some(ratchet_pub_b64.to_string());
        state.prev_sequence_number_send = state.sequence_number_send;
        state.sequence_number_send = 0;
        state.sequence_number_recv = 0;

        state.recv_header_key = state.next_recv_header_key.take(); 
    }

    skip_message_keys(&mut state, n)?;
    
    let current_ck = decode_b64(state.recv_chain_key.as_ref().ok_or("No recv chain")?)?;
    let (next_ck, mk) = kdf_ck(&current_ck)?;
    state.recv_chain_key = Some(encode_b64(&next_ck));
    state.sequence_number_recv += 1;

    let cipher = Aes256Gcm::new_from_slice(&mk).map_err(|e| e.to_string())?;
    
    let ct_b64 = msg_obj["body"].as_str().ok_or("Missing body")?;
    let nonce_b64 = msg_obj["nonce"].as_str().ok_or("Missing nonce")?;
    let ct = decode_b64(ct_b64)?;
    let nonce_bytes = decode_b64(nonce_b64)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let pt_padded = cipher.decrypt(nonce, ct.as_slice()).map_err(|e| format!("Decrypt failed: {}", e))?;
    let plaintext = unpad_message(&pt_padded)?;

    if let Some(lh) = msg_obj["lh"].as_str() {
        if let Some(my_last) = &state.last_sent_hash {
             if lh != my_last && lh != "" {
                 return Err(format!("CONTINUITY_BREAK: Remote LH {} != Local LH {}", lh, my_last));
             }
        }
    }
    
    let mut hasher = Sha256::new();
    hasher.update(&ct);
    state.last_recv_hash = Some(hex::encode(hasher.finalize()));

    state.save_to_db(conn, remote_hash)?;
    
    String::from_utf8(plaintext).map_err(|e| e.to_string())
}

pub fn seal_sender(
    message: serde_json::Value,
    my_identity_public: &str,
    recipient_identity_public: &X25519PublicKey,
    recipient_pq_identity_public: &str
) -> Result<serde_json::Value, String> {
    let mut rng = thread_rng();
    let mut ephem_arr = [0u8; 32];
    rng.fill_bytes(&mut ephem_arr);
    let ephem_secret = StaticSecret::from(ephem_arr);
    let ephem_public = X25519PublicKey::from(&ephem_secret);

    let shared_secret = ephem_secret.diffie_hellman(recipient_identity_public);
    
    let pq_pk_bytes = decode_b64(recipient_pq_identity_public)?;
    let pq_pk = kyber1024::PublicKey::from_bytes(&pq_pk_bytes).map_err(|_| "Invalid PQ IK")?;
    let (pq_ss, pq_ct) = kyber1024::encapsulate(&pq_pk);

    let mut km = Vec::new();
    km.extend_from_slice(shared_secret.as_bytes());
    km.extend_from_slice(pq_ss.as_bytes());

    let mut hasher = Sha256::new();
    hasher.update(&km);
    let aes_key = hasher.finalize();

    let envelope = SealedEnvelope {
        sender: my_identity_public.to_string(),
        message: message
    };
    let envelope_json = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;

    let cipher = Aes256Gcm::new_from_slice(&aes_key).map_err(|e| e.to_string())?;
    let mut nonce_arr = [0u8; 12];
    rng.fill_bytes(&mut nonce_arr);
    let nonce = Nonce::from_slice(&nonce_arr);

    let ciphertext = cipher.encrypt(nonce, envelope_json.as_slice()).map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "ephemeral_public": encode_b64(ephem_public.as_bytes()),
        "pq_ct": encode_b64(pq_ct.as_bytes()),
        "nonce": encode_b64(&nonce_arr),
        "ciphertext": encode_b64(&ciphertext)
    }))
}

pub fn unseal_sender(
    sealed_obj: &serde_json::Value,
    my_identity_secret: &StaticSecret,
    my_pq_identity_secret: &kyber1024::SecretKey
) -> Result<(String, serde_json::Value), String> {
    let ephem_b64 = sealed_obj["ephemeral_public"].as_str().ok_or("No ephemeral_public")?;
    let pq_ct_b64 = sealed_obj["pq_ct"].as_str().ok_or("No pq_ct")?;
    let nonce_b64 = sealed_obj["nonce"].as_str().ok_or("No nonce")?;
    let ct_b64 = sealed_obj["ciphertext"].as_str().ok_or("No ciphertext")?;

    let ephem_bytes = decode_b64(ephem_b64)?;
    let mut ephem_arr = [0u8; 32];
    ephem_arr.copy_from_slice(&ephem_bytes);
    let ephem_pub = X25519PublicKey::from(ephem_arr);
    let shared_secret = my_identity_secret.diffie_hellman(&ephem_pub);

    let pq_ct = kyber1024::Ciphertext::from_bytes(&decode_b64(pq_ct_b64)?).map_err(|_| "Invalid PQ CT")?;
    let pq_ss = kyber1024::decapsulate(&pq_ct, my_pq_identity_secret);

    let mut km = Vec::new();
    km.extend_from_slice(shared_secret.as_bytes());
    km.extend_from_slice(pq_ss.as_bytes());

    let mut hasher = Sha256::new();
    hasher.update(&km);
    let aes_key = hasher.finalize();

    let cipher = Aes256Gcm::new_from_slice(&aes_key).map_err(|e| e.to_string())?;
    let nonce_vec = decode_b64(nonce_b64).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_vec);
    let ct_vec = decode_b64(ct_b64).map_err(|e| e.to_string())?;

    let pt = cipher.decrypt(nonce, ct_vec.as_slice()).map_err(|e| e.to_string())?;
    let envelope: SealedEnvelope = serde_json::from_slice(&pt).map_err(|e| e.to_string())?;

    Ok((envelope.sender, envelope.message))
}

pub fn save_pending_message(conn: &Connection, msg: &PendingMessage) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO pending_messages (id, recipient_hash, body, timestamp, retries) VALUES (?1, ?2, ?3, ?4, ?5);",
        params![msg.id, msg.recipient_hash, msg.body, msg.timestamp, msg.retries],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_pending_messages(conn: &Connection) -> Result<Vec<PendingMessage>, String> {
    let mut stmt = conn.prepare("SELECT id, recipient_hash, body, timestamp, retries FROM pending_messages;").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(PendingMessage {
            id: row.get(0)?,
            recipient_hash: row.get(1)?,
            body: row.get(2)?,
            timestamp: row.get(3)?,
            retries: row.get(4)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut msgs = Vec::new();
    for row in rows {
        msgs.push(row.map_err(|e| e.to_string())?);
    }
    Ok(msgs)
}

pub fn remove_pending_message(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM pending_messages WHERE id = ?1;", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn verify_session(
    conn: &Connection,
    remote_hash: &str,
    is_verified: bool
) -> Result<(), String> {
    let mut state = SessionState::load_from_db(conn, remote_hash)?.ok_or("Session not found")?;
    state.is_verified = is_verified;
    state.verification_timestamp = if is_verified { 
        Some(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)) 
    } else { None };
    state.save_to_db(conn, remote_hash)?;
    Ok(())
}

pub fn secure_nuke_database(db_path: &std::path::Path) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::io::Write;

    if db_path.exists() {
        let size = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(1024 * 1024);
        let mut rng = thread_rng();
        
        for _ in 0..3 {
            let mut file = OpenOptions::new().write(true).open(db_path).map_err(|e| e.to_string())?;
            let mut remaining = size;
            let chunk_size = 1024 * 1024;
            let mut junk = vec![0u8; chunk_size];
            
            while remaining > 0 {
                let to_write = std::cmp::min(remaining, chunk_size as u64);
                rng.fill_bytes(&mut junk[..to_write as usize]);
                file.write_all(&junk[..to_write as usize]).map_err(|e| e.to_string())?;
                remaining -= to_write;
            }
            file.sync_all().map_err(|e| e.to_string())?;
        }
        
        let file = OpenOptions::new().write(true).open(db_path).map_err(|e| e.to_string())?;
        file.set_len(0).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);

        let mut random_name = [0u8; 16];
        rng.fill_bytes(&mut random_name);
        let new_path = db_path.with_file_name(hex::encode(random_name));
        let _ = std::fs::rename(db_path, &new_path);
        
        std::fs::remove_file(new_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
