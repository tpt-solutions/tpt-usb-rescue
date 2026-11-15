//! exFAT parser: boot sector, FAT chains or contiguous runs, and file-set directory entries.

use crate::{FileEntry, Filesystem, FsError, VolumeKind, le32, slice};
use std::collections::HashSet;

const MAX_DEPTH: u32 = 32;
const EOC_MIN: u32 = 0xFFFF_FFF8;
const ENTRY_FILE: u8 = 0x05;
const ENTRY_NAME: u8 = 0x41;
const IN_USE: u8 = 0x80;

pub struct ExFat<'a> {
    img: &'a [u8],
    fat_offset: usize,
    heap_offset: usize,
    cluster_bytes: usize,
    cluster_count: u32,
    root_cluster: u32,
}

/// Where a directory's data lives: its first cluster, stored length, and NoFatChain flag.
#[derive(Clone, Copy)]
struct DirLoc {
    first: u32,
    size: u64,
    contiguous: bool,
}

impl<'a> ExFat<'a> {
    pub fn open(img: &'a [u8], base: usize) -> Result<Self, FsError> {
        let b = slice(img, base, 512)?;
        if &b[3..11] != b"EXFAT   " || b[510..512] != [0x55, 0xAA] {
            return Err(FsError::NotThisFilesystem("exFAT"));
        }
        let corrupt = || FsError::Corrupt("boot sector");
        let fat_sector = le32(b, 80).ok_or_else(corrupt)? as usize;
        let fat_len = le32(b, 84).ok_or_else(corrupt)? as usize;
        let heap_sector = le32(b, 88).ok_or_else(corrupt)? as usize;
        let cluster_count = le32(b, 92).ok_or_else(corrupt)?;
        let root_cluster = le32(b, 96).ok_or_else(corrupt)?;
        let bytes_shift = b[108];
        let spc_shift = b[109];

        if !(9..=12).contains(&bytes_shift) {
            return Err(FsError::Corrupt("bytes per sector shift"));
        }
        if spc_shift > 25 {
            return Err(FsError::Corrupt("sectors per cluster shift"));
        }
        let bps = 1usize << bytes_shift;
        let cluster_bytes = bps << spc_shift;
        if cluster_bytes > 32 << 20 {
            return Err(FsError::Corrupt("cluster size"));
        }
        if root_cluster < 2 || root_cluster > cluster_count.saturating_add(1) {
            return Err(FsError::Corrupt("root cluster"));
        }
        if fat_len == 0 || fat_sector == 0 {
            return Err(FsError::Corrupt("FAT location"));
        }
        Ok(Self {
            img,
            fat_offset: base + fat_sector * bps,
            heap_offset: base + heap_sector * bps,
            cluster_bytes,
            cluster_count,
            root_cluster,
        })
    }

    fn cluster_abs(&self, c: u32) -> Option<usize> {
        if c < 2 || c - 2 >= self.cluster_count {
            return None;
        }
        Some(self.heap_offset + (c - 2) as usize * self.cluster_bytes)
    }

    fn next(&self, c: u32) -> Option<u32> {
        le32(self.img, self.fat_offset.checked_add(c as usize * 4)?)
    }

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

    fn run(&self, start: u32, bytes: u64) -> Vec<u32> {
        let n = bytes.div_ceil(self.cluster_bytes as u64) as u32;
        (0..n)
            .map(|i| start.saturating_add(i))
            .take_while(|c| self.cluster_abs(*c).is_some())
            .collect()
    }

    fn clusters_for(&self, first: u32, size: u64, contiguous: bool) -> Vec<u32> {
        if contiguous {
            self.run(first, size)
        } else {
            self.chain(first)
        }
    }

