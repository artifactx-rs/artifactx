//! PGP signing via rpgp (`pgp` crate, pure Rust).
//!
//! Keys are generated as **v4 RSA** so signatures are verifiable by the
//! traditional gpg used by apt and dnf/rpm across old and new distributions.
//!
//! The private key can be **encrypted at rest** with a passphrase (OpenPGP S2K).
//! All signing entry points take the passphrase to unlock the key; an empty
//! passphrase means the key is unencrypted.

use anyhow::{anyhow, Context, Result};
use chrono::SubsecRound;
use pgp::composed::cleartext::CleartextSignedMessage;
use pgp::composed::{
    Deserializable, KeyType, SecretKeyParamsBuilder, SignedSecretKey, StandaloneSignature,
};
use pgp::crypto::hash::HashAlgorithm;
use pgp::packet::{SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::types::{KeyVersion, PublicKeyTrait, SecretKeyTrait};
use pgp::ArmorOptions;

/// RSA key size for generated signing keys. 2048 is the de-facto standard for
/// repository signing — fast to generate and verifiable everywhere apt/dnf run.
const RSA_BITS: u32 = 2048;

/// A freshly generated signing keypair, both armored (ASCII).
pub struct GeneratedKey {
    pub private_armored: String,
    pub public_armored: String,
}

/// Generate a new v4 RSA signing keypair bound to `user_id`.
///
/// A non-empty `passphrase` encrypts the private key at rest (OpenPGP S2K); an
/// empty passphrase produces an unencrypted key.
pub fn generate_key(user_id: &str, passphrase: &str) -> Result<GeneratedKey> {
    let mut builder = SecretKeyParamsBuilder::default();
    builder
        .key_type(KeyType::Rsa(RSA_BITS))
        .version(KeyVersion::V4)
        .can_sign(true)
        .can_certify(true)
        .primary_user_id(user_id.to_string());
    if !passphrase.is_empty() {
        builder.passphrase(Some(passphrase.to_string()));
    }
    let params = builder
        .build()
        .map_err(|e| anyhow!("building key params: {e}"))?;

    let secret = params
        .generate(rand::thread_rng())
        .context("generating secret key")?;
    // The passphrase both encrypts the key at rest and unlocks it for self-signing.
    let signed = secret
        .sign(&mut rand::thread_rng(), || passphrase.to_string())
        .context("self-signing secret key")?;

    let private_armored = signed
        .to_armored_string(ArmorOptions::default())
        .context("armoring private key")?;

    let public = signed.public_key();
    let signed_public = public
        .sign(&mut rand::thread_rng(), &signed, || passphrase.to_string())
        .context("signing public key")?;
    let public_armored = signed_public
        .to_armored_string(ArmorOptions::default())
        .context("armoring public key")?;

    Ok(GeneratedKey {
        private_armored,
        public_armored,
    })
}

/// Load an armored secret key from a string (may be passphrase-encrypted).
pub fn load_secret_key(armored: &str) -> Result<SignedSecretKey> {
    let (key, _headers) =
        SignedSecretKey::from_string(armored).context("parsing armored secret key")?;
    Ok(key)
}

/// Derive the armored public key from a (possibly encrypted) secret key.
pub fn public_from_secret(secret: &SignedSecretKey, passphrase: &str) -> Result<String> {
    let public = secret.public_key();
    let signed_public = public
        .sign(&mut rand::thread_rng(), secret, || passphrase.to_string())
        .context("signing public key (wrong passphrase?)")?;
    signed_public
        .to_armored_string(ArmorOptions::default())
        .context("armoring public key")
}

fn standard_subpackets(key: &SignedSecretKey) -> (Vec<Subpacket>, Vec<Subpacket>) {
    let hashed = vec![
        Subpacket::regular(SubpacketData::IssuerFingerprint(key.fingerprint())),
        Subpacket::regular(SubpacketData::SignatureCreationTime(
            chrono::Utc::now().trunc_subsecs(0),
        )),
    ];
    let unhashed = vec![Subpacket::regular(SubpacketData::Issuer(key.key_id()))];
    (hashed, unhashed)
}

/// Produce an armored **detached** signature over `data`.
///
/// Used for apt `Release.gpg` and yum `repomd.xml.asc`.
pub fn detached_sign(key: &SignedSecretKey, passphrase: &str, data: &[u8]) -> Result<String> {
    let mut config = SignatureConfig::v4(
        SignatureType::Binary,
        key.algorithm(),
        HashAlgorithm::SHA2_256,
    );
    let (hashed, unhashed) = standard_subpackets(key);
    config.hashed_subpackets = hashed;
    config.unhashed_subpackets = unhashed;

    let signature = config
        .sign(key, || passphrase.to_string(), data)
        .context("creating detached signature (wrong passphrase?)")?;
    StandaloneSignature::new(signature)
        .to_armored_string(ArmorOptions::default())
        .context("armoring detached signature")
}

/// Produce an inline **cleartext** signed message over `text`.
///
/// Used for apt `InRelease`. The signature version adapts to the key version
/// (v4 key → v4 signature), keeping it gpg/apt-compatible.
pub fn clearsign(key: &SignedSecretKey, passphrase: &str, text: &str) -> Result<String> {
    let msg =
        CleartextSignedMessage::sign(rand::thread_rng(), text, key, || passphrase.to_string())
            .context("creating cleartext signature (wrong passphrase?)")?;
    msg.to_armored_string(ArmorOptions::default())
        .context("armoring cleartext signature")
}

#[cfg(test)]
mod tests {
    use super::*;

    // RSA-2048 is the minimum rpgp accepts; used here to exercise the
    // sign/verify round-trip wiring.
    fn test_key() -> SignedSecretKey {
        let mut builder = SecretKeyParamsBuilder::default();
        builder
            .key_type(KeyType::Rsa(2048))
            .version(KeyVersion::V4)
            .can_sign(true)
            .can_certify(true)
            .primary_user_id("Test <test@localhost>".to_string());
        let params = builder.build().unwrap();
        let secret = params.generate(rand::thread_rng()).unwrap();
        secret.sign(&mut rand::thread_rng(), String::new).unwrap()
    }

    #[test]
    fn detached_signature_verifies() {
        let key = test_key();
        let data = b"repomd.xml contents";
        let armored = detached_sign(&key, "", data).unwrap();
        assert!(armored.contains("BEGIN PGP SIGNATURE"));

        let (sig, _) = StandaloneSignature::from_string(&armored).unwrap();
        sig.verify(&key.public_key(), data)
            .expect("detached signature must verify");
    }

    #[test]
    fn clearsign_verifies() {
        let key = test_key();
        let armored = clearsign(&key, "", "Origin: ArtifactX\nSuite: stable\n").unwrap();
        assert!(armored.contains("BEGIN PGP SIGNED MESSAGE"));

        let (msg, _) = CleartextSignedMessage::from_string(&armored).unwrap();
        msg.verify(&key.public_key())
            .expect("cleartext signature must verify");
    }

    #[test]
    fn load_roundtrip() {
        let armored = test_key()
            .to_armored_string(ArmorOptions::default())
            .unwrap();
        let loaded = load_secret_key(&armored).unwrap();
        let sig = detached_sign(&loaded, "", b"data").unwrap();
        assert!(sig.contains("BEGIN PGP SIGNATURE"));
    }

    #[test]
    fn encrypted_key_requires_correct_passphrase() {
        let gen = generate_key("Enc <enc@localhost>", "s3cret").unwrap();
        let key = load_secret_key(&gen.private_armored).unwrap();

        // Correct passphrase signs and verifies.
        let armored = detached_sign(&key, "s3cret", b"payload").unwrap();
        let (sig, _) = StandaloneSignature::from_string(&armored).unwrap();
        sig.verify(&key.public_key(), b"payload").unwrap();

        // Wrong passphrase must fail to unlock the key.
        assert!(detached_sign(&key, "wrong", b"payload").is_err());
    }
}
