//! Parsing of `/proc/self/mountinfo`, used to find which block device backs a path.
//!
//! The file format is the same on every platform that has it, so the parser is compiled and
//! tested everywhere even though only the Linux backend calls it.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::Path;

/// Returns the mount source (for example `/dev/sda2` or `tmpfs`) of the mount that contains
/// `path`, choosing the mount point with the longest matching prefix.
pub(crate) fn mount_source(mountinfo: &str, path: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mountinfo.lines() {
        let Some((pre, post)) = line.split_once(" - ") else {
            continue;
        };
        let fields: Vec<&str> = pre.split(' ').collect();
        let Some(mount_point) = fields.get(4) else {
            continue;
        };
        let mount_point = unescape(mount_point);
        if !path.starts_with(&mount_point) {
            continue;
        }
        // `post` is "fstype source options".
        let Some(source) = post.split(' ').nth(1) else {
            continue;
        };
        let depth = Path::new(&mount_point).components().count();
        if best.as_ref().is_none_or(|(d, _)| depth >= *d) {
            best = Some((depth, unescape(source)));
        }
    }
    best.map(|(_, source)| source)
}

/// Decodes the `\040`-style octal escapes the kernel uses for spaces and similar characters.
fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let v = u32::from(bytes[i + 1] - b'0') * 64
                + u32::from(bytes[i + 2] - b'0') * 8
                + u32::from(bytes[i + 3] - b'0');
            if let Ok(v) = u8::try_from(v) {
                out.push(v);
                i += 4;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "\
22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/mapper/vg-root rw,subvol=/
30 22 8:17 / /mnt/usb\\040stick rw,relatime - vfat /dev/sdb1 rw
31 22 0:30 / /tmp rw - tmpfs tmpfs rw
";

    #[test]
    fn longest_mount_wins() {
        assert_eq!(
            mount_source(INFO, Path::new("/home/a/out.img")).as_deref(),
            Some("/dev/mapper/vg-root")
        );
        assert_eq!(
            mount_source(INFO, Path::new("/tmp/x")).as_deref(),
            Some("tmpfs")
        );
    }

    #[test]
    fn out_of_range_escape_is_left_alone() {
        assert_eq!(unescape(r"a\777b"), r"a\777b");
    }

    #[test]
    fn decodes_escaped_mount_points() {
        assert_eq!(
            mount_source(INFO, Path::new("/mnt/usb stick/out.img")).as_deref(),
            Some("/dev/sdb1")
        );
    }
}
