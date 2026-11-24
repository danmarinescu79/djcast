//! Read-only, in-memory decryption of SQLCipher 4 databases (as used by rekordbox 6/7).
//!
//! We decrypt pages ourselves instead of linking SQLCipher + OpenSSL, which keeps the
//! build dependency-free on Windows and macOS. The plaintext image only lives in memory.
//!
//! SQLCipher 4 defaults:
//! - page size 4096, reserve 80 bytes per page = IV (16) + HMAC-SHA512 (64)
//! - key = PBKDF2-HMAC-SHA512(passphrase, salt, 256000 iterations, 32 bytes)
//! - salt = first 16 bytes of the file (page 1 ciphertext starts after it)
//! - HMAC key = PBKDF2-HMAC-SHA512(key, salt ^ 0x3a, 2 iterations, 32 bytes)
//! - HMAC = HMAC-SHA512(ciphertext || IV || page_number as u32 little-endian)
//! - cipher = AES-256-CBC without padding

use aes::Aes256;
use aes::cipher::{BlockCipherDecrypt, KeyInit};
use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use sha2::Sha512;

pub const PAGE_SIZE: usize = 4096;
const SALT_LEN: usize = 16;
const IV_LEN: usize = 16;
const HMAC_LEN: usize = 64;
const RESERVE: usize = IV_LEN + HMAC_LEN;
const KDF_ITER: u32 = 256_000;
const HMAC_SALT_MASK: u8 = 0x3a;
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

const WAL_HEADER_LEN: usize = 32;
const WAL_FRAME_HEADER_LEN: usize = 24;

/// Keys derived for one database file (derivation is slow, so cache this per salt).
#[derive(Clone)]
pub struct Keys {
    salt: [u8; SALT_LEN],
    aes: Aes256,
    hmac_key: [u8; 32],
}

impl Keys {
    pub fn derive(passphrase: &str, salt: [u8; SALT_LEN]) -> Self {
        let key: [u8; 32] = pbkdf2::pbkdf2_hmac_array::<Sha512, 32>(passphrase.as_bytes(), &salt, KDF_ITER);
        let hmac_salt = salt.map(|b| b ^ HMAC_SALT_MASK);
        let hmac_key: [u8; 32] = pbkdf2::pbkdf2_hmac_array::<Sha512, 32>(&key, &hmac_salt, 2);
        let aes = Aes256::new(&key.into());
        Self { salt, aes, hmac_key }
    }

    pub fn salt(&self) -> [u8; SALT_LEN] {
        self.salt
    }

    /// Decrypt one page in place-ish: returns the plaintext page (same size, reserve zeroed).
    /// `pgno` is 1-based.
    fn decrypt_page(&self, page: &[u8], pgno: u32) -> Result<Vec<u8>> {
        if page.len() != PAGE_SIZE {
            bail!("page {pgno}: unexpected size {}", page.len());
        }
        let start = if pgno == 1 { SALT_LEN } else { 0 };
        let data_end = PAGE_SIZE - RESERVE;
        let cipher = &page[start..data_end];
        let iv = &page[data_end..data_end + IV_LEN];
        let tag = &page[data_end + IV_LEN..data_end + IV_LEN + HMAC_LEN];

        let mut mac = <Hmac<Sha512> as hmac::KeyInit>::new_from_slice(&self.hmac_key).expect("any key length");
        mac.update(cipher);
        mac.update(iv);
        mac.update(&pgno.to_le_bytes());
        if mac.verify_slice(tag).is_err() {
            bail!("page {pgno}: HMAC mismatch (wrong key or page being written)");
        }

        // AES-256-CBC decrypt (no padding).
        let mut blocks: Vec<aes::Block> = cipher.as_chunks::<16>().0.iter().map(|c| aes::Block::from(*c)).collect();
        self.aes.decrypt_blocks(&mut blocks);

        let mut out = vec![0u8; PAGE_SIZE];
        if pgno == 1 {
            out[..SALT_LEN].copy_from_slice(SQLITE_HEADER);
        }
        let mut prev = iv;
        for (i, block) in blocks.iter().enumerate() {
            let off = start + i * 16;
            let ct = &cipher[i * 16..i * 16 + 16];
            for j in 0..16 {
                out[off + j] = block[j] ^ prev[j];
            }
            prev = ct;
        }
        Ok(out)
    }
}

