//! Structure walkers and validators for the formats added to reach the top-20 set.
//!
//! Each `*_end` function returns the length of the file that starts at `data[0]`, or `None` when
//! the format has no end marker the carver can find (the carver then bounds the file by the next
//! header or the type's maximum size). Each `*_checks` function returns the list of format checks
//! that the candidate satisfies; the validator scores the fraction that pass.

use crate::signature::find;

fn le16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn le32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn le64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

// --- BMP --------------------------------------------------------------------------------------

pub fn bmp_end(d: &[u8]) -> Option<usize> {
    let size = le32(d, 2)? as usize;
    (size >= 26 && size <= d.len()).then_some(size)
}

pub fn bmp_checks(d: &[u8]) -> Vec<bool> {
    let size = le32(d, 2).unwrap_or(0) as usize;
    let dib = le32(d, 14).unwrap_or(0);
    let pixel_offset = le32(d, 10).unwrap_or(u32::MAX) as usize;
    vec![
        d.starts_with(b"BM"),
        size == d.len(),
        matches!(dib, 12 | 40 | 52 | 56 | 64 | 108 | 124),
        pixel_offset >= 14 + dib as usize && pixel_offset < size.max(1),
        le32(d, 6) == Some(0),
    ]
}

// --- RIFF family (WAV, AVI, WEBP) -------------------------------------------------------------

/// Total length of a RIFF container, from its 32-bit size field.
pub fn riff_end(d: &[u8]) -> Option<usize> {
    let total = 8usize.checked_add(le32(d, 4)? as usize)?;
    (total <= d.len() && total >= 12).then_some(total)
}

pub fn wav_checks(d: &[u8]) -> Vec<bool> {
    let end = riff_end(d).unwrap_or(0);
    vec![
        d.starts_with(b"RIFF") && d.get(8..12) == Some(b"WAVE"),
        end > 0 && end <= d.len(),
        d.get(12..16) == Some(b"fmt "),
        d.get(36..40) == Some(b"data") || find(&d[..d.len().min(256)], b"data").is_some(),
    ]
}

pub fn avi_checks(d: &[u8]) -> Vec<bool> {
    let end = riff_end(d).unwrap_or(0);
    vec![
        d.starts_with(b"RIFF") && d.get(8..12) == Some(b"AVI "),
        end > 0 && end <= d.len(),
        d.get(12..16) == Some(b"LIST"),
        find(&d[..d.len().min(64)], b"hdrl").is_some(),
    ]
}

pub fn webp_checks(d: &[u8]) -> Vec<bool> {
    let end = riff_end(d).unwrap_or(0);
    vec![
        d.starts_with(b"RIFF") && d.get(8..12) == Some(b"WEBP"),
        end > 0 && end <= d.len(),
        matches!(d.get(12..16), Some(b"VP8 " | b"VP8L" | b"VP8X")),
    ]
}

// --- MP3 (ID3-tagged) -------------------------------------------------------------------------

fn syncsafe(b: &[u8]) -> Option<usize> {
    if b.len() < 4 || b.iter().any(|x| x & 0x80 != 0) {
        return None;
    }
    Some(b.iter().fold(0usize, |acc, &x| (acc << 7) | x as usize))
}

/// End of the ID3 tag, where the first audio frame should begin.
fn id3_tag_end(d: &[u8]) -> Option<usize> {
    let size = syncsafe(d.get(6..10)?)?;
    Some(10 + size)
}

pub fn mp3_checks(d: &[u8]) -> Vec<bool> {
    let major = d.get(3).copied().unwrap_or(0);
    let tag_end = id3_tag_end(d);
    let frame_ok = tag_end
        .and_then(|e| d.get(e..e + 2))
        .is_some_and(|f| f[0] == 0xFF && f[1] & 0xE0 == 0xE0);
    vec![
        d.starts_with(b"ID3"),
        (2..=4).contains(&major),
        d.get(5).is_some_and(|f| f & 0x0F == 0),
        tag_end.is_some(),
        frame_ok,
    ]
}

// --- FLAC -------------------------------------------------------------------------------------

