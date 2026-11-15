# Supported formats

## Carving (20 types)

Each type has a header signature, a maximum size, an end-of-file rule and a validator. The
validator scores the fraction of format checks that pass.

| Type | Header | End of file | Validator checks |
|---|---|---|---|
| JPEG | `FF D8 FF` | EOI marker, found by walking segments | SOI, DQT, SOF, DHT, SOS, EOI reached, no bad segments |
| PNG | 8-byte PNG signature | IEND chunk | First chunk IHDR, IDAT present, CRC of every chunk, IEND present |
| GIF | `GIF87a` / `GIF89a` | Trailer `3B`, found by walking blocks | Header, trailer reached, image blocks, sub-blocks valid |
| BMP | `BM` | Size field at offset 2 | Size matches length, DIB header size known, pixel offset, reserved zero |
| TIFF | `II*\0` / `MM\0*` | None | Byte order, IFD offset in range, IFD entries fit |
| WEBP | `RIFF` + `WEBP` | RIFF size field | RIFF/WEBP, size consistent, VP8 chunk type |
| WAV | `RIFF` + `WAVE` | RIFF size field | RIFF/WAVE, size consistent, `fmt ` chunk, `data` chunk |
| AVI | `RIFF` + `AVI ` | RIFF size field | RIFF/AVI, size consistent, LIST chunk, `hdrl` |
| MP3 | `ID3` | None (ID3 tag size is checked) | ID3 version 2–4, flags, tag end, MPEG frame sync after tag |
| FLAC | `fLaC` | None | STREAMINFO block, length 34, block sizes ≥ 16 |
| OGG | `OggS` | End-of-stream page | Page structure, page count, EOS reached |
| PDF | `%PDF-` | `%%EOF` marker | Header, `%%EOF` in last 1 KiB, `startxref` points at an xref or object |
| ZIP (DOCX, XLSX, …) | `PK 03 04` | End-of-central-directory record | Local header, EOCD found, central directory consistent |
| MP4 / MOV | `ftyp` (carved from 4 bytes before) | End of last top-level box | First box `ftyp`, `moov` or `mdat` present (a bare header scores zero) |
| MKV / WebM | EBML `1A 45 DF A3` | None | EBML header, `matroska` or `webm` doctype |
| 7z | `7z BC AF 27 1C` | Next-header offset and size | Start-header CRC, end inside the file |
| RAR | `Rar! 1A 07` | End-of-archive block (RAR 4) | Signature, block chain reaches end block (RAR 4) |
| GZIP | `1F 8B 08` | None | Method 8, reserved flag bits zero, OS byte known |
| SQLite | `SQLite format 3\0` | Page size × page count | Page size valid, file format versions, page count |
| PSD | `8BPS` + version 1 | None | Channels, dimensions, depth, colour mode |

"None" means the carver cannot find the end. The file is then cut at the next signature hit, or at
the type's maximum size, whichever comes first. The size is an upper bound in those cases.

### Limits of the validators

- Validators check structure, not content. A JPEG with valid segments but corrupt entropy-coded
  data still scores 1.0.
- JPEG checks segments only. Huffman table contents are not decoded.
- The header-only formats (MP3, FLAC, TIFF, MKV, GZIP, PSD) are scored on their structure fields.
  Their scores are not a proof of intact data.
- The carver is contiguous only. A fragmented file is not reassembled, and `is_fragmented` is
  always false.

## Filesystems

| Filesystem | Read | Deleted entries | Notes |
|---|---|---|---|
| FAT32 | Yes | Listed, read contiguously | Follows FAT chains, LFN names |
| exFAT | Yes | Listed, read contiguously | File sets, NoFatChain runs |
| NTFS | Yes | Listed, read from run lists | MFT fixups; tree from parent references |
| ext4, APFS, others | No | No | Not implemented |

Volumes are found by scanning for boot sectors, so a damaged partition table does not stop
detection. The MBR partition listing is shown separately.