/// Salt of an encrypted database file (its first 16 bytes).
pub fn read_salt(db: &[u8]) -> Result<[u8; SALT_LEN]> {
    if db.len() < PAGE_SIZE {
        bail!("file too small to be a SQLCipher database");
    }
    if db.starts_with(SQLITE_HEADER) {
        bail!("database is not encrypted");
    }
    Ok(db[..SALT_LEN].try_into().unwrap())
}

/// Decrypt a whole database image, then apply committed frames from its WAL (if any).
pub fn decrypt(keys: &Keys, db: &[u8], wal: Option<&[u8]>) -> Result<Vec<u8>> {
    if !db.len().is_multiple_of(PAGE_SIZE) {
        bail!("database size {} is not a multiple of the page size", db.len());
    }
    let mut image = Vec::with_capacity(db.len());
    for (i, page) in db.as_chunks::<PAGE_SIZE>().0.iter().enumerate() {
        image.extend(keys.decrypt_page(page, i as u32 + 1)?);
    }
    if let Some(wal) = wal {
        apply_wal(keys, &mut image, wal).context("applying WAL")?;
    }
    // Header bytes 18/19 = file format read/write version; 2 means WAL mode, which an
    // in-memory (deserialized) database cannot use. Switch the copy to rollback mode.
    image[18] = 1;
    image[19] = 1;
    Ok(image)
}

