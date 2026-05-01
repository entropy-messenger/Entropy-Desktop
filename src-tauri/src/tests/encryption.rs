//! Production-grade validation suite for the PQXDH encryption pipeline.

use async_trait::async_trait;
use libsignal_protocol::*;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A thread-safe, in-memory implementation of Signal's storage traits for testing.
#[derive(Clone)]
pub struct InMemorySignalStore {
    identity_key_pair: IdentityKeyPair,
    registration_id: u32,
    sessions: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    pre_keys: Arc<RwLock<HashMap<PreKeyId, Vec<u8>>>>,
    signed_pre_keys: Arc<RwLock<HashMap<SignedPreKeyId, Vec<u8>>>>,
    kyber_pre_keys: Arc<RwLock<HashMap<KyberPreKeyId, Vec<u8>>>>,
    identities: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    used_base_keys: Arc<RwLock<std::collections::HashSet<Vec<u8>>>>,
}

impl InMemorySignalStore {
    pub fn new(identity_key_pair: IdentityKeyPair, registration_id: u32) -> Self {
        Self {
            identity_key_pair,
            registration_id,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pre_keys: Arc::new(RwLock::new(HashMap::new())),
            signed_pre_keys: Arc::new(RwLock::new(HashMap::new())),
            kyber_pre_keys: Arc::new(RwLock::new(HashMap::new())),
            identities: Arc::new(RwLock::new(HashMap::new())),
            used_base_keys: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }
}

#[async_trait(?Send)]
impl IdentityKeyStore for InMemorySignalStore {
    async fn get_identity_key_pair(&self) -> Result<IdentityKeyPair, SignalProtocolError> {
        Ok(self.identity_key_pair)
    }
    async fn get_local_registration_id(&self) -> Result<u32, SignalProtocolError> {
        Ok(self.registration_id)
    }
    async fn save_identity(
        &mut self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
    ) -> Result<IdentityChange, SignalProtocolError> {
        let mut lock = self.identities.write().unwrap();
        let pub_bytes = identity.serialize().to_vec();
        if let Some(existing) = lock.get(&address.to_string()) {
            if existing == &pub_bytes {
                Ok(IdentityChange::NewOrUnchanged)
            } else {
                lock.insert(address.to_string(), pub_bytes);
                Ok(IdentityChange::ReplacedExisting)
            }
        } else {
            lock.insert(address.to_string(), pub_bytes);
            Ok(IdentityChange::NewOrUnchanged)
        }
    }
    async fn is_trusted_identity(
        &self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
        _direction: Direction,
    ) -> Result<bool, SignalProtocolError> {
        let lock = self.identities.read().unwrap();
        if let Some(stored) = lock.get(&address.to_string()) {
            Ok(stored == identity.serialize().as_ref())
        } else {
            // Trust on first use
            Ok(true)
        }
    }
    async fn get_identity(
        &self,
        address: &ProtocolAddress,
    ) -> Result<Option<IdentityKey>, SignalProtocolError> {
        let lock = self.identities.read().unwrap();
        if let Some(bytes) = lock.get(&address.to_string()) {
            Ok(Some(IdentityKey::decode(bytes)?))
        } else {
            Ok(None)
        }
    }
}

#[async_trait(?Send)]
impl SessionStore for InMemorySignalStore {
    async fn load_session(
        &self,
        address: &ProtocolAddress,
    ) -> Result<Option<SessionRecord>, SignalProtocolError> {
        let lock = self.sessions.read().unwrap();
        if let Some(bytes) = lock.get(&address.to_string()) {
            Ok(Some(SessionRecord::deserialize(bytes)?))
        } else {
            Ok(None)
        }
    }
    async fn store_session(
        &mut self,
        address: &ProtocolAddress,
        record: &SessionRecord,
    ) -> Result<(), SignalProtocolError> {
        let mut lock = self.sessions.write().unwrap();
        lock.insert(address.to_string(), record.serialize()?);
        Ok(())
    }
}

#[async_trait(?Send)]
impl PreKeyStore for InMemorySignalStore {
    async fn get_pre_key(&self, pre_key_id: PreKeyId) -> Result<PreKeyRecord, SignalProtocolError> {
        let lock = self.pre_keys.read().unwrap();
        let bytes = lock
            .get(&pre_key_id)
            .ok_or(SignalProtocolError::InvalidPreKeyId)?;
        PreKeyRecord::deserialize(bytes)
    }
    async fn save_pre_key(
        &mut self,
        pre_key_id: PreKeyId,
        record: &PreKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let mut lock = self.pre_keys.write().unwrap();
        lock.insert(pre_key_id, record.serialize()?);
        Ok(())
    }
    async fn remove_pre_key(&mut self, pre_key_id: PreKeyId) -> Result<(), SignalProtocolError> {
        let mut lock = self.pre_keys.write().unwrap();
        lock.remove(&pre_key_id);
        Ok(())
    }
}

#[async_trait(?Send)]
impl SignedPreKeyStore for InMemorySignalStore {
    async fn get_signed_pre_key(
        &self,
        signed_pre_key_id: SignedPreKeyId,
    ) -> Result<SignedPreKeyRecord, SignalProtocolError> {
        let lock = self.signed_pre_keys.read().unwrap();
        let bytes = lock
            .get(&signed_pre_key_id)
            .ok_or(SignalProtocolError::InvalidSignedPreKeyId)?;
        SignedPreKeyRecord::deserialize(bytes)
    }
    async fn save_signed_pre_key(
        &mut self,
        signed_pre_key_id: SignedPreKeyId,
        record: &SignedPreKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let mut lock = self.signed_pre_keys.write().unwrap();
        lock.insert(signed_pre_key_id, record.serialize()?);
        Ok(())
    }
}

#[async_trait(?Send)]
impl KyberPreKeyStore for InMemorySignalStore {
    async fn get_kyber_pre_key(
        &self,
        kyber_prekey_id: KyberPreKeyId,
    ) -> Result<KyberPreKeyRecord, SignalProtocolError> {
        let lock = self.kyber_pre_keys.read().unwrap();
        let bytes = lock
            .get(&kyber_prekey_id)
            .ok_or(SignalProtocolError::InvalidKyberPreKeyId)?;
        KyberPreKeyRecord::deserialize(bytes)
    }
    async fn save_kyber_pre_key(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        record: &KyberPreKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let mut lock = self.kyber_pre_keys.write().unwrap();
        lock.insert(kyber_prekey_id, record.serialize()?);
        Ok(())
    }
    async fn mark_kyber_pre_key_used(
        &mut self,
        _kyber_prekey_id: KyberPreKeyId,
        _ec_prekey_id: SignedPreKeyId,
        base_key: &PublicKey,
    ) -> Result<(), SignalProtocolError> {
        let mut lock = self.used_base_keys.write().unwrap();
        let bytes = base_key.serialize().to_vec();
        if lock.contains(&bytes) {
            return Err(SignalProtocolError::InvalidMessage(
                libsignal_protocol::CiphertextMessageType::PreKey,
                "reused base key",
            ));
        }
        lock.insert(bytes);
        Ok(())
    }
}

#[tokio::test]
async fn encryption_handshake() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::from_os_rng();

