//! Encryption and decryption utilities for Webex content.
//!
//! This module handles decryption of JWE-encrypted content such as reactions.
//! Webex uses AES-256-GCM encryption with keys managed by the KMS (Key Management Service).

use crate::error::Error;
use base64::Engine;
use log::*;
use serde::{Deserialize, Serialize};

/// Represents a JWE (JSON Web Encryption) token header
#[derive(Debug, Deserialize, Serialize)]
pub struct JweHeader {
    /// Algorithm used for key encryption/agreement
    alg: String,
    /// Key ID pointing to the KMS key
    kid: String,
    /// Content encryption algorithm
    enc: String,
}

/// Represents an encrypted content object from Webex
#[derive(Debug)]
pub struct EncryptedContent {
    /// The JWE token containing the encrypted data
    pub jwe_token: String,
    /// The KMS URL for key retrieval
    pub encryption_key_url: String,
}

impl EncryptedContent {
    /// Create a new `EncryptedContent` from a JWE token and encryption key URL
    #[must_use]
    pub fn new(jwe_token: String, encryption_key_url: String) -> Self {
        Self {
            jwe_token,
            encryption_key_url,
        }
    }

    /// Parse the JWE header to extract encryption metadata
    ///
    /// # Errors
    ///
    /// Returns an error if the JWE token is malformed or cannot be parsed
    pub fn parse_header(&self) -> Result<JweHeader, Error> {
        // JWE Compact Serialization format: header.encrypted_key.iv.ciphertext.tag
        let parts: Vec<&str> = self.jwe_token.split('.').collect();
        if parts.is_empty() {
            return Err("Invalid JWE token: no parts found".into());
        }

        // Decode the header (first part) - uses base64url encoding per JWE spec
        let header_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|e| format!("Failed to decode JWE header: {e}"))?;

        let header_str = std::str::from_utf8(&header_bytes)
            .map_err(|e| format!("Invalid UTF-8 in JWE header: {e}"))?;

        let header: JweHeader = serde_json::from_str(header_str)
            .map_err(|e| format!("Failed to parse JWE header: {e}"))?;

        debug!("Parsed JWE header: alg={}, enc={}, kid={}", header.alg, header.enc, header.kid);

        Ok(header)
    }

    /// Extract the encryption components from the JWE token
    ///
    /// Returns (encrypted_key, iv, ciphertext, tag) if successful
    ///
    /// # Errors
    ///
    /// Returns an error if the JWE token is malformed
    pub fn extract_components(&self) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>), Error> {
        let parts: Vec<&str> = self.jwe_token.split('.').collect();
        if parts.len() != 5 {
            return Err(format!("Invalid JWE token: expected 5 parts, got {}", parts.len()).into());
        }

        // JWE uses base64url encoding (not standard base64)
        let encrypted_key = if parts[1].is_empty() {
            // Direct key agreement uses empty encrypted_key
            Vec::new()
        } else {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[1])
                .map_err(|e| format!("Failed to decode encrypted key: {e}"))?
        };

        let iv = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|e| format!("Failed to decode IV: {e}"))?;

        let ciphertext = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[3])
            .map_err(|e| format!("Failed to decode ciphertext: {e}"))?;

        let tag = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[4])
            .map_err(|e| format!("Failed to decode authentication tag: {e}"))?;

        debug!(
            "Extracted JWE components: encrypted_key_len={}, iv_len={}, ciphertext_len={}, tag_len={}",
            encrypted_key.len(),
            iv.len(),
            ciphertext.len(),
            tag.len()
        );

        Ok((encrypted_key, iv, ciphertext, tag))
    }
}

/// Decryption service for handling KMS-encrypted content
pub struct DecryptionService {
    /// HTTP client for making KMS requests
    client: reqwest::Client,
    /// Bearer token for authentication
    token: String,
}

