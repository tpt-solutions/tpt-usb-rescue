//! Built-in structural validators.
//!
//! Each validator runs a small set of format checks and returns the fraction that pass. A file
//! scoring 1.0 is structurally intact; anything lower is either corrupted or a false-positive
//! header match. The carver uses the score to reject or flag candidates.

use crate::signature::{FileType, find, walk_gif, walk_jpeg, walk_png, walk_zip};

/// Validates carved bytes.
pub trait MediaValidator {
    /// Returns an integrity score in 0.0..=1.0.
    fn validate(&self, file_type: FileType, data: &[u8]) -> f32;
}

/// Structural validators for the built-in file types.
#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinValidator;

impl MediaValidator for BuiltinValidator {
    fn validate(&self, file_type: FileType, data: &[u8]) -> f32 {
        let checks: &[bool] = &match file_type {
            FileType::Jpeg => {
                let w = walk_jpeg(data);
                [
                    data.starts_with(&[0xFF, 0xD8]),
                    w.has_dqt,
                    w.has_sof,
                    w.has_dht,
                    w.has_sos,
                    w.end.is_some(),
                    w.bad_segments == 0,
                ]
                .to_vec()
            }
            FileType::Png => {
                let w = walk_png(data);
                let ok = w.chunks > 0 && w.bad_crc == 0;
                vec![
                    data.starts_with(&[0x89, b'P', b'N', b'G']),
                    w.first_is_ihdr,
                    w.has_idat,
                    ok,
                    w.has_iend,
                ]
            }
            FileType::Pdf => {
                let tail = &data[data.len().saturating_sub(1024)..];
                let startxref_ok = startxref_target_ok(data);
                vec![
                    data.starts_with(b"%PDF-"),
                    find(tail, b"%%EOF").is_some(),
                    startxref_ok,
                ]
            }
            FileType::Gif => {
                let w = walk_gif(data);
                vec![
                    data.starts_with(b"GIF8"),
                    w.end.is_some(),
                    w.blocks > 0,
                    w.sub_block_errors == 0,
                ]
            }
            FileType::Zip => {
                let w = walk_zip(data);
                vec![data.starts_with(b"PK\x03\x04"), w.eocd.is_some(), w.cd_ok]
            }
        };
        if checks.is_empty() {
            return 0.0;
        }
        let passed = checks.iter().filter(|&&c| c).count();
        passed as f32 / checks.len() as f32
    }
}

/// Checks that the last `startxref` value points at an `xref` table or an indirect object.
fn startxref_target_ok(data: &[u8]) -> bool {
    let tail = &data[data.len().saturating_sub(4096)..];
    let Some(pos) = find(tail, b"startxref") else {
        return false;
    };
    let digits: Vec<u8> = tail[pos + 9..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .copied()
        .collect();
    let Some(offset) = std::str::from_utf8(&digits)
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    else {
        return false;
    };
    match data.get(offset..) {
        Some(rest) => rest.starts_with(b"xref") || rest.first().is_some_and(|b| b.is_ascii_digit()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_after_jpeg_header_scores_low() {
        let junk = [0xFF, 0xD8, 0xFF, 0x00, 0x01, 0x02, 0x03, 0x04];
        let score = BuiltinValidator.validate(FileType::Jpeg, &junk);
        assert!(score < 0.5, "score {score}");
    }

    #[test]
    fn png_with_bad_crc_is_not_intact() {
        let mut d = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        d.extend_from_slice(&[0, 0, 0, 0]);
        d.extend_from_slice(b"IHDR");
        d.extend_from_slice(&[0, 0, 0, 0]); // wrong CRC
        d.extend_from_slice(&[0, 0, 0, 0]);
        d.extend_from_slice(b"IEND");
        d.extend_from_slice(&[0xAE, 0x42, 0x60, 0x82]);
        let score = BuiltinValidator.validate(FileType::Png, &d);
        assert!(score < 1.0);
    }

    #[test]
    fn pdf_with_valid_startxref_scores_full() {
        let body = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\nxref\n0 1\n0000000000 65535 f \ntrailer\n<<>>\nstartxref\n9\n%%EOF\n";
        // "xref" begins at byte 9 of body? Compute the true offset instead of guessing.
        let xref_at = find(body, b"xref\n0 1").unwrap();
        let mut doc = body.to_vec();
        let needle = b"startxref\n9\n";
        let pos = find(&doc, needle).unwrap();
        doc.truncate(pos);
        doc.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
        assert_eq!(BuiltinValidator.validate(FileType::Pdf, &doc), 1.0);
    }
}
