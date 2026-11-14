//! Cryptographic hashing of images and carved files, and verification of extracted output
//! against the source image.

use crate::extract::{INDEX_NAME, IndexRow, read_index};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::Path;

/// Hex digests of a byte slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digests {
    pub blake3: String,
    pub sha256: String,
}

pub fn digest(data: &[u8]) -> Digests {
    Digests {
        blake3: blake3::hash(data).to_hex().to_string(),
        sha256: hex(&Sha256::digest(data)),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Carved file is byte-identical to the image range it came from.
    Match,
    /// Carved file differs from the image range, or the range lies outside the image.
    Mismatch,
    /// The carved file is missing from the output directory.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRow {
    pub id: u64,
    pub file: String,
    pub outcome: Outcome,
}

/// Checks every entry in `<dir>/index.csv` against `image`.
pub fn verify_output(image: &[u8], dir: &Path) -> io::Result<Vec<VerifyRow>> {
    let rows = read_index(&dir.join(INDEX_NAME))?;
    Ok(rows.iter().map(|row| verify_row(image, dir, row)).collect())
}

fn verify_row(image: &[u8], dir: &Path, row: &IndexRow) -> VerifyRow {
    let outcome = match fs::read(dir.join(&row.file)) {
        Err(_) => Outcome::Missing,
        Ok(carved) => match image.get(row.start_offset as usize..row.end_offset as usize) {
            Some(range) if range == carved.as_slice() => Outcome::Match,
            _ => Outcome::Mismatch,
        },
    };
    VerifyRow {
        id: row.id,
        file: row.file.clone(),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        let d = digest(b"abc");
        assert_eq!(
            d.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(d.blake3.len(), 64);
    }
}