pub fn flac_checks(d: &[u8]) -> Vec<bool> {
    let block_type = d.get(4).map(|b| b & 0x7F);
    let block_len = d
        .get(5..8)
        .map(|b| (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32);
    let min_block = be16(d, 8);
    let max_block = be16(d, 10);
    vec![
        d.starts_with(b"fLaC"),
        block_type == Some(0),
        block_len == Some(34),
        min_block.is_some_and(|m| m >= 16),
        max_block.is_some_and(|m| m >= 16),
    ]
}

// --- OGG --------------------------------------------------------------------------------------

/// Walks Ogg pages; returns the end of the page that carries the end-of-stream flag.
pub fn ogg_end(d: &[u8]) -> Option<usize> {
    let mut p = 0usize;
    while let Some(page) = ogg_page(d, p) {
        if page.eos {
            return Some(page.end);
        }
        p = page.end;
    }
    None
}

struct OggPage {
    end: usize,
    eos: bool,
}

fn ogg_page(d: &[u8], p: usize) -> Option<OggPage> {
    if d.get(p..p + 4)? != b"OggS" || *d.get(p + 4)? != 0 {
        return None;
    }
    let flags = *d.get(p + 5)?;
    let segments = *d.get(p + 26)? as usize;
    let table = d.get(p + 27..p + 27 + segments)?;
    let body: usize = table.iter().map(|&s| s as usize).sum();
    let end = p + 27 + segments + body;
    if end > d.len() {
        return None;
    }
    Some(OggPage {
        end,
        eos: flags & 0x04 != 0,
    })
}

pub fn ogg_checks(d: &[u8]) -> Vec<bool> {
    let mut pages = 0u32;
    let mut p = 0usize;
    let mut bad = 0u32;
    while let Some(page) = ogg_page(d, p) {
        pages += 1;
        p = page.end;
        if page.eos {
            break;
        }
    }
    if pages > 0 && p < d.len() && d.get(p..p + 4) == Some(b"OggS") {
        bad += 1; // next page is malformed
    }
    vec![
        d.starts_with(b"OggS"),
        pages >= 1,
        bad == 0,
        ogg_end(d).is_some(),
    ]
}

// --- MP4 / MOV (ISO base media) ---------------------------------------------------------------

struct Box4 {
    kind: [u8; 4],
    end: usize,
}

/// One ISO box at `p`. Size 0 means "to end of data"; size 1 means a 64-bit size follows.
fn iso_box(d: &[u8], p: usize) -> Option<Box4> {
    let size32 = be32(d, p)? as u64;
    let kind: [u8; 4] = d.get(p + 4..p + 8)?.try_into().ok()?;
    if !kind
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b' ' || *c == b'-')
    {
        return None;
    }
    let size = match size32 {
        0 => (d.len() - p) as u64,
        1 => be64(d, p + 8)?,
        s => s,
    };
    let header = if size32 == 1 { 16 } else { 8 };
    if size < header as u64 {
        return None;
    }
    let end = p.checked_add(size as usize).filter(|e| *e <= d.len())?;
    Some(Box4 { kind, end })
}

/// Walks top-level boxes; returns (boxes, end of last valid box).
fn iso_walk(d: &[u8]) -> (Vec<[u8; 4]>, usize) {
    let mut kinds = Vec::new();
    let mut p = 0usize;
    while let Some(b) = iso_box(d, p) {
        kinds.push(b.kind);
        p = b.end;
        if p >= d.len() {
            break;
        }
    }
    (kinds, p)
}

pub fn mp4_end(d: &[u8]) -> Option<usize> {
    // The carver starts at the 'ftyp' header minus four bytes, so the first box is the ftyp box.
    let (kinds, end) = iso_walk(d);
    (kinds.len() >= 2).then_some(end)
}

pub fn mp4_checks(d: &[u8]) -> Vec<bool> {
    let (kinds, end) = iso_walk(d);
    // Without a media payload box the candidate is a bare header: score it zero.
    if !kinds.iter().any(|k| k == b"moov" || k == b"mdat") {
        return vec![false; 4];
    }
    vec![
        kinds.first() == Some(b"ftyp"),
        kinds.iter().any(|k| k == b"moov" || k == b"mdat"),
        kinds.len() >= 2,
        end > 0,
    ]
}

// --- Matroska / WebM --------------------------------------------------------------------------

pub fn mkv_checks(d: &[u8]) -> Vec<bool> {
    let head = &d[..d.len().min(64)];
    vec![
        d.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]),
        find(head, b"matroska").is_some() || find(head, b"webm").is_some(),
    ]
}

