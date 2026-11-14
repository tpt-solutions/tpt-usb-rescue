//! FAT32 parser: boot sector, FAT chains, directory tree (including LFN and deleted entries).

use crate::{FileEntry, Filesystem, FsError, VolumeKind, le16, le32, slice};
use std::collections::HashSet;

const MAX_DEPTH: u32 = 32;
const EOC_MIN: u32 = 0x0FFF_FFF8;

pub struct Fat32<'a> {
    img: &'a [u8],
    fat_offset: usize,
    data_offset: usize,
    cluster_bytes: usize,
    cluster_count: u32,
    root_cluster: u32,
}

impl<'a> Fat32<'a> {
    /// Parses the boot sector at `base`. Fails with `NotThisFilesystem` if the signature is absent.
    pub fn open(img: &'a [u8], base: usize) -> Result<Self, FsError> {
        let b = slice(img, base, 512)?;
        if b[510..512] != [0x55, 0xAA] || &b[82..90] != b"FAT32   " {
            return Err(FsError::NotThisFilesystem("FAT32"));
        }
        let bytes_per_sector = le16(b, 11).ok_or(FsError::Corrupt("boot sector"))? as usize;
        let sectors_per_cluster = b[13] as usize;
        let reserved = le16(b, 14).ok_or(FsError::Corrupt("boot sector"))? as usize;
        let num_fats = b[16] as usize;
        let root_entries = le16(b, 17).ok_or(FsError::Corrupt("boot sector"))?;
        let fat16_size = le16(b, 22).ok_or(FsError::Corrupt("boot sector"))?;
        let fat_size = le32(b, 36).ok_or(FsError::Corrupt("boot sector"))? as usize;
        let root_cluster = le32(b, 44).ok_or(FsError::Corrupt("boot sector"))?;
        let mut total_sectors = le32(b, 32).ok_or(FsError::Corrupt("boot sector"))? as usize;
        if total_sectors == 0 {
            total_sectors = le16(b, 19).ok_or(FsError::Corrupt("boot sector"))? as usize;
        }

        if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
            return Err(FsError::Corrupt("bytes per sector"));
        }
        if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
            return Err(FsError::Corrupt("sectors per cluster"));
        }
        if num_fats == 0 || fat_size == 0 || root_entries != 0 || fat16_size != 0 {
            return Err(FsError::Corrupt("FAT geometry"));
        }
        if root_cluster < 2 {
            return Err(FsError::Corrupt("root cluster"));
        }
        let meta_sectors = reserved + num_fats * fat_size;
        if total_sectors <= meta_sectors {
            return Err(FsError::Corrupt("volume smaller than its metadata"));
        }
        let cluster_count = ((total_sectors - meta_sectors) / sectors_per_cluster) as u32;

        Ok(Self {
            img,
            fat_offset: base + reserved * bytes_per_sector,
            data_offset: base + meta_sectors * bytes_per_sector,
            cluster_bytes: sectors_per_cluster * bytes_per_sector,
            cluster_count,
            root_cluster,
        })
    }

    fn cluster_abs(&self, c: u32) -> Option<usize> {
        if c < 2 || c - 2 >= self.cluster_count {
            return None;
        }
        Some(self.data_offset + (c - 2) as usize * self.cluster_bytes)
    }

    fn next(&self, c: u32) -> Option<u32> {
        let at = self.fat_offset.checked_add(c as usize * 4)?;
        Some(le32(self.img, at)? & 0x0FFF_FFFF)
    }

    /// Follows the FAT from `start`. Stops at end-of-chain, a free/bad entry, a cycle, or the
    /// volume's cluster count.
    fn chain(&self, start: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut c = start;
        while self.cluster_abs(c).is_some() && seen.insert(c) {
            out.push(c);
            match self.next(c) {
                Some(n) if n >= EOC_MIN => break,
                Some(n) if n >= 2 => c = n,
                _ => break,
            }
        }
        out
    }

    fn cluster_run(&self, start: u32, bytes: u64) -> Vec<u32> {
        let n = bytes.div_ceil(self.cluster_bytes as u64) as u32;
        (0..n)
            .map(|i| start.saturating_add(i))
            .take_while(|c| self.cluster_abs(*c).is_some())
            .collect()
    }

    /// Concatenates a directory's clusters, following the FAT.
    fn dir_bytes(&self, first: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for c in self.chain(first) {
            let Some(off) = self.cluster_abs(c) else {
                continue;
            };
            if let Some(bytes) = self.img.get(off..off + self.cluster_bytes) {
                out.extend_from_slice(bytes);
            }
        }
        out
    }

    fn walk(
        &self,
        first: u32,
        prefix: &str,
        depth: u32,
        seen: &mut HashSet<u32>,
        out: &mut Vec<FileEntry>,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        let dir = self.dir_bytes(first);
        let mut lfn: Vec<(u8, Vec<u16>)> = Vec::new();
        for raw in dir.chunks_exact(32) {
            if raw[0] == 0x00 {
                break;
            }
            let deleted = raw[0] == 0xE5;
            if raw[11] == 0x0F {
                // Long-file-name piece: 13 UTF-16 code units spread over three fields.
                let seq = raw[0] & 0x1F;
                let mut units = Vec::with_capacity(13);
                for range in [1..11usize, 14..26, 28..32] {
                    for pair in raw[range].chunks_exact(2) {
                        units.push(u16::from_le_bytes([pair[0], pair[1]]));
                    }
                }
                lfn.push((seq, units));
                continue;
            }
            if raw[11] & 0x08 != 0 {
                lfn.clear(); // volume label
                continue;
            }
            let short = short_name(raw, deleted);
            let name = if lfn.is_empty() {
                short
            } else {
                lfn.sort_by_key(|(seq, _)| *seq);
                let units: Vec<u16> = lfn
                    .iter()
                    .flat_map(|(_, u)| u.iter().copied())
                    .take_while(|u| *u != 0x0000 && *u != 0xFFFF)
                    .collect();
                lfn.clear();
                String::from_utf16_lossy(&units)
            };
            if !deleted && (name == "." || name == "..") {
                continue;
            }
            let is_dir = raw[11] & 0x10 != 0;
            let hi = le16(raw, 20).unwrap_or(0) as u32;
            let lo = le16(raw, 26).unwrap_or(0) as u32;
            let first_cluster = (hi << 16) | lo;
            let size = le32(raw, 28).unwrap_or(0) as u64;
            let path = format!("{prefix}/{name}");
            out.push(FileEntry {
                path: path.clone(),
                is_dir,
                size,
                first_cluster,
                deleted,
                contiguous: deleted,
            });
            if is_dir && !deleted && first_cluster >= 2 && seen.insert(first_cluster) {
                self.walk(first_cluster, &path, depth + 1, seen, out);
            }
        }
    }
}

