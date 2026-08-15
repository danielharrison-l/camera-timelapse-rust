use anyhow::{anyhow, Context, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

pub struct RtspCipher;

impl RtspCipher {
    /// Descriptografa uma string cifrada (formato NestJS `v1:iv:tag:ciphertext` em Base64 ou formato legado em Hex)
    /// utilizando a chave AES-256-GCM (`key_hex` de 64 caracteres hexadecimais).
    pub fn decrypt(cipher_text: &str, key_hex: &str) -> Result<String> {
        let key_bytes = hex::decode(key_hex.trim())
            .context("RTSP_ENC_KEY precisa ser uma string hexadecimal válida de 64 caracteres")?;

        if key_bytes.len() != 32 {
            return Err(anyhow!(
                "RTSP_ENC_KEY deve conter exatamente 64 caracteres hexadecimais (32 bytes). Tamanho atual: {} bytes",
                key_bytes.len()
            ));
        }

        let parts: Vec<&str> = cipher_text.split(':').collect();

        let (iv_bytes, tag_bytes, encrypted_bytes) = if parts.len() == 4 && parts[0] == "v1" {
            // Formato v1 (Base64)
            let iv = BASE64.decode(parts[1]).context("Falha ao decodificar IV Base64")?;
            let tag = BASE64.decode(parts[2]).context("Falha ao decodificar Auth Tag Base64")?;
            let encrypted = BASE64.decode(parts[3]).context("Falha ao decodificar Ciphertext Base64")?;
            (iv, tag, encrypted)
        } else if parts.len() == 3 {
            // Formato legado (Hex)
            let iv = hex::decode(parts[0]).context("Falha ao decodificar IV Hex legado")?;
            let tag = hex::decode(parts[1]).context("Falha ao decodificar Auth Tag Hex legado")?;
            let encrypted = hex::decode(parts[2]).context("Falha ao decodificar Ciphertext Hex legado")?;
            (iv, tag, encrypted)
        } else {
            return Err(anyhow!("Formato de credencial cifrada inválido: '{}'", cipher_text));
        };

        if iv_bytes.len() != 12 {
            return Err(anyhow!("Tamanho de IV inválido (esperado 12 bytes, recebido {})", iv_bytes.len()));
        }

        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| anyhow!("Falha ao inicializar Aes256Gcm: {}", e))?;

        let nonce = Nonce::from_slice(&iv_bytes);

        // aes-gcm crate espera o ciphertext concatenado com a tag (tag ao final)
        let mut ciphertext_with_tag = encrypted_bytes;
        ciphertext_with_tag.extend_from_slice(&tag_bytes);

        let decrypted_bytes = cipher
            .decrypt(nonce, ciphertext_with_tag.as_ref())
            .map_err(|e| anyhow!("Falha na descriptografia AES-256-GCM (chave incorreta ou dado corrompido): {}", e))?;

        let plain_text = String::from_utf8(decrypted_bytes)
            .context("Texto descriptografado não é uma string UTF-8 válida")?;

        Ok(plain_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decrypt_v1_format() {
        // Chave de teste de 64 caracteres hexadecimais (32 bytes de zeros)
        let key_hex = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        
        // Simulação de criptografia compatível com RtspCipher NestJS
        // Texto original: "senhaGaragem456!"
        // IV (12 bytes): 00 01 02 03 04 05 06 07 08 09 0a 0b -> Base64: AAECAwQFBgcICQoL
        let key_bytes = hex::decode(key_hex).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key_bytes).unwrap();
        let iv = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let nonce = Nonce::from_slice(&iv);
        let plain = b"senhaGaragem456!";
        
        let encrypted_vec = cipher.encrypt(nonce, plain.as_ref()).unwrap();
        let (ciphertext, tag) = encrypted_vec.split_at(plain.len());

        let iv_b64 = BASE64.encode(iv);
        let tag_b64 = BASE64.encode(tag);
        let ciphertext_b64 = BASE64.encode(ciphertext);

        let formatted = format!("v1:{}:{}:{}", iv_b64, tag_b64, ciphertext_b64);

        let decrypted = RtspCipher::decrypt(&formatted, key_hex).unwrap();
        assert_eq!(decrypted, "senhaGaragem456!");
    }
}
