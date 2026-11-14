//! Signature database and end-of-file detection.
//!
//! Each entry is a header (magic bytes) plus a maximum size. End detection walks the file's own
//! structure where that is cheap (JPEG segments, PNG chunks, GIF blocks, ZIP end-of-central-dir),
//! so a carved file ends where the format says it does rather than at a guessed length.

/// File types the carver knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    Jpeg,
    Png,
    Pdf,
    Gif,
    /// Generic ZIP container; also covers DOCX, XLSX and similar Office formats.
    Zip,
}

impl FileType {
    pub fn extension(self) -> &'static str {
        match self {
            FileType::Jpeg => "jpg",
            FileType::Png => "png",
            FileType::Pdf => "pdf",
            FileType::Gif => "gif",
            FileType::Zip => "zip",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FileType::Jpeg => "JPEG",
            FileType::Png => "PNG",
            FileType::Pdf => "PDF",
            FileType::Gif => "GIF",
            FileType::Zip => "ZIP",
        }
    }
}

/// A header signature for one file type.
#[derive(Debug, Clone, Copy)]
pub struct Signature {
    pub file_type: FileType,
    pub header: &'static [u8],
    /// Upper bound on carved size; scanning never reads past `start + max_size`.
    pub max_size: u64,
}

/// Compiled-in signature table.
pub const SIGNATURES: &[Signature] = &[
    Signature {
        file_type: FileType::Jpeg,
        header: &[0xFF, 0xD8, 0xFF],
        max_size: 64 << 20,
    },
    Signature {
        file_type: FileType::Png,
        header: &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        max_size: 128 << 20,
    },
    Signature {
        file_type: FileType::Pdf,
        header: b"%PDF-",
        max_size: 512 << 20,
    },
    Signature {
        file_type: FileType::Gif,
        header: b"GIF89a",
        max_size: 32 << 20,
    },
    Signature {
        file_type: FileType::Gif,
        header: b"GIF87a",
        max_size: 32 << 20,
    },
    Signature {
        file_type: FileType::Zip,
        header: &[b'P', b'K', 0x03, 0x04],
        max_size: 2 << 30,
    },
];

/// Returns the offset (relative to `data`) one past the end of the file starting at `data[0]`,
/// or `None` if the structure never closes within `data`.
pub fn find_end(file_type: FileType, data: &[u8]) -> Option<usize> {
    match file_type {
        FileType::Jpeg => walk_jpeg(data).end,
        FileType::Png => walk_png(data).end,
        FileType::Pdf => find_pdf_end(data),
        FileType::Gif => walk_gif(data).end,
        FileType::Zip => walk_zip(data).end,
    }
}

// --- JPEG -------------------------------------------------------------------------------------

/// Result of walking JPEG marker segments. Shared with the validator.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JpegWalk {
    pub end: Option<usize>,
    pub has_dqt: bool,
    pub has_sof: bool,
    pub has_dht: bool,
    pub has_sos: bool,
    pub bad_segments: u32,
}

pub fn walk_jpeg(d: &[u8]) -> JpegWalk {
    let mut w = JpegWalk::default();
    if d.len() < 4 || d[0] != 0xFF || d[1] != 0xD8 {
        w.bad_segments = 1;
        return w;
    }
    let mut p = 2usize;
    while p < d.len() {
        if d[p] != 0xFF {
            w.bad_segments += 1;
            break;
        }
        while p < d.len() && d[p] == 0xFF {
            p += 1; // fill bytes
        }
        if p >= d.len() {
            break;
        }
        let m = d[p];
        p += 1;
        if m == 0xD9 {
            w.end = Some(p);
            break;
        }
        if m == 0x00 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            // Stray stuffing or standalone marker outside entropy data: not valid here.
            w.bad_segments += 1;
            break;
        }
        if p + 2 > d.len() {
            break;
        }
        let len = usize::from(u16::from_be_bytes([d[p], d[p + 1]]));
        if len < 2 || p + len > d.len() {
            w.bad_segments += 1;
            break;
        }
        match m {
            0xDB => w.has_dqt = true,
            0xC4 => w.has_dht = true,
            0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) => w.has_sof = true,
            _ => {}
        }
        p += len;
        if m == 0xDA {
            w.has_sos = true;
            // Entropy-coded data: skip until a marker that is not a stuffed byte or RST.
            while p + 1 < d.len() {
                let (a, b) = (d[p], d[p + 1]);
                if a == 0xFF && b != 0x00 && !(0xD0..=0xD7).contains(&b) {
                    break;
                }
                p += 1;
            }
        }
    }
    w
}