fn short_name(raw: &[u8], deleted: bool) -> String {
    let mut base: Vec<u8> = raw[0..8].to_vec();
    if raw[0] == 0x05 {
        base[0] = 0xE5;
    }
    if deleted {
        base[0] = b'_'; // first character is lost on deletion
    }
    let base = String::from_utf8_lossy(&base).trim_end().to_string();
    let ext = String::from_utf8_lossy(&raw[8..11]).trim_end().to_string();
    if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    }
}

impl Filesystem for Fat32<'_> {
    fn kind(&self) -> VolumeKind {
        VolumeKind::Fat32
    }

    fn entries(&self) -> Vec<FileEntry> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        seen.insert(self.root_cluster);
        self.walk(self.root_cluster, "", 0, &mut seen, &mut out);
        out
    }

    fn read(&self, e: &FileEntry) -> Result<Vec<u8>, FsError> {
        if e.is_dir {
            return Err(FsError::IsDirectory);
        }
        let want = e.size as usize;
        let clusters = if e.contiguous {
            self.cluster_run(e.first_cluster, e.size)
        } else {
            self.chain(e.first_cluster)
        };
        let mut out = Vec::with_capacity(want.min(1 << 30));
        for c in clusters {
            if out.len() >= want {
                break;
            }
            let Some(off) = self.cluster_abs(c) else {
                break;
            };
            let take = (want - out.len()).min(self.cluster_bytes);
            match self.img.get(off..off + take) {
                Some(bytes) => out.extend_from_slice(bytes),
                None => break, // cluster runs past the end of the image
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod test_image {
    //! Builds a small FAT32 volume in memory for tests.
    //!
    //! Layout: 64 sectors of 512 bytes. Reserved 32, one FAT of 1 sector, data from sector 33.
    //! Root (cluster 2): HELLO.TXT (c3), a deleted SECRET.DAT (c6), LongName.txt via LFN (c9),
    //! CHAIN.BIN (c4->c5, 1000 bytes), and DOCS/ (c7) containing README.TXT (c8).

    pub const SS: usize = 512;

    pub fn build() -> Vec<u8> {
        let mut img = vec![0u8; 64 * SS];
        // Boot sector.
        let b = &mut img[..SS];
        b[11..13].copy_from_slice(&512u16.to_le_bytes());
        b[13] = 1;
        b[14..16].copy_from_slice(&32u16.to_le_bytes());
        b[16] = 1;
        b[32..36].copy_from_slice(&64u32.to_le_bytes());
        b[36..40].copy_from_slice(&1u32.to_le_bytes());
        b[44..48].copy_from_slice(&2u32.to_le_bytes());
        b[82..90].copy_from_slice(b"FAT32   ");
        b[510] = 0x55;
        b[511] = 0xAA;

        // FAT at sector 32.
        let fat = 32 * SS;
        let put = |img: &mut Vec<u8>, c: usize, v: u32| {
            img[fat + c * 4..fat + c * 4 + 4].copy_from_slice(&v.to_le_bytes())
        };
        put(&mut img, 0, 0x0FFF_FFF8);
        put(&mut img, 1, 0x0FFF_FFFF);
        put(&mut img, 2, 0x0FFF_FFFF); // root
        put(&mut img, 3, 0x0FFF_FFFF); // HELLO.TXT
        put(&mut img, 4, 5); // CHAIN.BIN
        put(&mut img, 5, 0x0FFF_FFFF);
        put(&mut img, 6, 0); // deleted: chain freed
        put(&mut img, 7, 0x0FFF_FFFF); // DOCS
        put(&mut img, 8, 0x0FFF_FFFF); // README.TXT
        put(&mut img, 9, 0x0FFF_FFFF); // LongName.txt

        let cluster = |c: usize| (33 + (c - 2)) * SS;

        // Root directory.
        let root = cluster(2);
        let mut ents: Vec<[u8; 32]> = Vec::new();
        ents.push(short(b"HELLO   ", b"TXT", 0x20, 3, 11));
        let mut deleted = short(b"SECRET  ", b"DAT", 0x20, 6, 300);
        deleted[0] = 0xE5;
        ents.push(deleted);
        ents.extend(lfn("LongName.txt", short_checksum(b"LONGNA~1", b"TXT")));
        ents.push(short(b"LONGNA~1", b"TXT", 0x20, 9, 4));
        ents.push(short(b"CHAIN   ", b"BIN", 0x20, 4, 1000));
        ents.push(short(b"DOCS    ", b"", 0x10, 7, 0));
        write_entries(&mut img, root, &ents);

        // HELLO.TXT content.
        img[cluster(3)..cluster(3) + 11].copy_from_slice(b"hello world");
        // CHAIN.BIN: 1000 bytes across clusters 4 and 5.
        for (i, byte) in img[cluster(4)..cluster(4) + 512].iter_mut().enumerate() {
            *byte = (i % 251) as u8;
        }
        for (i, byte) in img[cluster(5)..cluster(5) + 488].iter_mut().enumerate() {
            *byte = ((i + 512) % 251) as u8;
        }
        // LongName.txt content in cluster 9.
        img[cluster(9)..cluster(9) + 4].copy_from_slice(b"long");
        // DOCS directory (cluster 7) holds README.TXT -> cluster 8.
        let docs = vec![
            short(b".       ", b"", 0x10, 7, 0),
            short(b"..      ", b"", 0x10, 0, 0),
            short(b"README  ", b"TXT", 0x20, 8, 4),
        ];
        write_entries(&mut img, cluster(7), &docs);
        img[cluster(8)..cluster(8) + 4].copy_from_slice(b"data");
        img
    }

    fn short(name: &[u8; 8], ext: &[u8], attr: u8, first: u32, size: u32) -> [u8; 32] {
        let mut e = [b' '; 32];
        e[0..8].copy_from_slice(name);
        e[8..8 + ext.len()].copy_from_slice(ext);
        e[11] = attr;
        e[20..22].copy_from_slice(&((first >> 16) as u16).to_le_bytes());
        e[26..28].copy_from_slice(&(first as u16).to_le_bytes());
        e[28..32].copy_from_slice(&size.to_le_bytes());
        e
    }

    fn short_checksum(name: &[u8; 8], ext: &[u8]) -> u8 {
        let mut sn = [b' '; 11];
        sn[..8].copy_from_slice(name);
        sn[8..8 + ext.len()].copy_from_slice(ext);
        sn.iter()
            .fold(0u8, |sum, &b| sum.rotate_right(1).wrapping_add(b))
    }

    /// Builds LFN entries (highest sequence first) for a name of at most 13 characters.
    fn lfn(name: &str, checksum: u8) -> Vec<[u8; 32]> {
        let mut units: Vec<u16> = name.encode_utf16().collect();
        units.push(0);
        while units.len() < 13 {
            units.push(0xFFFF);
        }
        let mut e = [0xFFu8; 32];
        e[0] = 0x41; // last + sequence 1
        e[11] = 0x0F;
        e[13] = checksum;
        let positions = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        for (i, &p) in positions.iter().enumerate() {
            e[p..p + 2].copy_from_slice(&units[i].to_le_bytes());
        }
        vec![e]
    }

    fn write_entries(img: &mut [u8], at: usize, ents: &[[u8; 32]]) {
        for (i, e) in ents.iter().enumerate() {
            img[at + i * 32..at + i * 32 + 32].copy_from_slice(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fat32::test_image::build;

    fn fs_of(img: &[u8]) -> Fat32<'_> {
        Fat32::open(img, 0).expect("valid FAT32 boot sector")
    }

    #[test]
    fn lists_live_and_deleted_entries_with_lfn() {
        let img = build();
        let fs = fs_of(&img);
        let entries = fs.entries();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"/HELLO.TXT"), "{paths:?}");
        assert!(paths.contains(&"/LongName.txt"), "{paths:?}");
        assert!(paths.contains(&"/DOCS"));
        assert!(paths.contains(&"/DOCS/README.TXT"));
        let secret = entries
            .iter()
            .find(|e| e.path.ends_with("ECRET.DAT"))
            .expect("deleted entry");
        assert!(secret.deleted && secret.contiguous);
    }

    #[test]
    fn reads_file_via_fat_chain_across_clusters() {
        let img = build();
        let fs = fs_of(&img);
        let entries = fs.entries();
        let chain = entries.iter().find(|e| e.path == "/CHAIN.BIN").unwrap();
        let data = fs.read(chain).unwrap();
        assert_eq!(data.len(), 1000);
        assert_eq!(data[0], 0);
        assert_eq!(data[600], ((88 + 512) % 251) as u8); // byte 88 of cluster 5
    }

    #[test]
    fn reads_nested_file_and_recovers_deleted_contiguous_data() {
        let img = build();
        let fs = fs_of(&img);
        let entries = fs.entries();
        let readme = entries
            .iter()
            .find(|e| e.path == "/DOCS/README.TXT")
            .unwrap();
        assert_eq!(fs.read(readme).unwrap(), b"data");
        // The deleted entry's FAT chain was freed; contiguous recovery reads cluster 6 anyway.
        let secret = entries.iter().find(|e| e.deleted).unwrap();
        assert_eq!(fs.read(secret).unwrap().len(), 300);
    }

    #[test]
    fn rejects_non_fat32_and_bad_geometry() {
        let mut img = build();
        assert!(matches!(
            Fat32::open(&vec![0u8; 512], 0),
            Err(FsError::NotThisFilesystem(_))
        ));
        img[11..13].copy_from_slice(&300u16.to_le_bytes());
        assert!(matches!(Fat32::open(&img, 0), Err(FsError::Corrupt(_))));
    }

    #[test]
    fn cycle_in_fat_terminates() {
        let mut img = build();
        // Point cluster 3 at itself.
        let fat = 32 * 512;
        img[fat + 12..fat + 16].copy_from_slice(&3u32.to_le_bytes());
        let fs = fs_of(&img);
        let entries = fs.entries();
        let hello = entries.iter().find(|e| e.path == "/HELLO.TXT").unwrap();
        assert_eq!(fs.read(hello).unwrap().len(), 11);
    }
}

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use crate::fat32::test_image::build;
    use proptest::prelude::*;

    proptest! {
        /// Corrupted metadata must produce errors or partial results, never a panic or hang.
        #[test]
        fn mutated_volume_never_panics(flips in proptest::collection::vec((0usize..64 * 512, any::<u8>()), 0..96)) {
            let mut img = build();
            for (i, v) in flips {
                img[i] = v;
            }
            if let Ok(fs) = Fat32::open(&img, 0) {
                for e in fs.entries() {
                    let _ = fs.read(&e);
                }
            }
            let _ = crate::detect(&img);
        }
    }
}
