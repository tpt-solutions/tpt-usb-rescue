//! Writes carved files and an index manifest to a directory.

use crate::integrity::sha256_hex;
use crate::{CarvedFile, FileType};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const INDEX_NAME: &str = "index.csv";
const INDEX_HEADER: &str =
    "id,type,start_offset,end_offset,size_bytes,integrity_score,is_fragmented,file,sha256";

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
            "{},{},{},{},{},{:.3},{},{},{}",
            f.id,
            f.file_type.name(),
            f.start_offset,
            f.end_offset,
            f.size_bytes,
            f.integrity_score,
            f.is_fragmented,
            name,
            sha256_hex(bytes)
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
    /// SHA-256 of the carved file, when the index records one (older indexes do not).
    pub sha256: Option<String>,
}

/// Parses an index written by [`write_carved`].
pub fn read_index(path: &Path) -> io::Result<Vec<IndexRow>> {
    let text = fs::read_to_string(path)?;
    let bad = |msg: String| io::Error::new(io::ErrorKind::InvalidData, msg);
    let mut rows = Vec::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() != 8 && cols.len() != 9 {
            return Err(bad(format!("bad index row: {line}")));
        }
        let num = |s: &str| s.parse::<u64>().map_err(|e| bad(e.to_string()));
        // The index is joined onto the output directory later; it must only name files in it.
        let file = cols[7];
        let plain = !file.is_empty()
            && file != "."
            && file != ".."
            && !file.contains(['/', '\\', ':', '\0']);
        if !plain {
            return Err(bad(format!(
                "index names a file outside the output folder: {file}"
            )));
        }
        rows.push(IndexRow {
            id: num(cols[0])?,
            file_type: cols[1].to_string(),
            start_offset: num(cols[2])?,
            end_offset: num(cols[3])?,
            file: cols[7].to_string(),
            sha256: match cols.get(8) {
                Some(h) if h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) => {
                    Some(h.to_ascii_lowercase())
                }
                Some(h) => return Err(bad(format!("bad sha256 in index: {h}"))),
                None => None,
            },
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

#[cfg(test)]
mod index_tests {
    use super::*;

    fn index_with(file: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("tpt-index-tests");
        fs::create_dir_all(&dir).unwrap();
        let unique: String = file
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let p = dir.join(format!("{unique}.csv"));
        fs::write(
            &p,
            format!(
                "id,type,start,end,size,score,frag,file
1,jpg,0,10,10,1.0,0,{file}
"
            ),
        )
        .unwrap();
        p
    }

    #[test]
    fn plain_names_are_accepted() {
        assert!(read_index(&index_with("000001.jpg")).is_ok());
    }

    #[test]
    fn names_that_escape_the_folder_are_rejected() {
        for bad in ["../x.jpg", "a/b.jpg", r"..\x", "C:evil.jpg", ".."] {
            assert!(read_index(&index_with(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn digest_column_round_trips_and_old_indexes_still_read() {
        let dir = std::env::temp_dir().join("tpt-index-digest");
        let _ = fs::remove_dir_all(&dir);
        let data = b"xxxxHELLOyyyy".to_vec();
        let file = CarvedFile {
            id: 1,
            file_type: FileType::Png,
            start_offset: 4,
            end_offset: 9,
            size_bytes: 5,
            integrity_score: 1.0,
            is_fragmented: false,
        };
        let index = write_carved(&data, &[file], &dir).unwrap();
        let rows = read_index(&index).unwrap();
        assert_eq!(
            rows[0].sha256.as_deref(),
            Some(sha256_hex(b"HELLO").as_str())
        );
        // An index without the column (written by an older version) has no digest.
        assert_eq!(
            read_index(&index_with("000001.png")).unwrap()[0].sha256,
            None
        );
        // A malformed digest is refused.
        fs::write(
            &index,
            "id,type,start,end,size,score,frag,file,sha256
1,png,4,9,5,1.0,false,a.png,nothex
",
        )
        .unwrap();
        assert!(read_index(&index).is_err());
    }
}