    /// Reads a directory's bytes. A NoFatChain directory (`contiguous`) has no FAT chain, so its
    /// clusters come from its stored length instead.
    fn dir_bytes(&self, first: u32, size: u64, contiguous: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for c in self.clusters_for(first, size, contiguous) {
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
        dir: DirLoc,
        prefix: &str,
        depth: u32,
        seen: &mut HashSet<u32>,
        out: &mut Vec<FileEntry>,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        let dir = self.dir_bytes(dir.first, dir.size, dir.contiguous);
        let mut i = 0usize;
        while i + 32 <= dir.len() {
            let t = dir[i];
            if t == 0x00 {
                break; // end of directory
            }
            let base_type = t & !IN_USE;
            let in_use = t & IN_USE != 0;
            if base_type != ENTRY_FILE {
                i += 32;
                continue;
            }
            let secondary = dir[i + 1] as usize;
            let set_end = i + 32 * (1 + secondary);
            if secondary < 2 || set_end > dir.len() {
                break; // malformed set; stop rather than guess
            }
            let attrs = u16::from_le_bytes([dir[i + 4], dir[i + 5]]);
            let stream = &dir[i + 32..i + 64];
            let stream_flags = stream[1];
            let name_len = stream[3] as usize;
            let first_cluster =
                u32::from_le_bytes([stream[20], stream[21], stream[22], stream[23]]);
            let size = u64::from_le_bytes([
                stream[24], stream[25], stream[26], stream[27], stream[28], stream[29], stream[30],
                stream[31],
            ]);
            let mut units: Vec<u16> = Vec::with_capacity(name_len);
            for name_entry in dir[i + 64..set_end].chunks_exact(32) {
                if name_entry[0] & !IN_USE != ENTRY_NAME {
                    break;
                }
                for pair in name_entry[2..32].chunks_exact(2) {
                    units.push(u16::from_le_bytes([pair[0], pair[1]]));
                }
            }
            units.truncate(name_len);
            let name = String::from_utf16_lossy(&units);
            let deleted = !in_use;
            let is_dir = attrs & 0x10 != 0;
            // NoFatChain (bit 1) means the data is a contiguous run.
            let contiguous = deleted || stream_flags & 0x02 != 0;
            let path = format!("{prefix}/{name}");
            out.push(FileEntry {
                path: path.clone(),
                is_dir,
                size,
                first_cluster,
                deleted,
                contiguous,
            });
            if is_dir && !deleted && first_cluster >= 2 && seen.insert(first_cluster) {
                let sub = DirLoc {
                    first: first_cluster,
                    size,
                    contiguous,
                };
                self.walk(sub, &path, depth + 1, seen, out);
            }
            i = set_end;
        }
    }
}

impl Filesystem for ExFat<'_> {
    fn kind(&self) -> VolumeKind {
        VolumeKind::ExFat
    }