impl DecryptionService {
    /// Create a new `DecryptionService` with the given authentication token
    #[must_use]
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
        }
    }

    /// Fetch the decryption key from KMS
    ///
    /// # Errors
    ///
    /// Returns an error if the KMS request fails or the response is invalid
    pub async fn fetch_key_from_kms(&self, kms_url: &str) -> Result<Vec<u8>, Error> {
        debug!("Fetching decryption key from KMS: {kms_url}");

        // The KMS URL format is kms://kms-cisco.wbx2.com/keys/{key_id}
        // We need to convert this to an HTTPS URL
        let https_url = kms_url
            .replace("kms://", "https://")
            .replace("/keys/", "/encryption/api/v1/keys/");

        debug!("Converted KMS URL to HTTPS: {https_url}");

        let response = self
            .client
            .get(&https_url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| format!("KMS request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(format!("KMS request failed with status {status}: {error_text}").into());
        }

        let key_data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse KMS response: {e}"))?;

        debug!("KMS response: {key_data:#?}");

        // Extract the key from the response
        // The actual structure will depend on the KMS API response format
        // This is a placeholder that needs to be updated based on actual KMS responses
        let key_str = key_data
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or("Key not found in KMS response")?;

        let key_bytes = base64::engine::general_purpose::STANDARD
            .decode(key_str)
            .map_err(|e| format!("Failed to decode key from KMS: {e}"))?;

        Ok(key_bytes)
    }

    /// Decrypt content using AES-256-GCM
    ///
    /// # Errors
    ///
    /// Returns an error if decryption fails
    pub fn decrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        ciphertext: &[u8],
        tag: &[u8],
    ) -> Result<Vec<u8>, Error> {
        use aes_gcm::{
            aead::{Aead, KeyInit, Payload},
            Aes256Gcm, Nonce,
        };

        debug!("Decrypting with AES-256-GCM: key_len={}, iv_len={}, ciphertext_len={}, tag_len={}",
            key.len(), iv.len(), ciphertext.len(), tag.len());

        // AES-GCM expects the tag to be appended to the ciphertext
        let mut ciphertext_with_tag = ciphertext.to_vec();
        ciphertext_with_tag.extend_from_slice(tag);

        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| format!("Failed to create cipher: {e}"))?;

        // IV must be exactly 12 bytes for AES-GCM
        let iv_array: [u8; 12] = iv
            .try_into()
            .map_err(|_| format!("IV must be exactly 12 bytes, got {}", iv.len()))?;
        let nonce = Nonce::from(iv_array);

        let payload = Payload {
            msg: &ciphertext_with_tag,
            aad: b"", // Additional authenticated data (empty for now)
        };

        let plaintext = cipher
            .decrypt(&nonce, payload)
            .map_err(|e| format!("Decryption failed: {e}"))?;

        Ok(plaintext)
    }

    /// Decrypt a JWE-encrypted reaction or other content
    ///
    /// # Errors
    ///
    /// Returns an error if any step of the decryption process fails
    pub async fn decrypt(&self, encrypted: &EncryptedContent) -> Result<String, Error> {
        info!("Decrypting JWE content");

        // Parse the JWE header
        let header = encrypted.parse_header()?;

        // Verify algorithm and encryption method
        if header.enc != "A256GCM" {
            return Err(format!("Unsupported encryption algorithm: {}", header.enc).into());
        }

        // Extract JWE components
        let (_encrypted_key, iv, ciphertext, tag) = encrypted.extract_components()?;

        // Fetch the decryption key from KMS
        let key = self.fetch_key_from_kms(&encrypted.encryption_key_url).await?;

        // Decrypt the content
        let plaintext_bytes = self.decrypt_aes_gcm(&key, &iv, &ciphertext, &tag)?;

        // Convert to UTF-8 string
        let plaintext = String::from_utf8(plaintext_bytes)
            .map_err(|e| format!("Decrypted data is not valid UTF-8: {e}"))?;

        info!("Successfully decrypted content: {plaintext}");

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jwe_header() {
        let jwe_token = "eyJhbGciOiAiZGlyIiwgImtpZCI6ICJrbXM6Ly9rbXMtY2lzY28ud2J4Mi5jb20va2V5cy83NGM1MDU1OS0xYTViLTRhYjAtYmNhOC04ZmVlNjI4ZTI0N2QiLCAiZW5jIjogIkEyNTZHQ00ifQ..cC--iBIY0QAEONFj.-7m5Rg.0H2qWrKnJ9xIr6yKBrA5nw";
        let encrypted = EncryptedContent::new(
            jwe_token.to_string(),
            "kms://kms-cisco.wbx2.com/keys/74c50559-1a5b-4ab0-bca8-8fee628e247d".to_string(),
        );

        let header = encrypted.parse_header().unwrap();
        assert_eq!(header.alg, "dir");
        assert_eq!(header.enc, "A256GCM");
        assert_eq!(
            header.kid,
            "kms://kms-cisco.wbx2.com/keys/74c50559-1a5b-4ab0-bca8-8fee628e247d"
        );
    }

    #[test]
    fn test_extract_components() {
        let jwe_token = "eyJhbGciOiAiZGlyIiwgImtpZCI6ICJrbXM6Ly9rbXMtY2lzY28ud2J4Mi5jb20va2V5cy83NGM1MDU1OS0xYTViLTRhYjAtYmNhOC04ZmVlNjI4ZTI0N2QiLCAiZW5jIjogIkEyNTZHQ00ifQ..cC--iBIY0QAEONFj.-7m5Rg.0H2qWrKnJ9xIr6yKBrA5nw";
        let encrypted = EncryptedContent::new(
            jwe_token.to_string(),
            "kms://kms-cisco.wbx2.com/keys/74c50559-1a5b-4ab0-bca8-8fee628e247d".to_string(),
        );

        let result = encrypted.extract_components();
        assert!(result.is_ok());

        let (encrypted_key, iv, ciphertext, tag) = result.unwrap();
        assert_eq!(encrypted_key.len(), 0); // Direct key agreement
        assert!(iv.len() > 0);
        assert!(ciphertext.len() > 0);
        assert!(tag.len() > 0);
    }
}
