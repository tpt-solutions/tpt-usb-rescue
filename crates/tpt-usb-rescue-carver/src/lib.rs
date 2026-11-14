//! Zero-copy signature carver with pluggable media validation.
//!
//! The carver works on a byte slice, typically a memory-mapped image, and never copies the image.
//! Candidates are found by header signatures ([`SIGNATURES`]), their end is found from the file's
//! own structure, and each candidate is scored by a [`MediaValidator`]. Candidates below the
//! minimum score are rejected as false positives.
//!
//! Output is written by [`extract::write_carved`]; [`integrity`] hashes images and carved files.

pub mod extract;
pub mod formats;
pub mod integrity;
pub mod signature;
pub mod validate;

pub use signature::{FileType, SIGNATURES, Signature, find_end};
pub use validate::{BuiltinValidator, MediaValidator};

/// A file recovered from the image.
#[derive(Debug, Clone, PartialEq)]
pub struct CarvedFile {
    pub id: u64,
    pub file_type: FileType,
    /// Byte offset of the first byte, inclusive.
    pub start_offset: u64,
    /// Byte offset one past the last byte, exclusive.
    pub end_offset: u64,
    pub size_bytes: u64,
    /// 0.0..=1.0, derived from media validation.
    pub integrity_score: f32,
    /// Always `false` in v1: only contiguous files are carved.
    pub is_fragmented: bool,
}

/// Carving options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarveOptions {
    /// Reject candidates whose integrity score is below `min_score`.
    pub validate: bool,
    pub min_score: f32,
}

impl Default for CarveOptions {
    fn default() -> Self {
        Self {
            validate: true,
            min_score: 0.5,
        }
    }
}

/// Whether the secondary check of a signature (e.g. "WEBP" inside RIFF) matches at `hit`.
fn check_ok(data: &[u8], hit: usize, sig: &Signature) -> bool {
    match sig.check_at {
        None => true,
        Some((off, bytes)) => data
            .get(hit + off..hit + off + bytes.len())
            .is_some_and(|got| got == bytes),
    }
}

/// Finds and validates files in `data`. Results are ordered by start offset and never overlap.
pub fn carve(data: &[u8], opts: &CarveOptions, validator: &dyn MediaValidator) -> Vec<CarvedFile> {
    let mut hits: Vec<(usize, &Signature)> = SIGNATURES
        .iter()
        .flat_map(|sig| {
            signature::find_all(data, sig.header)
                .into_iter()
                .filter(move |&o| check_ok(data, o, sig))
                .filter(move |&o| o >= sig.start_adjust)
                .map(move |o| (o - sig.start_adjust, sig))
        })
        .collect();
    hits.sort_by_key(|(offset, _)| *offset);

    let mut out = Vec::new();
    let mut cursor = 0usize;
    for (idx, &(start, sig)) in hits.iter().enumerate() {
        if start < cursor {
            continue; // inside a file already carved
        }
        let max_end = (start as u64)
            .saturating_add(sig.max_size)
            .min(data.len() as u64) as usize;
        let end = match find_end(sig.file_type, &data[start..max_end]) {
            Some(rel) => start + rel,
            None => {
                // Never closed. Keep the partial data for inspection, but stop at the next
                // header so a stray header cannot swallow the real files after it.
                let next_header = hits[idx + 1..]
                    .iter()
                    .map(|(offset, _)| *offset)
                    .find(|offset| *offset > start)
                    .unwrap_or(data.len());
                max_end.min(next_header)
            }
        };
        let score = validator.validate(sig.file_type, &data[start..end]);
        if opts.validate && score < opts.min_score {
            continue;
        }
        let id = out.len() as u64 + 1;
        out.push(CarvedFile {
            id,
            file_type: sig.file_type,
            start_offset: start as u64,
            end_offset: end as u64,
            size_bytes: (end - start) as u64,
            integrity_score: score,
            is_fragmented: false,
        });
        cursor = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&crc32fast::hash(&c).to_be_bytes());
    }

    fn png_bytes() -> Vec<u8> {
        let mut d = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let ihdr = [0u8, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        push_chunk(&mut d, b"IHDR", &ihdr);
        push_chunk(&mut d, b"IDAT", &[9, 9, 9, 9]);
        push_chunk(&mut d, b"IEND", &[]);
        d
    }

    #[test]
    fn finds_png_inside_noise_with_exact_bounds() {
        let png = png_bytes();
        let mut img = vec![0x55u8; 1000];
        img.extend_from_slice(&png);
        img.extend_from_slice(&[0x11; 500]);
        let files = carve(&img, &CarveOptions::default(), &BuiltinValidator);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.file_type, FileType::Png);
        assert_eq!(f.start_offset, 1000);
        assert_eq!(f.size_bytes, png.len() as u64);
        assert_eq!(f.integrity_score, 1.0);
        assert!(!f.is_fragmented);
    }

    #[test]
    fn rejects_false_positive_header() {
        // A PNG signature followed by garbage: no valid chunks, so it must not be carved.
        let mut img = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        img.extend_from_slice(&[0xEE; 200]);
        assert!(carve(&img, &CarveOptions::default(), &BuiltinValidator).is_empty());
        // With validation off it is still reported, flagged by its low score.
        let loose = CarveOptions {
            validate: false,
            min_score: 0.0,
        };
        let files = carve(&img, &loose, &BuiltinValidator);
        assert_eq!(files.len(), 1);
        assert!(files[0].integrity_score < 0.5);
    }

    #[test]
    fn stray_unterminated_header_does_not_swallow_real_file() {
        // A JPEG header with no end marker sits in front of a real PNG.
        let mut img = vec![0xFF, 0xD8, 0xFF, 0xE0];
        img.extend_from_slice(&[0x33; 100]);
        let png_at = img.len() as u64;
        img.extend_from_slice(&png_bytes());
        let loose = CarveOptions {
            validate: false,
            min_score: 0.0,
        };
        let files = carve(&img, &loose, &BuiltinValidator);
        let png = files
            .iter()
            .find(|f| f.file_type == FileType::Png)
            .expect("PNG must still be carved");
        assert_eq!(png.start_offset, png_at);
        assert_eq!(png.integrity_score, 1.0);
    }

    #[test]
    fn carved_files_do_not_overlap() {
        let png = png_bytes();
        let mut img = png.clone();
        img.extend_from_slice(&png);
        let files = carve(&img, &CarveOptions::default(), &BuiltinValidator);
        assert_eq!(files.len(), 2);
        assert!(files[0].end_offset <= files[1].start_offset);
        assert_eq!(files[1].start_offset, png.len() as u64);
    }
}

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Every signature's walker must cope with arbitrary tails: no panic, no out-of-range slice.
        #[test]
        fn carving_arbitrary_tails_never_panics(
            which in 0..SIGNATURES.len(),
            tail in proptest::collection::vec(any::<u8>(), 0..4096),
        ) {
            let mut data = SIGNATURES[which].header.to_vec();
            data.extend_from_slice(&tail);
            let loose = CarveOptions { validate: false, min_score: 0.0 };
            for f in carve(&data, &loose, &BuiltinValidator) {
                prop_assert!(f.start_offset <= f.end_offset);
                prop_assert!(f.end_offset as usize <= data.len());
            }
        }
    }
}