    // 1. Setup Alice and Bob
    let alice_id_pair = IdentityKeyPair::generate(&mut rng);
    let alice_reg_id = 1111;
    let mut alice_store = InMemorySignalStore::new(alice_id_pair, alice_reg_id);
    let alice_addr = ProtocolAddress::new("alice".to_string(), DeviceId::try_from(1)?);

    let bob_id_pair = IdentityKeyPair::generate(&mut rng);
    let bob_reg_id = 2222;
    let mut bob_store = InMemorySignalStore::new(bob_id_pair, bob_reg_id);
    let bob_addr = ProtocolAddress::new("bob".to_string(), DeviceId::try_from(1)?);

    let bob_pre_key_id = PreKeyId::from(1);
    let bob_pre_key_pair = KeyPair::generate(&mut rng);
    let bob_pre_key_record = PreKeyRecord::new(bob_pre_key_id, &bob_pre_key_pair);
    bob_store
        .save_pre_key(bob_pre_key_id, &bob_pre_key_record)
        .await?;

    let bob_signed_pre_key_id = SignedPreKeyId::from(1);
    let bob_signed_pre_key_pair = KeyPair::generate(&mut rng);
    let timestamp = Timestamp::from_epoch_millis(123456789);
    let signature = bob_id_pair
        .private_key()
        .calculate_signature(&bob_signed_pre_key_pair.public_key.serialize(), &mut rng)?;
    let bob_signed_pre_key_record = SignedPreKeyRecord::new(
        bob_signed_pre_key_id,
        timestamp,
        &bob_signed_pre_key_pair,
        &signature,
    );
    bob_store
        .save_signed_pre_key(bob_signed_pre_key_id, &bob_signed_pre_key_record)
        .await?;

