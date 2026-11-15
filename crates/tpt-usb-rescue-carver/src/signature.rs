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
    Gif,
    Bmp,
    Tiff,
    Webp,
    Wav,
    Avi,
    Mp3,
    Flac,
    Ogg,
    Pdf,
    /// Generic ZIP container; also covers DOCX, XLSX and similar Office formats.
    Zip,
    /// ISO base media: MP4 and QuickTime MOV.
    Mp4,
    Mkv,
    SevenZip,
    Rar,
    Gzip,
    Sqlite,
    Psd,
}

impl FileType {
    pub fn extension(self) -> &'static str {
        match self {
            FileType::Jpeg => "jpg",
            FileType::Png => "png",
            FileType::Gif => "gif",
            FileType::Bmp => "bmp",
            FileType::Tiff => "tif",
            FileType::Webp => "webp",
            FileType::Wav => "wav",
            FileType::Avi => "avi",
            FileType::Mp3 => "mp3",
            FileType::Flac => "flac",
            FileType::Ogg => "ogg",
            FileType::Pdf => "pdf",
            FileType::Zip => "zip",
            FileType::Mp4 => "mp4",
            FileType::Mkv => "mkv",
            FileType::SevenZip => "7z",
            FileType::Rar => "rar",
            FileType::Gzip => "gz",
            FileType::Sqlite => "sqlite",
            FileType::Psd => "psd",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FileType::Jpeg => "JPEG",
            FileType::Png => "PNG",
            FileType::Gif => "GIF",
            FileType::Bmp => "BMP",
            FileType::Tiff => "TIFF",
            FileType::Webp => "WEBP",
            FileType::Wav => "WAV",
            FileType::Avi => "AVI",
            FileType::Mp3 => "MP3",
            FileType::Flac => "FLAC",
            FileType::Ogg => "OGG",
            FileType::Pdf => "PDF",
            FileType::Zip => "ZIP",
            FileType::Mp4 => "MP4",
            FileType::Mkv => "MKV",
            FileType::SevenZip => "7Z",
            FileType::Rar => "RAR",
            FileType::Gzip => "GZIP",
            FileType::Sqlite => "SQLITE",
            FileType::Psd => "PSD",
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
    /// Extra bytes that must also match, at a fixed offset from the header (e.g. "WEBP" inside RIFF).
    pub check_at: Option<(usize, &'static [u8])>,
    /// The file starts this many bytes before the matched header (e.g. MP4's `ftyp` at offset 4).
    pub start_adjust: usize,
}

const fn sig(file_type: FileType, header: &'static [u8], max_size: u64) -> Signature {
    Signature {
        file_type,
        header,
        max_size,
        check_at: None,
        start_adjust: 0,
    }
}

const fn sig_at(
    file_type: FileType,
    header: &'static [u8],
    max_size: u64,
    off: usize,
    extra: &'static [u8],
) -> Signature {
    Signature {
        file_type,
        header,
        max_size,
        check_at: Some((off, extra)),
        start_adjust: 0,
    }
}

const MB: u64 = 1 << 20;
const GB: u64 = 1 << 30;

/// Compiled-in signature table.
pub const SIGNATURES: &[Signature] = &[
    sig(FileType::Jpeg, &[0xFF, 0xD8, 0xFF], 64 * MB),
    sig(
        FileType::Png,
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        128 * MB,
    ),
    sig(FileType::Gif, b"GIF89a", 32 * MB),
    sig(FileType::Gif, b"GIF87a", 32 * MB),
    sig(FileType::Bmp, b"BM", 64 * MB),
    sig(FileType::Tiff, b"II*\0", 256 * MB),
    sig(FileType::Tiff, b"MM\0*", 256 * MB),
    sig_at(FileType::Webp, b"RIFF", 256 * MB, 8, b"WEBP"),
    sig_at(FileType::Wav, b"RIFF", GB, 8, b"WAVE"),
    sig_at(FileType::Avi, b"RIFF", 4 * GB, 8, b"AVI "),
    sig(FileType::Mp3, b"ID3", 64 * MB),
    sig(FileType::Flac, b"fLaC", 2 * GB),
    sig(FileType::Ogg, b"OggS", 2 * GB),
    sig(FileType::Pdf, b"%PDF-", 512 * MB),
    sig(FileType::Zip, &[b'P', b'K', 0x03, 0x04], 2 * GB),
    Signature {
        file_type: FileType::Mp4,
        header: b"ftyp",
        max_size: 4 * GB,
        check_at: None,
        start_adjust: 4,
    },
    sig(FileType::Mkv, &[0x1A, 0x45, 0xDF, 0xA3], 4 * GB),
    sig(
        FileType::SevenZip,
        &[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C],
        4 * GB,
    ),
    sig(FileType::Rar, b"Rar!\x1A\x07", 4 * GB),
    sig(FileType::Gzip, &[0x1F, 0x8B, 0x08], 4 * GB),
    sig(FileType::Sqlite, b"SQLite format 3\0", 4 * GB),
    sig(FileType::Psd, b"8BPS\0\x01", 4 * GB),
];

/// Returns the offset (relative to `data`) one past the end of the file starting at `data[0]`,
/// or `None` if the structure never closes within `data`.
pub fn find_end(file_type: FileType, data: &[u8]) -> Option<usize> {
    use crate::formats as f;
    match file_type {
        FileType::Jpeg => walk_jpeg(data).end,
        FileType::Png => walk_png(data).end,
        FileType::Pdf => find_pdf_end(data),
        FileType::Gif => walk_gif(data).end,
        FileType::Zip => walk_zip(data).end,
        FileType::Bmp => f::bmp_end(data),
        FileType::Webp | FileType::Wav | FileType::Avi => f::riff_end(data),
        FileType::Ogg => f::ogg_end(data),
        FileType::Mp4 => f::mp4_end(data),
        FileType::SevenZip => f::sevenz_end(data),
        FileType::Rar => f::rar4_end(data),
        FileType::Sqlite => f::sqlite_end(data),
        FileType::Tiff
        | FileType::Mp3
        | FileType::Flac
        | FileType::Mkv
        | FileType::Gzip
        | FileType::Psd => None,
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

/// Most end-of-central-directory candidates examined before settling for the first one.
const MAX_EOCD_CANDIDATES: usize = 64;

pub fn walk_zip(d: &[u8]) -> ZipWalk {
    if d.len() < 22 || &d[..4] != b"PK" {
        return ZipWalk::default();
    }
    // A stored (uncompressed) archive inside this one carries its own end record, so the first
    // `PK` is not necessarily ours. Prefer the first whose central directory is
    // consistent; if none is, fall back to the first candidate that fits.
    let mut first_fit: Option<ZipWalk> = None;
    let mut from = 4usize;
    for _ in 0..MAX_EOCD_CANDIDATES {
        let Some(rel) = find(&d[from..], b"PK") else {
            break;
        };
        let eocd = from + rel;
        from = eocd + 1;
        let Some(w) = zip_at(d, eocd) else { continue };
        if w.cd_ok {
            return w;
        }
        first_fit.get_or_insert(w);
    }
    first_fit.unwrap_or_default()
}

/// Reads the end-of-central-directory record at `eocd`, if it fits inside `d`.
fn zip_at(d: &[u8], eocd: usize) -> Option<ZipWalk> {
    if eocd + 22 > d.len() {
        return None;
    }
    let comment = usize::from(u16::from_le_bytes([d[eocd + 20], d[eocd + 21]]));
    let end = eocd + 22 + comment;
    if end > d.len() {
        return None;
    }
    let mut w = ZipWalk {
        eocd: Some(eocd),
        end: Some(end),
        ..ZipWalk::default()
    };
    w.cd_size = u32::from_le_bytes([d[eocd + 12], d[eocd + 13], d[eocd + 14], d[eocd + 15]]);
    w.cd_offset = u32::from_le_bytes([d[eocd + 16], d[eocd + 17], d[eocd + 18], d[eocd + 19]]);
    let cd = w.cd_offset as usize;
    w.cd_ok = cd
        .checked_add(w.cd_size as usize)
        .is_some_and(|e| e <= eocd)
        && d.get(cd..cd + 4) == Some(b"PK");
    Some(w)
}

/// Every offset at which `needle` occurs in `hay`, ascending. Lazy, so scanning a large image
/// does not first collect every hit of a short signature such as `BM` into memory.
pub(crate) fn find_all<'a>(hay: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    let mut from = 0usize;
    std::iter::from_fn(move || {
        let rel = find(hay.get(from..)?, needle)?;
        let at = from + rel;
        from = at + 1;
        Some(at)
    })
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

    /// A minimal archive: local header, one central directory entry, end record.
    fn tiny_zip(prefix_len: usize) -> Vec<u8> {
        let mut d = b"PK".to_vec();
        d.extend_from_slice(&[0u8; 26]);
        let cd_off = (d.len() + prefix_len) as u32;
        d.extend_from_slice(b"PK");
        d.extend_from_slice(&[0u8; 42]);
        let cd_size = (d.len() + prefix_len) as u32 - cd_off;
        d.extend_from_slice(b"PK");
        d.extend_from_slice(&[0u8; 4]);
        d.extend_from_slice(&[1, 0, 1, 0]);
        d.extend_from_slice(&cd_size.to_le_bytes());
        d.extend_from_slice(&cd_off.to_le_bytes());
        d.extend_from_slice(&[0, 0]);
        d
    }

    #[test]
    fn zip_with_a_stored_zip_inside_ends_at_the_outer_record() {
        let inner = tiny_zip(0);
        // Outer archive: local header + stored inner archive, then its own directory and end.
        let mut d = b"PK".to_vec();
        d.extend_from_slice(&[0u8; 26]);
        d.extend_from_slice(&inner);
        let cd_off = d.len() as u32;
        d.extend_from_slice(b"PK");
        d.extend_from_slice(&[0u8; 42]);
        let cd_size = d.len() as u32 - cd_off;
        let eocd = d.len();
        d.extend_from_slice(b"PK");
        d.extend_from_slice(&[0u8; 4]);
        d.extend_from_slice(&[1, 0, 1, 0]);
        d.extend_from_slice(&cd_size.to_le_bytes());
        d.extend_from_slice(&cd_off.to_le_bytes());
        d.extend_from_slice(&[0, 0]);
        let w = walk_zip(&d);
        assert_eq!(
            w.eocd,
            Some(eocd),
            "must skip the inner archive's end record"
        );
        assert_eq!(w.end, Some(d.len()));
        assert!(w.cd_ok);
    }

    #[test]
    fn pdf_end_after_eof_marker() {
        let d = b"%PDF-1.4\n...\n%%EOF\r\nextra";
        assert_eq!(find_end(FileType::Pdf, d), Some(d.len() - 5));
    }
}