// --- 7z ---------------------------------------------------------------------------------------

pub fn sevenz_end(d: &[u8]) -> Option<usize> {
    let next_off = le64(d, 12)?;
    let next_size = le64(d, 20)?;
    let end = 32u64.checked_add(next_off)?.checked_add(next_size)?;
    (end <= d.len() as u64).then_some(end as usize)
}

pub fn sevenz_checks(d: &[u8]) -> Vec<bool> {
    let start_crc_ok = le32(d, 8).is_some_and(|c| c == crc32fast::hash(&d[12..32.min(d.len())]));
    vec![
        d.starts_with(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]),
        start_crc_ok,
        sevenz_end(d).is_some(),
    ]
}

// --- RAR ----------------------------------------------------------------------------------------

/// RAR 4 block walk; ends at the end-of-archive block (type 0x7B).
pub fn rar4_end(d: &[u8]) -> Option<usize> {
    let mut p = 7usize;
    loop {
        let head_type = *d.get(p + 2)?;
        let flags = le16(d, p + 3)?;
        let head_size = le16(d, p + 5)? as usize;
        if head_size < 7 {
            return None;
        }
        let add = if flags & 0x8000 != 0 {
            le32(d, p + 7)? as usize
        } else {
            0
        };
        let next = p.checked_add(head_size)?.checked_add(add)?;
        if head_type == 0x7B {
            return (next <= d.len()).then_some(next);
        }
        if next > d.len() || next <= p {
            return None;
        }
        p = next;
    }
}

pub fn rar_checks(d: &[u8]) -> Vec<bool> {
    let v4 = d.starts_with(b"Rar!\x1A\x07\x00");
    let v5 = d.starts_with(b"Rar!\x1A\x07\x01\x00");
    let mut checks = vec![v4 || v5];
    if v4 {
        checks.push(rar4_end(d).is_some());
    } else if v5 {
        checks.push(true);
    }
    checks
}

// --- GZIP -------------------------------------------------------------------------------------

pub fn gzip_checks(d: &[u8]) -> Vec<bool> {
    let flags = d.get(3).copied().unwrap_or(0xFF);
    vec![
        d.starts_with(&[0x1F, 0x8B, 0x08]),
        flags & 0xE0 == 0,
        d.get(9).is_some_and(|os| *os <= 13 || *os == 255),
    ]
}

// --- SQLite -----------------------------------------------------------------------------------

pub fn sqlite_end(d: &[u8]) -> Option<usize> {
    let page_size = sqlite_page_size(d)?;
    let pages = be32(d, 28)? as usize;
    let end = pages.checked_mul(page_size)?;
    (end > 0 && end <= d.len()).then_some(end)
}

fn sqlite_page_size(d: &[u8]) -> Option<usize> {
    match be16(d, 16)? {
        1 => Some(65536),
        s if s.is_power_of_two() && s >= 512 => Some(s as usize),
        _ => None,
    }
}

pub fn sqlite_checks(d: &[u8]) -> Vec<bool> {
    vec![
        d.starts_with(b"SQLite format 3\0"),
        sqlite_page_size(d).is_some(),
        matches!(d.get(18), Some(1 | 2)) && matches!(d.get(19), Some(1 | 2)),
        be32(d, 28).is_some_and(|p| p > 0),
    ]
}

// --- PSD --------------------------------------------------------------------------------------

pub fn psd_checks(d: &[u8]) -> Vec<bool> {
    let channels = be16(d, 12).unwrap_or(0);
    let height = be32(d, 14).unwrap_or(0);
    let width = be32(d, 18).unwrap_or(0);
    let depth = be16(d, 22).unwrap_or(0);
    let mode = be16(d, 24).unwrap_or(99);
    vec![
        d.starts_with(b"8BPS\0\x01"),
        (1..=56).contains(&channels),
        height > 0 && width > 0,
        matches!(depth, 1 | 8 | 16 | 32),
        matches!(mode, 0 | 1 | 2 | 3 | 4 | 7 | 8 | 9),
    ]
}

// --- TIFF -------------------------------------------------------------------------------------