/// Apply all *committed* WAL frames. Frames after the last commit (a transaction in
/// progress) and stale frames from before the last checkpoint (salt mismatch) are ignored.
fn apply_wal(keys: &Keys, image: &mut Vec<u8>, wal: &[u8]) -> Result<()> {
    if wal.len() < WAL_HEADER_LEN {
        return Ok(()); // empty / just reset
    }
    let be = |b: &[u8]| u32::from_be_bytes(b.try_into().unwrap());
    let magic = be(&wal[0..4]);
    if magic & 0xFFFF_FFFE != 0x377F_0682 {
        bail!("bad WAL magic {magic:08x}");
    }
    let page_size = be(&wal[8..12]) as usize;
    if page_size != PAGE_SIZE {
        bail!("unexpected WAL page size {page_size}");
    }
    let salt = &wal[16..24];

    let frame_len = WAL_FRAME_HEADER_LEN + PAGE_SIZE;
    let mut pending: Vec<(u32, &[u8])> = Vec::new();
    let mut off = WAL_HEADER_LEN;
    while off + frame_len <= wal.len() {
        let hdr = &wal[off..off + WAL_FRAME_HEADER_LEN];
        if &hdr[8..16] != salt {
            break; // leftover from an earlier WAL generation
        }
        let pgno = be(&hdr[0..4]);
        let commit_size = be(&hdr[4..8]);
        pending.push((pgno, &wal[off + WAL_FRAME_HEADER_LEN..off + frame_len]));
        if commit_size > 0 {
            for (pgno, page) in pending.drain(..) {
                let plain = keys.decrypt_page(page, pgno)?;
                let start = (pgno as usize - 1) * PAGE_SIZE;
                if image.len() < start + PAGE_SIZE {
                    image.resize(start + PAGE_SIZE, 0);
                }
                image[start..start + PAGE_SIZE].copy_from_slice(&plain);
            }
            image.truncate(commit_size as usize * PAGE_SIZE);
        }
        off += frame_len;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encrypt a page the way SQLCipher does, to test decryption round-trips.
    fn encrypt_page(keys: &Keys, plain: &[u8], pgno: u32, iv: [u8; 16]) -> Vec<u8> {
        use aes::cipher::BlockCipherEncrypt;
        let start = if pgno == 1 { SALT_LEN } else { 0 };
        let data_end = PAGE_SIZE - RESERVE;
        let mut out = vec![0u8; PAGE_SIZE];
        if pgno == 1 {
            out[..SALT_LEN].copy_from_slice(&keys.salt);
        }
        let mut prev = iv;
        for off in (start..data_end).step_by(16) {
            let mut b = [0u8; 16];
            for j in 0..16 {
                b[j] = plain[off + j] ^ prev[j];
            }
            let mut block = aes::Block::from(b);
            keys.aes.encrypt_block(&mut block);
            out[off..off + 16].copy_from_slice(&block);
            prev = block.into();
        }
        out[data_end..data_end + IV_LEN].copy_from_slice(&iv);
        let mut mac = <Hmac<Sha512> as hmac::KeyInit>::new_from_slice(&keys.hmac_key).unwrap();
        mac.update(&out[start..data_end]);
        mac.update(&iv);
        mac.update(&pgno.to_le_bytes());
        out[data_end + IV_LEN..].copy_from_slice(&mac.finalize().into_bytes());
        out
    }

    fn test_keys() -> Keys {
        Keys::derive("test-passphrase", [7u8; 16])
    }

    fn plain_page(fill: u8, pgno: u32) -> Vec<u8> {
        let mut p = vec![fill; PAGE_SIZE];
        if pgno == 1 {
            p[..16].copy_from_slice(SQLITE_HEADER);
        }
        p[PAGE_SIZE - RESERVE..].fill(0);
        p
    }

    #[test]
    fn decrypts_pages_and_restores_header() {
        let k = test_keys();
        let p1 = plain_page(0xAB, 1);
        let p2 = plain_page(0xCD, 2);
        let mut db = encrypt_page(&k, &p1, 1, [1; 16]);
        db.extend(encrypt_page(&k, &p2, 2, [2; 16]));
        let img = decrypt(&k, &db, None).unwrap();
        let mut expected_p1 = p1.clone();
        expected_p1[18] = 1; // WAL flag cleared for in-memory use
        expected_p1[19] = 1;
        assert_eq!(&img[..PAGE_SIZE], &expected_p1[..]);
        assert_eq!(&img[PAGE_SIZE..], &p2[..]);
    }

    #[test]
    fn wrong_key_fails_hmac() {
        let k = test_keys();
        let db = encrypt_page(&k, &plain_page(1, 1), 1, [1; 16]);
        let wrong = Keys::derive("nope", k.salt());
        assert!(decrypt(&wrong, &db, None).unwrap_err().to_string().contains("HMAC"));
    }

    #[test]
    fn wal_applies_only_committed_frames() {
        let k = test_keys();
        let mut db = encrypt_page(&k, &plain_page(1, 1), 1, [1; 16]);
        db.extend(encrypt_page(&k, &plain_page(2, 2), 2, [2; 16]));

        let salt = [9u8, 9, 9, 9, 8, 8, 8, 8];
        let mut wal = vec![0u8; WAL_HEADER_LEN];
        wal[0..4].copy_from_slice(&0x377F_0682u32.to_be_bytes());
        wal[8..12].copy_from_slice(&(PAGE_SIZE as u32).to_be_bytes());
        wal[16..24].copy_from_slice(&salt);
        let mut frame = |pgno: u32, commit: u32, fill: u8, salt: [u8; 8]| {
            let mut h = vec![0u8; WAL_FRAME_HEADER_LEN];
            h[0..4].copy_from_slice(&pgno.to_be_bytes());
            h[4..8].copy_from_slice(&commit.to_be_bytes());
            h[8..16].copy_from_slice(&salt);
            wal.extend(h);
            wal.extend(encrypt_page(&k, &plain_page(fill, pgno), pgno, [fill; 16]));
        };
        frame(2, 0, 0x22, salt); // part of txn 1
        frame(3, 3, 0x33, salt); // commit txn 1: db grows to 3 pages
        frame(2, 0, 0x44, salt); // uncommitted: must be ignored
        frame(2, 3, 0x55, [0; 8]); // stale generation: must be ignored

        let img = decrypt(&k, &db, Some(&wal)).unwrap();
        assert_eq!(img.len(), 3 * PAGE_SIZE);
        assert_eq!(img[PAGE_SIZE], 0x22);
        assert_eq!(img[2 * PAGE_SIZE], 0x33);
    }
}