    let bob_kyber_pre_key_id = KyberPreKeyId::from(1);
    let bob_kyber_pre_key_record = KyberPreKeyRecord::generate(
        kem::KeyType::Kyber1024,
        bob_kyber_pre_key_id,
        bob_id_pair.private_key(),
    )?;
    bob_store
        .save_kyber_pre_key(bob_kyber_pre_key_id, &bob_kyber_pre_key_record)
        .await?;

    let bob_bundle = PreKeyBundle::new(
        bob_reg_id,
        DeviceId::try_from(1)?,
        Some((bob_pre_key_id, bob_pre_key_pair.public_key)),
        bob_signed_pre_key_id,
        bob_signed_pre_key_pair.public_key,
        signature.to_vec(),
        bob_kyber_pre_key_id,
        bob_kyber_pre_key_record.public_key()?,
        bob_kyber_pre_key_record.signature()?.to_vec(),
        *bob_id_pair.identity_key(),
    )?;

    process_prekey_bundle(
        &bob_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        &bob_bundle,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    let plaintext = b"Hello, Entropy Post-Quantum World!";
    let ciphertext = message_encrypt(
        plaintext,
        &bob_addr,
        &alice_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    assert!(matches!(
        ciphertext,
        CiphertextMessage::PreKeySignalMessage(_)
    ));

    let decrypted = message_decrypt(
        &ciphertext,
        &alice_addr,
        &bob_addr,
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &bob_store.clone(),
        &mut bob_store,
        &mut rng,
    )
    .await?;

    assert_eq!(decrypted, plaintext);
    println!("Encryption roundtrip successful!");

    let bob_reply = b"I hear you loud and clear, Alice.";
    let bob_ciphertext = message_encrypt(
        bob_reply,
        &alice_addr,
        &bob_addr,
        &mut bob_store.clone(),
        &mut bob_store,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    assert!(matches!(
        bob_ciphertext,
        CiphertextMessage::SignalMessage(_)
    ));

    let alice_decrypted = message_decrypt(
        &bob_ciphertext,
        &bob_addr,
        &alice_addr,
        &mut alice_store.clone(),
        &mut alice_store.clone(),
        &mut alice_store.clone(),
        &alice_store.clone(),
        &mut alice_store,
        &mut rng,
    )
    .await?;

    assert_eq!(alice_decrypted, bob_reply);
    println!("Ratcheting reply successful!");

    Ok(())
}

#[tokio::test]
async fn out_of_order_delivery() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::from_os_rng();

    let (mut alice_store, alice_addr) = setup_test_user("alice", 1111, &mut rng).await?;
    let (mut bob_store, bob_addr) = setup_test_user("bob", 2222, &mut rng).await?;

    let bob_bundle = generate_bundle(&mut bob_store, 2222, &mut rng).await?;

    process_prekey_bundle(
        &bob_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        &bob_bundle,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    let msg1 = b"Message 1";
    let msg2 = b"Message 2";
    let msg3 = b"Message 3";

    let cipher1 = message_encrypt(
        msg1,
        &bob_addr,
        &alice_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;
    let cipher2 = message_encrypt(
        msg2,
        &bob_addr,
        &alice_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;
    let cipher3 = message_encrypt(
        msg3,
        &bob_addr,
        &alice_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    let dec1 = message_decrypt(
        &cipher1,
        &alice_addr,
        &bob_addr,
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &bob_store.clone(),
        &mut bob_store,
        &mut rng,
    )
    .await?;
    assert_eq!(dec1, msg1);

    let dec3 = message_decrypt(
        &cipher3,
        &alice_addr,
        &bob_addr,
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &bob_store.clone(),
        &mut bob_store,
        &mut rng,
    )
    .await?;
    assert_eq!(dec3, msg3);

    let dec2 = message_decrypt(
        &cipher2,
        &alice_addr,
        &bob_addr,
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &bob_store.clone(),
        &mut bob_store,
        &mut rng,
    )
    .await?;
    assert_eq!(dec2, msg2);

    println!("Out-of-order decryption successful!");
    Ok(())
}

#[tokio::test]
async fn malicious_bundle_rejection() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::from_os_rng();

    let (mut alice_store, _alice_addr) = setup_test_user("alice", 1111, &mut rng).await?;
    let (mut bob_store, bob_addr) = setup_test_user("bob", 2222, &mut rng).await?;

    let mut bob_bundle = generate_bundle(&mut bob_store, 2222, &mut rng).await?;

    let malicious_id_pair = IdentityKeyPair::generate(&mut rng);
    bob_bundle = PreKeyBundle::new(
        bob_bundle.registration_id()?,
        bob_bundle.device_id()?,
        match (bob_bundle.pre_key_id()?, bob_bundle.pre_key_public()?) {
            (Some(id), Some(k)) => Some((id, k)),
            _ => None,
        },
        bob_bundle.signed_pre_key_id()?,
        bob_bundle.signed_pre_key_public()?,
        bob_bundle.signed_pre_key_signature()?.to_vec(),
        bob_bundle.kyber_pre_key_id()?,
        bob_bundle.kyber_pre_key_public()?.clone(),
        bob_bundle.kyber_pre_key_signature()?.to_vec(),
        *malicious_id_pair.identity_key(), // Wrong key!
    )?;

    // Alice should fail to process this bundle because the signature on the signed prekey
    // won't match the malicious identity key.
    let result = process_prekey_bundle(
        &bob_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        &bob_bundle,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await;

    assert!(result.is_err());
    println!("Identity mismatch correctly rejected!");
    Ok(())
}

#[tokio::test]
async fn extended_chat_stability() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::from_os_rng();

    let (mut alice_store, alice_addr) = setup_test_user("alice", 1111, &mut rng).await?;
    let (mut bob_store, bob_addr) = setup_test_user("bob", 2222, &mut rng).await?;

    let bob_bundle = generate_bundle(&mut bob_store, 2222, &mut rng).await?;
    process_prekey_bundle(
        &bob_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        &bob_bundle,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    for i in 0..10 {
        // Alice -> Bob
        let a_msg = format!("Ping {}", i);
        let a_cipher = message_encrypt(
            a_msg.as_bytes(),
            &bob_addr,
            &alice_addr,
            &mut alice_store.clone(),
            &mut alice_store,
            std::time::SystemTime::now(),
            &mut rng,
        )
        .await?;
        let b_dec = message_decrypt(
            &a_cipher,
            &alice_addr,
            &bob_addr,
            &mut bob_store.clone(),
            &mut bob_store.clone(),
            &mut bob_store.clone(),
            &bob_store.clone(),
            &mut bob_store,
            &mut rng,
        )
        .await?;
        assert_eq!(b_dec, a_msg.as_bytes());

        // Bob -> Alice
        let b_msg = format!("Pong {}", i);
        let b_cipher = message_encrypt(
            b_msg.as_bytes(),
            &alice_addr,
            &bob_addr,
            &mut bob_store.clone(),
            &mut bob_store,
            std::time::SystemTime::now(),
            &mut rng,
        )
        .await?;
        let a_dec = message_decrypt(
            &b_cipher,
            &bob_addr,
            &alice_addr,
            &mut alice_store.clone(),
            &mut alice_store.clone(),
            &mut alice_store.clone(),
            &alice_store.clone(),
            &mut alice_store,
            &mut rng,
        )
        .await?;
        assert_eq!(a_dec, b_msg.as_bytes());
    }

    println!("20-message stress ratchet successful!");
    Ok(())
}

#[tokio::test]
async fn replay_attack_prevention() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::from_os_rng();

    let (mut alice_store, alice_addr) = setup_test_user("alice", 1111, &mut rng).await?;
    let (mut bob_store, bob_addr) = setup_test_user("bob", 2222, &mut rng).await?;

    let bob_bundle = generate_bundle(&mut bob_store, 2222, &mut rng).await?;
    process_prekey_bundle(
        &bob_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        &bob_bundle,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    let msg = b"Reusable?";
    let ciphertext = message_encrypt(
        msg,
        &bob_addr,
        &alice_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    message_decrypt(
        &ciphertext,
        &alice_addr,
        &bob_addr,
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &bob_store.clone(),
        &mut bob_store,
        &mut rng,
    )
    .await?;

    // Second decryption with SAME ciphertext should fail because mark_kyber_pre_key_used will see the same base key
    let result = message_decrypt(
        &ciphertext,
        &alice_addr,
        &bob_addr,
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &mut bob_store.clone(),
        &bob_store.clone(),
        &mut bob_store,
        &mut rng,
    )
    .await;

    assert!(result.is_err());
    println!("Base key reuse correctly rejected!");
    Ok(())
}

#[tokio::test]
async fn identity_rotation_detection() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::from_os_rng();

    let (mut alice_store, _alice_addr) = setup_test_user("alice", 1111, &mut rng).await?;
    let (mut bob_store, bob_addr) = setup_test_user("bob", 2222, &mut rng).await?;

    let bob_bundle1 = generate_bundle(&mut bob_store, 2222, &mut rng).await?;
    process_prekey_bundle(
        &bob_addr,
        &mut alice_store.clone(),
        &mut alice_store,
        &bob_bundle1,
        std::time::SystemTime::now(),
        &mut rng,
    )
    .await?;

    assert!(
        alice_store
            .is_trusted_identity(&bob_addr, bob_bundle1.identity_key()?, Direction::Sending)
            .await?
    );

    let bob_id_pair2 = IdentityKeyPair::generate(&mut rng);
    let mut bob_store2 = InMemorySignalStore::new(bob_id_pair2, 3333);
    let bob_bundle2 = generate_bundle(&mut bob_store2, 3333, &mut rng).await?;
    // is_trusted_identity should return false because the key has changed.
    let is_trusted = alice_store
        .is_trusted_identity(&bob_addr, bob_bundle2.identity_key()?, Direction::Sending)
        .await?;
    assert!(!is_trusted);

    // When Alice saves the new identity, it should return IdentityChange::ReplacedExisting
    let change = alice_store
        .save_identity(&bob_addr, bob_bundle2.identity_key()?)
        .await?;
    assert!(matches!(change, IdentityChange::ReplacedExisting));

    println!("Identity key change correctly detected!");
    Ok(())
}

// --- Helper Functions for Tests ---

async fn setup_test_user(
    name: &str,
    reg_id: u32,
    rng: &mut StdRng,
) -> Result<(InMemorySignalStore, ProtocolAddress), Box<dyn std::error::Error>> {
    let id_pair = IdentityKeyPair::generate(rng);
    let store = InMemorySignalStore::new(id_pair, reg_id);
    let addr = ProtocolAddress::new(name.to_string(), DeviceId::try_from(1)?);
    Ok((store, addr))
}

async fn generate_bundle(
    store: &mut InMemorySignalStore,
    reg_id: u32,
    rng: &mut StdRng,
) -> Result<PreKeyBundle, Box<dyn std::error::Error>> {
    let id_pair = store.get_identity_key_pair().await?;

    let pre_key_id = PreKeyId::from(1);
    let pre_key_pair = KeyPair::generate(rng);
    let pre_key_record = PreKeyRecord::new(pre_key_id, &pre_key_pair);
    store.save_pre_key(pre_key_id, &pre_key_record).await?;

    let signed_pre_key_id = SignedPreKeyId::from(1);
    let signed_pre_key_pair = KeyPair::generate(rng);
    let timestamp = Timestamp::from_epoch_millis(123456789);
    let signature = id_pair
        .private_key()
        .calculate_signature(&signed_pre_key_pair.public_key.serialize(), rng)?;
    let signed_pre_key_record = SignedPreKeyRecord::new(
        signed_pre_key_id,
        timestamp,
        &signed_pre_key_pair,
        &signature,
    );
    store
        .save_signed_pre_key(signed_pre_key_id, &signed_pre_key_record)
        .await?;

    let kyber_pre_key_id = KyberPreKeyId::from(1);
    let kyber_pre_key_record = KyberPreKeyRecord::generate(
        kem::KeyType::Kyber1024,
        kyber_pre_key_id,
        id_pair.private_key(),
    )?;
    store
        .save_kyber_pre_key(kyber_pre_key_id, &kyber_pre_key_record)
        .await?;

    Ok(PreKeyBundle::new(
        reg_id,
        DeviceId::try_from(1)?,
        Some((pre_key_id, pre_key_pair.public_key)),
        signed_pre_key_id,
        signed_pre_key_pair.public_key,
        signature.to_vec(),
        kyber_pre_key_id,
        kyber_pre_key_record.public_key()?,
        kyber_pre_key_record.signature()?.to_vec(),
        *id_pair.identity_key(),
    )?)
}