pub fn tiff_checks(d: &[u8]) -> Vec<bool> {
    let le = d.starts_with(b"II*\0");
    let be = d.starts_with(b"MM\0*");
    let ifd = if le {
        le32(d, 4)
    } else if be {
        be32(d, 4)
    } else {
        None
    }
    .unwrap_or(0) as usize;
    let count = if le { le16(d, ifd) } else { be16(d, ifd) }.unwrap_or(0) as usize;
    let entries_fit = count > 0 && ifd + 2 + count * 12 + 4 <= d.len();
    vec![le || be, ifd >= 8 && ifd < d.len(), entries_fit]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(checks: &[bool]) -> f32 {
        checks.iter().filter(|c| **c).count() as f32 / checks.len() as f32
    }

    #[test]
    fn bmp_size_field_defines_end() {
        let mut d = vec![0u8; 60];
        d[0..2].copy_from_slice(b"BM");
        d[2..6].copy_from_slice(&60u32.to_le_bytes());
        d[10..14].copy_from_slice(&54u32.to_le_bytes());
        d[14..18].copy_from_slice(&40u32.to_le_bytes());
        assert_eq!(bmp_end(&d), Some(60));
        assert_eq!(ok(&bmp_checks(&d)), 1.0);
    }

    #[test]
    fn wav_riff_size_defines_end() {
        let mut d = b"RIFF".to_vec();
        d.extend_from_slice(&32u32.to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        d.extend_from_slice(&[0u8; 16]);
        d.extend_from_slice(b"data");
        d.extend_from_slice(&[0u8; 4]);
        assert_eq!(riff_end(&d), Some(d.len()));
        assert_eq!(ok(&wav_checks(&d)), 1.0);
    }

    #[test]
    fn mp4_box_walk_finds_end() {
        let mut d = Vec::new();
        d.extend_from_slice(&16u32.to_be_bytes());
        d.extend_from_slice(b"ftyp");
        d.extend_from_slice(b"isom\0\0\0\0");
        d.extend_from_slice(&8u32.to_be_bytes());
        d.extend_from_slice(b"moov");
        d.extend_from_slice(b"junk after the file");
        assert_eq!(mp4_end(&d), Some(24));
        assert_eq!(ok(&mp4_checks(&d)), 1.0);
    }

    #[test]
    fn sevenz_start_header_crc_and_end() {
        let mut d = vec![b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4];
        d.extend_from_slice(&[0u8; 4]); // start header CRC placeholder
        d.extend_from_slice(&4u64.to_le_bytes()); // next header offset
        d.extend_from_slice(&6u64.to_le_bytes()); // next header size
        d.extend_from_slice(&[0u8; 4]); // next header CRC
        d.extend_from_slice(&[0u8; 10]); // 4 bytes of packed data, then 6 bytes of next header
        let crc = crc32fast::hash(&d[12..32]);
        d[8..12].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(sevenz_end(&d), Some(d.len()));
        assert_eq!(ok(&sevenz_checks(&d)), 1.0);
    }

    #[test]
    fn sqlite_page_count_defines_end() {
        let mut d = vec![0u8; 4096 * 2];
        d[..16].copy_from_slice(b"SQLite format 3\0");
        d[16..18].copy_from_slice(&4096u16.to_be_bytes());
        d[18] = 1;
        d[19] = 1;
        d[28..32].copy_from_slice(&2u32.to_be_bytes());
        assert_eq!(sqlite_end(&d), Some(8192));
        assert_eq!(ok(&sqlite_checks(&d)), 1.0);
    }

    #[test]
    fn ogg_stops_at_end_of_stream_page() {
        let mut page = b"OggS\0\x04".to_vec(); // EOS flag
        page.extend_from_slice(&[0u8; 20]);
        page.push(1); // one segment
        page.push(3); // of length 3
        page.extend_from_slice(&[9, 9, 9]);
        let len = page.len();
        page.extend_from_slice(b"trailing");
        assert_eq!(ogg_end(&page), Some(len));
    }

    #[test]
    fn flac_streaminfo_checks() {
        let mut d = b"fLaC".to_vec();
        d.extend_from_slice(&[0x00, 0x00, 0x00, 34]);
        d.extend_from_slice(&[0x10, 0x00, 0x10, 0x00]); // min/max block 4096
        d.extend_from_slice(&[0u8; 26]);
        assert_eq!(ok(&flac_checks(&d)), 1.0);
    }
}
