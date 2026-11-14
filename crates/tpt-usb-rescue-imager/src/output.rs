//! Image writer.
//!
//! Unrecovered sectors are left unwritten. On a freshly created file those ranges read back as
//! zeroes, which keeps byte alignment intact and avoids allocating disk space for bad areas.
//! All-zero blocks from a fresh image are skipped for the same reason. Resumed images always
//! write every recovered block, because an earlier run may have left data at that offset.

use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

pub struct OutputImage {
    file: File,
    total_bytes: u64,
    fresh: bool,
}

impl OutputImage {
    /// Creates (or truncates) the image and sizes it to `total_bytes`.
    pub fn create(path: &Path, total_bytes: u64) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(total_bytes)?;
        Ok(Self {
            file,
            total_bytes,
            fresh: true,
        })
    }

    /// Opens an existing image for a resumed run, creating it if missing.
    pub fn open_resume(path: &Path, total_bytes: u64) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if file.metadata()?.len() < total_bytes {
            file.set_len(total_bytes)?;
        }
        Ok(Self {
            file,
            total_bytes,
            fresh: false,
        })
    }

    /// Writes `data` at byte `offset`.
    pub fn write_block(&mut self, offset: u64, data: &[u8]) -> io::Result<()> {
        if offset + data.len() as u64 > self.total_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write beyond image size",
            ));
        }
        if self.fresh && data.iter().all(|&b| b == 0) {
            return Ok(());
        }
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)
    }

    /// Flushes written data to stable storage.
    pub fn sync(&self) -> io::Result<()> {
        self.file.sync_data()
    }

    /// Ensures the file is exactly `total_bytes` long and syncs it.
    pub fn finish(&self) -> io::Result<()> {
        self.file.set_len(self.total_bytes)?;
        self.file.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_zero_blocks_are_skipped_but_data_is_kept() {
        let dir = std::env::temp_dir().join("tpt-output-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("sparse.img");
        let mut out = OutputImage::create(&p, 4096).unwrap();
        out.write_block(0, &[0u8; 512]).unwrap();
        out.write_block(512, &[7u8; 512]).unwrap();
        out.finish().unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(bytes.len(), 4096);
        assert!(bytes[..512].iter().all(|&b| b == 0));
        assert!(bytes[512..1024].iter().all(|&b| b == 7));
    }

    #[test]
    fn rejects_writes_past_end() {
        let dir = std::env::temp_dir().join("tpt-output-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bounds.img");
        let mut out = OutputImage::create(&p, 512).unwrap();
        assert!(out.write_block(256, &[1u8; 512]).is_err());
    }
}
