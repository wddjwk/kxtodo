//! 同步密码学：Argon2id 派生 + HKDF 分离认证/加密密钥 + XChaCha20-Poly1305 实体加密。
//!
//! 安全模型（Etebase 式）：
//! - `auth_key`：登录证明（HMAC 挑战应答），服务器持有同值；
//! - `enc_key`：实体加密密钥，永不出设备，服务器只见密文；
//! - 抓包/拖库均无法还原明文；纯 HTTP 下残留风险 = token 被嗅探后冒写密文（无明文泄露）。

use argon2::Argon2;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};

pub const NONCE_BYTES: usize = 24;

#[derive(Debug, Clone)]
pub struct SyncKeys {
    pub auth_key: [u8; 32],
    pub enc_key: [u8; 32],
}

/// 确定性盐：同一 (username, secret) 在任意设备派生出相同密钥。
/// 账户标识就是用户名（v0.5.1 起不再拼邮箱），所以盐里也只有用户名。
fn derive_salt(username: &str) -> String {
    format!("kxtodo|{}", username.trim().to_lowercase())
}

/// 派生结果进程内缓存：Argon2id 单次约 1s，而自动同步最短 5s 一轮，
/// 每轮重新派生纯属白烧 CPU（移动端更明显）。键是 sha256(salt|secret)，不落明文。
fn key_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, SyncKeys>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, SyncKeys>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn derive_keys(username: &str, secret: &str) -> CoreResult<SyncKeys> {
    if username.trim().is_empty() {
        return Err(CoreError::validation(
            "SYNC_USERNAME_REQUIRED",
            "同步用户名不能为空",
        ));
    }
    if secret.trim().is_empty() {
        return Err(CoreError::validation(
            "SYNC_SECRET_REQUIRED",
            "同步密钥不能为空",
        ));
    }
    let salt = derive_salt(username);
    let cache_key = sha256_hex(format!("{salt}\u{0}{secret}").as_bytes());
    if let Ok(cache) = key_cache().lock() {
        if let Some(keys) = cache.get(&cache_key) {
            return Ok(keys.clone());
        }
    }
    let mut master = [0u8; 32];
    Argon2::default()
        .hash_password_into(secret.as_bytes(), salt.as_bytes(), &mut master)
        .map_err(|error| {
            CoreError::internal(format!("Argon2id 密钥派生失败：{error}"))
        })?;
    let keys = SyncKeys {
        auth_key: hkdf_sha256(&master, b"kxtodo-auth-v1"),
        enc_key: hkdf_sha256(&master, b"kxtodo-enc-v1"),
    };
    master.iter_mut().for_each(|byte| *byte = 0);
    if let Ok(mut cache) = key_cache().lock() {
        cache.insert(cache_key, keys.clone());
    }
    Ok(keys)
}

fn hkdf_sha256(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = hkdf::Hkdf::<Sha256>::new(None, ikm);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .expect("HKDF-SHA256 输出 32 字节必然成功");
    okm
}

pub fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn to_hex(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    to_hex(&buf)
}

/// 加密一个同步实体。AAD 绑定实体 ID：服务器/中间人无法把 A 的密文嫁接到 B。
pub fn seal_entity(
    enc_key: &[u8; 32],
    entity_id: &str,
    plaintext: &str,
) -> CoreResult<(String, String)> {
    let (nonce, cipher) = seal_bytes(enc_key, entity_id, plaintext.as_bytes())?;
    Ok((B64.encode(nonce), B64.encode(cipher)))
}

/// 加密任意字节（图片等大对象走裸字节体，不做 base64 膨胀）。
/// 返回 (nonce, 密文)；AAD 同样绑定实体 ID。
pub fn seal_bytes(enc_key: &[u8; 32], entity_id: &str, plaintext: &[u8]) -> CoreResult<(Vec<u8>, Vec<u8>)> {
    let mut nonce_bytes = [0u8; NONCE_BYTES];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let cipher = XChaCha20Poly1305::new(Key::from_slice(enc_key))
        .encrypt(
            XNonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: entity_id.as_bytes(),
            },
        )
        .map_err(|_| CoreError::internal("实体加密失败"))?;
    Ok((nonce_bytes.to_vec(), cipher))
}

/// 解密任意字节；AAD 不匹配或密文被篡改时直接报错。
pub fn open_bytes(
    enc_key: &[u8; 32],
    entity_id: &str,
    nonce: &[u8],
    cipher: &[u8],
) -> CoreResult<Vec<u8>> {
    if nonce.len() != NONCE_BYTES {
        return Err(CoreError::io("实体 nonce 长度无效"));
    }
    XChaCha20Poly1305::new(Key::from_slice(enc_key))
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: cipher,
                aad: entity_id.as_bytes(),
            },
        )
        .map_err(|_| {
            CoreError::new(
                crate::error::ErrorKind::Io,
                "ENTITY_DECRYPT_FAILED",
                format!("实体 {entity_id} 解密失败（密钥不符或数据被篡改）"),
            )
        })
}

/// 解密一个同步实体；AAD 不匹配或密文被篡改时直接报错。
pub fn open_entity(
    enc_key: &[u8; 32],
    entity_id: &str,
    nonce_b64: &str,
    cipher_b64: &str,
) -> CoreResult<String> {
    let nonce = B64
        .decode(nonce_b64)
        .map_err(|_| CoreError::io("实体 nonce 不是合法 base64"))?;
    let cipher = B64
        .decode(cipher_b64)
        .map_err(|_| CoreError::io("实体密文不是合法 base64"))?;
    let plain = open_bytes(enc_key, entity_id, &nonce, &cipher)?;
    String::from_utf8(plain).map_err(|_| CoreError::io("实体明文不是合法 UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_deterministic_and_separated() {
        let a = derive_keys("user", "secret-1").unwrap();
        let b = derive_keys("USER", "secret-1").unwrap();
        assert_eq!(a.auth_key, b.auth_key, "用户名大小写不敏感");
        assert_eq!(a.enc_key, b.enc_key);
        let c = derive_keys("user", "secret-2").unwrap();
        assert_ne!(a.auth_key, c.auth_key);
        let d = derive_keys("other", "secret-1").unwrap();
        assert_ne!(a.auth_key, d.auth_key, "不同用户名必须是不同账户");
        assert_ne!(a.auth_key, a.enc_key, "认证与加密密钥必须分离");
        assert!(derive_keys("", "secret-1").is_err());
        assert!(derive_keys("user", " ").is_err());
    }

    #[test]
    fn seal_open_roundtrip_and_tamper() {
        let keys = derive_keys("u", "s").unwrap();
        let (nonce, cipher) = seal_entity(&keys.enc_key, "task-abc", "{\"a\":1}").unwrap();
        let plain = open_entity(&keys.enc_key, "task-abc", &nonce, &cipher).unwrap();
        assert_eq!(plain, "{\"a\":1}");

        // 换实体 ID（AAD 不匹配）→ 拒绝
        assert!(open_entity(&keys.enc_key, "task-xyz", &nonce, &cipher).is_err());
        // 换密钥 → 拒绝
        let other = derive_keys("u2", "s").unwrap();
        assert!(open_entity(&other.enc_key, "task-abc", &nonce, &cipher).is_err());
        // 篡改密文 → 拒绝
        let mut bytes = cipher.clone();
        bytes.replace_range(4..5, if &cipher[4..5] == "A" { "B" } else { "A" });
        assert!(open_entity(&keys.enc_key, "task-abc", &nonce, &bytes).is_err());
    }
}
