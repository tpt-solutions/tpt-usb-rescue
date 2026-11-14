//! Writes carved files and an index manifest to a directory.

use crate::{CarvedFile, FileType};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const INDEX_NAME: &str = "index.csv";
const INDEX_HEADER: &str =
    "id,type,start_offset,end_offset,size_bytes,integrity_score,is_fragmented,file";

/// Writes every carved file as `file_<id>.<ext>` plus `index.csv`. Returns the index path.
pub fn write_carved(data: &[u8], files: &[CarvedFile], dir: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let index = dir.join(INDEX_NAME);
    let mut w = io::BufWriter::new(fs::File::create(&index)?);
    writeln!(w, "{INDEX_HEADER}")?;
    for f in files {
        let name = file_name(f);
        let bytes = &data[f.start_offset as usize..f.end_offset as usize];
        fs::write(dir.join(&name), bytes)?;
        writeln!(
            w,
            "{},{},{},{},{},{:.3},{},{}",
            f.id,
            f.file_type.name(),
            f.start_offset,
            f.end_offset,
            f.size_bytes,
            f.integrity_score,
            f.is_fragmented,
            name
        )?;
    }
    w.flush()?;
    Ok(index)
}

/// Name used for a carved file inside the output directory.
pub fn file_name(f: &CarvedFile) -> String {
    format!("file_{:06}.{}", f.id, f.file_type.extension())
}

/// One row of `index.csv`, as read back by verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub id: u64,
    pub file_type: String,
    pub start_offset: u64,
    pub end_offset: u64,
    pub file: String,
}

/// Parses an index written by [`write_carved`].
pub fn read_index(path: &Path) -> io::Result<Vec<IndexRow>> {
    let text = fs::read_to_string(path)?;
    let bad = |msg: String| io::Error::new(io::ErrorKind::InvalidData, msg);
    let mut rows = Vec::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() != 8 {
            return Err(bad(format!("bad index row: {line}")));
        }
        let num = |s: &str| s.parse::<u64>().map_err(|e| bad(e.to_string()));
        rows.push(IndexRow {
            id: num(cols[0])?,
            file_type: cols[1].to_string(),
            start_offset: num(cols[2])?,
            end_offset: num(cols[3])?,
            file: cols[7].to_string(),
        });
    }
    Ok(rows)
}

/// Maps an index type name back to its [`FileType`].
pub fn type_from_name(name: &str) -> Option<FileType> {
    [
        FileType::Jpeg,
        FileType::Png,
        FileType::Pdf,
        FileType::Gif,
        FileType::Zip,
    ]
    .into_iter()
    .find(|t| t.name() == name)
}
