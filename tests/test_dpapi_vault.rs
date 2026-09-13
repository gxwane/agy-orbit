use agy_orbit::infra::crypto::create_default_vault;

#[test]
fn test_vault_roundtrip_unicode_and_large_payload() {
    let vault = create_default_vault();

    // 1. Test Unicode and JSON payloads
    let plaintext = r#"{"refresh_token": "ya29.a0AfH6SM...日本語测试🔐", "client_id": "google"}"#;
    let sealed = vault.seal(plaintext.as_bytes()).expect("Sealing failed");
    assert_ne!(sealed, plaintext.as_bytes());

    let unsealed = vault.unseal(&sealed).expect("Unsealing failed");
    assert_eq!(String::from_utf8(unsealed).unwrap(), plaintext);

    // 2. Test large binary payload (e.g. 16KB token/certificate)
    let large_data: Vec<u8> = (0..16384).map(|i| (i % 255) as u8).collect();
    let large_sealed = vault.seal(&large_data).expect("Large seal failed");
    let large_unsealed = vault.unseal(&large_sealed).expect("Large unseal failed");
    assert_eq!(large_unsealed, large_data);
}

#[test]
fn test_vault_tampering_rejection() {
    let vault = create_default_vault();
    let plaintext = b"secret-token";
    let mut sealed = vault.seal(plaintext).expect("Sealing failed");

    // Corrupt the ciphertext
    if !sealed.is_empty() {
        let len = sealed.len();
        sealed[len / 2] ^= 0xFF;
    }

    let unsealed_result = vault.unseal(&sealed);
    assert!(
        unsealed_result.is_err(),
        "Tampered ciphertext must be rejected"
    );
}