// --- PNG --------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PngWalk {
    pub end: Option<usize>,
    pub chunks: u32,
    pub bad_crc: u32,
    pub first_is_ihdr: bool,
    pub has_idat: bool,
    pub has_iend: bool,
}

pub fn walk_png(d: &[u8]) -> PngWalk {
    let mut w = PngWalk::default();
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if d.len() < 8 || d[..8] != SIG {
        return w;
    }
    let mut p = 8usize;
    while p + 12 <= d.len() {
        let len = u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize;
        let kind = &d[p + 4..p + 8];
        let Some(total) = len.checked_add(12).filter(|t| p + t <= d.len()) else {
            break;
        };
        let body = &d[p + 4..p + 8 + len];
        let stored = u32::from_be_bytes([
            d[p + 8 + len],
            d[p + 9 + len],
            d[p + 10 + len],
            d[p + 11 + len],
        ]);
        if crc32fast::hash(body) != stored {
            w.bad_crc += 1;
        }
        if w.chunks == 0 {
            w.first_is_ihdr = kind == b"IHDR";
        }
        if kind == b"IDAT" {
            w.has_idat = true;
        }
        w.chunks += 1;
        p += total;
        if kind == b"IEND" {
            w.has_iend = true;
            w.end = Some(p);
            break;
        }
    }
    w
}

// --- PDF --------------------------------------------------------------------------------------

fn find_pdf_end(d: &[u8]) -> Option<usize> {
    let pos = find(d, b"%%EOF")?;
    let mut end = pos + 5;
    // Include the line terminator that conventionally follows %%EOF.
    for _ in 0..2 {
        if end < d.len() && (d[end] == b'\r' || d[end] == b'\n') {
            end += 1;
        }
    }
    Some(end)
}

// --- GIF --------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GifWalk {
    pub end: Option<usize>,
    pub blocks: u32,
    pub sub_block_errors: u32,
}

pub fn walk_gif(d: &[u8]) -> GifWalk {
    let mut w = GifWalk::default();
    if d.len() < 13 || !(d.starts_with(b"GIF87a") || d.starts_with(b"GIF89a")) {
        return w;
    }
    let flags = d[10];
    let mut p = 13usize;
    if flags & 0x80 != 0 {
        p += 3 * (1usize << ((flags & 0x07) + 1));
    }
    while p < d.len() {
        match d[p] {
            0x3B => {
                w.end = Some(p + 1);
                break;
            }
            0x21 => {
                // Extension: label byte then sub-blocks.
                if p + 2 > d.len() {
                    break;
                }
                p += 2;
                match skip_sub_blocks(d, p) {
                    Some(next) => p = next,
                    None => {
                        w.sub_block_errors += 1;
                        break;
                    }
                }
            }
            0x2C => {
                // Image descriptor: 9 bytes, optional local colour table, LZW size, sub-blocks.
                if p + 10 > d.len() {
                    break;
                }
                let lflags = d[p + 9];
                p += 10;
                if lflags & 0x80 != 0 {
                    p += 3 * (1usize << ((lflags & 0x07) + 1));
                }
                if p >= d.len() {
                    break;
                }
                p += 1; // LZW minimum code size
                match skip_sub_blocks(d, p) {
                    Some(next) => p = next,
                    None => {
                        w.sub_block_errors += 1;
                        break;
                    }
                }
                w.blocks += 1;
            }
            _ => {
                w.sub_block_errors += 1;
                break;
            }
        }
    }
    w
}

fn skip_sub_blocks(d: &[u8], mut p: usize) -> Option<usize> {
    loop {
        let n = *d.get(p)? as usize;
        p += 1;
        if n == 0 {
            return Some(p);
        }
        p = p.checked_add(n).filter(|q| *q <= d.len())?;
    }
}

// --- ZIP --------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ZipWalk {
    pub end: Option<usize>,
    /// Offset of the end-of-central-directory record, relative to the start.
    pub eocd: Option<usize>,
    pub cd_offset: u32,
    pub cd_size: u32,
    pub cd_ok: bool,
}

pub fn walk_zip(d: &[u8]) -> ZipWalk {
    let mut w = ZipWalk::default();
    if d.len() < 22 || &d[..4] != b"PK\x03\x04" {
        return w;
    }
    // The first end-of-central-directory signature after the local header closes this archive.
    let Some(eocd) = find(&d[4..], b"PK\x05\x06").map(|x| x + 4) else {
        return w;
    };
    if eocd + 22 > d.len() {
        return w;
    }
    let comment = usize::from(u16::from_le_bytes([d[eocd + 20], d[eocd + 21]]));
    let end = eocd + 22 + comment;
    if end > d.len() {
        return w;
    }
    w.eocd = Some(eocd);
    w.end = Some(end);
    w.cd_size = u32::from_le_bytes([d[eocd + 12], d[eocd + 13], d[eocd + 14], d[eocd + 15]]);
    w.cd_offset = u32::from_le_bytes([d[eocd + 16], d[eocd + 17], d[eocd + 18], d[eocd + 19]]);
    let cd = w.cd_offset as usize;
    w.cd_ok = cd
        .checked_add(w.cd_size as usize)
        .is_some_and(|e| e <= eocd)
        && d.get(cd..cd + 4) == Some(b"PK\x01\x02");
    w
}