    fn entries(&self) -> Vec<FileEntry> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        seen.insert(self.root_cluster);
        let root = DirLoc {
            first: self.root_cluster,
            size: 0,
            contiguous: false,
        };
        self.walk(root, "", 0, &mut seen, &mut out);
        out
    }

    fn read(&self, e: &FileEntry) -> Result<Vec<u8>, FsError> {
        if e.is_dir {
            return Err(FsError::IsDirectory);
        }
        let want = e.size as usize;
        let mut out = Vec::with_capacity(want.min(self.img.len()));
        for c in self.clusters_for(e.first_cluster, e.size, e.contiguous) {
            if out.len() >= want {
                break;
            }
            let Some(off) = self.cluster_abs(c) else {
                break;
            };
            let take = (want - out.len()).min(self.cluster_bytes);
            match self.img.get(off..off + take) {
                Some(bytes) => out.extend_from_slice(bytes),
                None => break,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 512-byte sectors, 1 sector per cluster. FAT at sector 2, heap at sector 4, root at cluster 2.
    fn build() -> Vec<u8> {
        let mut img = vec![0u8; 64 * 512];
        let b = &mut img[..512];
        b[3..11].copy_from_slice(b"EXFAT   ");
        b[80..84].copy_from_slice(&2u32.to_le_bytes()); // FatOffset
        b[84..88].copy_from_slice(&1u32.to_le_bytes()); // FatLength
        b[88..92].copy_from_slice(&4u32.to_le_bytes()); // ClusterHeapOffset
        b[92..96].copy_from_slice(&50u32.to_le_bytes()); // ClusterCount
        b[96..100].copy_from_slice(&2u32.to_le_bytes()); // root cluster
        b[108] = 9;
        b[109] = 0;
        b[510] = 0x55;
        b[511] = 0xAA;

        let fat = 2 * 512;
        let put = |img: &mut Vec<u8>, c: usize, v: u32| {
            img[fat + c * 4..fat + c * 4 + 4].copy_from_slice(&v.to_le_bytes())
        };
        put(&mut img, 2, 0xFFFF_FFFF); // root
        put(&mut img, 3, 4); // multi-cluster file via chain
        put(&mut img, 4, 0xFFFF_FFFF);
        put(&mut img, 5, 0xFFFF_FFFF); // file 'contig' uses NoFatChain, FAT left as-is

        let cluster = |c: usize| (4 + (c - 2)) * 512;
        let root = cluster(2);

        // File set helper.
        let set = |name: &str,
                   first: u32,
                   size: u64,
                   contiguous: bool,
                   dir: bool,
                   in_use: bool|
         -> Vec<u8> {
            let units: Vec<u16> = name.encode_utf16().collect();
            let name_entries = units.len().div_ceil(15);
            let mut v = vec![0u8; 32 * (2 + name_entries)];
            v[0] = if in_use { 0x85 } else { 0x05 };
            v[1] = (1 + name_entries) as u8;
            v[4..6].copy_from_slice(&(if dir { 0x10u16 } else { 0x20u16 }).to_le_bytes());
            let s = 32;
            v[s] = if in_use { 0xC0 } else { 0x40 };
            v[s + 1] = if contiguous { 0x03 } else { 0x01 };
            v[s + 3] = units.len() as u8;
            v[s + 20..s + 24].copy_from_slice(&first.to_le_bytes());
            v[s + 24..s + 32].copy_from_slice(&size.to_le_bytes());
            for (k, chunk) in units.chunks(15).enumerate() {
                let e = 64 + 32 * k;
                v[e] = if in_use { 0xC1 } else { 0x41 };
                for (j, u) in chunk.iter().enumerate() {
                    v[e + 2 + j * 2..e + 4 + j * 2].copy_from_slice(&u.to_le_bytes());
                }
            }
            v
        };

        let mut dir = Vec::new();
        dir.extend(set("note.txt", 3, 600, false, false, true));
        dir.extend(set("gone.bin", 5, 200, true, false, false));
        dir.extend(set("photos", 6, 0, false, true, true));
        dir.extend(set("big", 8, 1024, true, true, true));
        img[root..root + dir.len()].copy_from_slice(&dir);

        // note.txt occupies clusters 3 and 4.
        for i in 0..600 {
            img[cluster(3) + i.min(511)] = b'n';
            if i >= 512 {
                img[cluster(4) + i - 512] = b'm';
            }
        }
        // gone.bin (deleted, contiguous) lives in cluster 5.
        img[cluster(5)..cluster(5) + 200].copy_from_slice(&[0xAB; 200]);

        // photos/ (cluster 6) holds a.jpg in cluster 7.
        let sub = set("a.jpg", 7, 3, true, false, true);
        img[cluster(6)..cluster(6) + sub.len()].copy_from_slice(&sub);
        img[cluster(7)..cluster(7) + 3].copy_from_slice(b"JPG");

        // big/ is a NoFatChain directory spanning clusters 8 and 9 with no FAT chain. Its first
        // cluster is filled with non-file entries; the real entry sits in the second cluster.
        for k in 0..16 {
            img[cluster(8) + k * 32] = 0x81;
        }
        let deep = set("deep.txt", 10, 4, true, false, true);
        img[cluster(9)..cluster(9) + deep.len()].copy_from_slice(&deep);
        img[cluster(10)..cluster(10) + 4].copy_from_slice(b"deep");
        img
    }

    #[test]
    fn reads_nofatchain_directory_beyond_first_cluster() {
        let img = build();
        let fs = ExFat::open(&img, 0).unwrap();
        let entries = fs.entries();
        let deep = entries
            .iter()
            .find(|e| e.path == "/big/deep.txt")
            .expect("entry in second cluster of a contiguous directory");
        assert_eq!(fs.read(deep).unwrap(), b"deep");
    }

    #[test]
    fn lists_files_and_deleted_entries() {
        let img = build();
        let fs = ExFat::open(&img, 0).unwrap();
        let paths: Vec<String> = fs.entries().into_iter().map(|e| e.path).collect();
        assert!(paths.contains(&"/note.txt".to_string()), "{paths:?}");
        assert!(paths.contains(&"/photos".to_string()));
        assert!(paths.contains(&"/gone.bin".to_string()));
    }

    #[test]
    fn reads_chained_file_and_contiguous_deleted_file() {
        let img = build();
        let fs = ExFat::open(&img, 0).unwrap();
        let entries = fs.entries();
        let note = entries.iter().find(|e| e.path == "/note.txt").unwrap();
        let data = fs.read(note).unwrap();
        assert_eq!(data.len(), 600);
        assert_eq!(data[0], b'n');
        assert_eq!(data[599], b'm');

        let gone = entries.iter().find(|e| e.path == "/gone.bin").unwrap();
        assert!(gone.deleted);
        assert_eq!(fs.read(gone).unwrap(), vec![0xAB; 200]);
    }

    #[test]
    fn reads_file_in_subdirectory() {
        let img = build();
        let fs = ExFat::open(&img, 0).unwrap();
        let entries = fs.entries();
        let jpg = entries.iter().find(|e| e.path == "/photos/a.jpg").unwrap();
        assert_eq!(fs.read(jpg).unwrap(), b"JPG");
    }

    #[test]
    fn rejects_other_filesystems() {
        assert!(matches!(
            ExFat::open(&vec![0u8; 1024], 0),
            Err(FsError::NotThisFilesystem(_))
        ));
    }
}

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn mutated_volume_never_panics(flips in proptest::collection::vec((0usize..64 * 512, any::<u8>()), 0..96)) {
            let mut img = vec![0u8; 64 * 512];
            img[3..11].copy_from_slice(b"EXFAT   ");
            img[80..84].copy_from_slice(&2u32.to_le_bytes());
            img[84..88].copy_from_slice(&1u32.to_le_bytes());
            img[88..92].copy_from_slice(&4u32.to_le_bytes());
            img[92..96].copy_from_slice(&50u32.to_le_bytes());
            img[96..100].copy_from_slice(&2u32.to_le_bytes());
            img[108] = 9;
            img[510] = 0x55;
            img[511] = 0xAA;
            for (i, v) in flips {
                img[i] = v;
            }
            if let Ok(fs) = ExFat::open(&img, 0) {
                for e in fs.entries() {
                    let _ = fs.read(&e);
                }
            }
        }
    }
}