/// Every offset at which `needle` occurs in `hay`, ascending.
pub(crate) fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = find(&hay[from..], needle) {
        out.push(from + rel);
        from += rel + 1;
    }
    out
}

/// First occurrence of `needle` in `hay`.
pub(crate) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let first = needle[0];
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        match hay[i..=hay.len() - needle.len()]
            .iter()
            .position(|&b| b == first)
        {
            Some(off) => {
                i += off;
                if &hay[i..i + needle.len()] == needle {
                    return Some(i);
                }
                i += 1;
            }
            None => break,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_walk_finds_eoi_after_scan_data() {
        // SOI, DQT(len 3), SOF0(len 4), DHT(len 3), SOS(len 2) + entropy bytes with a stuffed FF00, EOI.
        let d = [
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x03, 0x00, 0xFF, 0xC0, 0x00, 0x04, 0x08, 0x00, 0xFF,
            0xC4, 0x00, 0x03, 0x00, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22, 0xFF, 0x00, 0x33, 0xFF,
            0xD9,
        ];
        let w = walk_jpeg(&d);
        assert_eq!(w.end, Some(d.len()));
        assert!(w.has_dqt && w.has_sof && w.has_dht && w.has_sos);
        assert_eq!(w.bad_segments, 0);
    }

    #[test]
    fn jpeg_truncated_has_no_end() {
        let d = [0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x03, 0x00];
        assert_eq!(walk_jpeg(&d).end, None);
    }

    #[test]
    fn png_walk_checks_crc_and_stops_at_iend() {
        let mut d = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let ihdr = [0u8, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        push_chunk(&mut d, b"IHDR", &ihdr);
        push_chunk(&mut d, b"IDAT", &[1, 2, 3]);
        push_chunk(&mut d, b"IEND", &[]);
        d.extend_from_slice(b"trailing junk");
        let w = walk_png(&d);
        assert!(w.has_iend && w.first_is_ihdr && w.has_idat);
        assert_eq!(w.bad_crc, 0);
        assert_eq!(w.end, Some(d.len() - b"trailing junk".len()));
    }

    fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&crc32fast::hash(&c).to_be_bytes());
    }

    #[test]
    fn gif_walk_reaches_trailer() {
        // Header, 1x1 logical screen without global table, image descriptor, one sub-block, trailer.
        let mut d = b"GIF89a".to_vec();
        d.extend_from_slice(&[1, 0, 1, 0, 0x00, 0, 0]);
        d.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0x00]); // descriptor
        d.push(0x02); // LZW min code size
        d.extend_from_slice(&[0x02, 0x4C, 0x01, 0x00]); // one sub-block then terminator
        d.push(0x3B);
        let w = walk_gif(&d);
        assert_eq!(w.end, Some(d.len()));
        assert_eq!(w.sub_block_errors, 0);
    }

    #[test]
    fn zip_walk_finds_eocd() {
        let mut d = b"PK\x03\x04".to_vec();
        d.extend_from_slice(&[0u8; 26]);
        let cd_off = d.len() as u32;
        d.extend_from_slice(b"PK\x01\x02");
        d.extend_from_slice(&[0u8; 42]);
        let cd_size = (d.len() as u32) - cd_off;
        let eocd = d.len();
        d.extend_from_slice(b"PK\x05\x06");
        d.extend_from_slice(&[0u8; 4]);
        d.extend_from_slice(&[1, 0, 1, 0]); // entries
        d.extend_from_slice(&cd_size.to_le_bytes());
        d.extend_from_slice(&cd_off.to_le_bytes());
        d.extend_from_slice(&[0, 0]); // comment length
        let w = walk_zip(&d);
        assert_eq!(w.eocd, Some(eocd));
        assert_eq!(w.end, Some(d.len()));
        assert!(w.cd_ok);
    }

    #[test]
    fn pdf_end_after_eof_marker() {
        let d = b"%PDF-1.4\n...\n%%EOF\r\nextra";
        assert_eq!(find_end(FileType::Pdf, d), Some(d.len() - 5));
    }
}
