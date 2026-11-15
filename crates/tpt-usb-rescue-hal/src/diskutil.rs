//! Reads `diskutil info -plist` output for the macOS system-disk guard.
//!
//! On macOS the root volume is an APFS volume inside a synthesized container disk (for example
//! `disk3`), and that container is backed by a physical partition (for example `disk0s2`). A
//! device-number comparison cannot see that link, so the guard follows it with `diskutil`.
//!
//! This module only parses text, so it builds and is tested on every platform. The macOS backend
//! runs `diskutil` and passes the output here.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// The keys of `diskutil info -plist` that the safety guard needs.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DiskInfo {
    /// `DeviceIdentifier`: the node itself, such as `disk3s1s1`.
    pub identifier: Option<String>,
    /// `ParentWholeDisk`: the whole disk that holds the node, such as `disk3`.
    pub parent_whole_disk: Option<String>,
    /// `APFSPhysicalStores`: the partitions that back an APFS container, such as `disk0s2`.
    pub physical_stores: Vec<String>,
}

/// Reads the keys above from `diskutil info -plist` XML.
pub(crate) fn parse_info(xml: &str) -> DiskInfo {
    DiskInfo {
        identifier: string_value(xml, "DeviceIdentifier"),
        parent_whole_disk: string_value(xml, "ParentWholeDisk"),
        physical_stores: physical_stores(xml),
    }
}

/// The whole disk a node name belongs to: `disk3s1s1` and `disk3` both give `disk3`.
/// Returns `None` for names that are not `disk<digits>` with an optional `s<digits>` suffix.
pub(crate) fn whole_disk(name: &str) -> Option<String> {
    let rest = name.strip_prefix("disk")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let tail = &rest[digits..];
    if !tail.is_empty() && !tail.starts_with('s') {
        return None;
    }
    Some(format!("disk{}", &rest[..digits]))
}

/// The text of the `<string>` that immediately follows `<key>key</key>`. Anything else in
/// between (a `<dict>` or `<array>`, for example) means the key is not a plain string.
fn string_value(xml: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let rest = xml[xml.find(&needle)? + needle.len()..].trim_start();
    let rest = rest.strip_prefix("<string>")?;
    Some(rest[..rest.find("</string>")?].trim().to_string())
}

/// Every whole-disk name listed in the `APFSPhysicalStores` array.
fn physical_stores(xml: &str) -> Vec<String> {
    let needle = "<key>APFSPhysicalStores</key>";
    let Some(start) = xml.find(needle) else {
        return Vec::new();
    };
    let rest = &xml[start + needle.len()..];
    let section = &rest[..rest.find("</array>").unwrap_or(rest.len())];
    let mut out = Vec::new();
    let mut s = section;
    while let Some(p) = s.find("<string>") {
        let after = &s[p + "<string>".len()..];
        let Some(end) = after.find("</string>") else {
            break;
        };
        let value = after[..end].trim();
        if whole_disk(value).is_some() {
            out.push(value.to_string());
        }
        s = &after[end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_VOLUME: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
    <key>DeviceIdentifier</key>
    <string>disk3s1s1</string>
    <key>ParentWholeDisk</key>
    <string>disk3</string>
    <key>APFSPhysicalStores</key>
    <array>
        <string>disk0s2</string>
    </array>
    <key>Writable</key>
    <false/>
</dict></plist>"#;

    #[test]
    fn reads_identifier_parent_and_physical_stores() {
        let info = parse_info(ROOT_VOLUME);
        assert_eq!(info.identifier.as_deref(), Some("disk3s1s1"));
        assert_eq!(info.parent_whole_disk.as_deref(), Some("disk3"));
        assert_eq!(info.physical_stores, vec!["disk0s2".to_string()]);
    }

    #[test]
    fn accepts_the_dict_form_of_physical_stores() {
        let xml = r#"<dict>
    <key>APFSPhysicalStores</key>
    <array>
        <dict>
            <key>APFSPhysicalStore</key>
            <string>disk4s2</string>
        </dict>
        <dict>
            <key>APFSPhysicalStore</key>
            <string>disk5s2</string>
        </dict>
    </array>
</dict>"#;
        assert_eq!(
            parse_info(xml).physical_stores,
            vec!["disk4s2".to_string(), "disk5s2".to_string()]
        );
    }

    #[test]
    fn a_non_string_value_after_the_key_is_not_read() {
        let xml = "<key>ParentWholeDisk</key>\n<array><string>disk9</string></array>";
        assert_eq!(parse_info(xml).parent_whole_disk, None);
    }

    #[test]
    fn missing_keys_give_empty_info() {
        assert_eq!(parse_info("<dict></dict>"), DiskInfo::default());
    }

    #[test]
    fn whole_disk_of_any_node_name() {
        assert_eq!(whole_disk("disk3s1s1").as_deref(), Some("disk3"));
        assert_eq!(whole_disk("disk12s10").as_deref(), Some("disk12"));
        assert_eq!(whole_disk("disk0").as_deref(), Some("disk0"));
        assert_eq!(whole_disk("disk"), None);
        assert_eq!(whole_disk("diskX"), None);
        assert_eq!(whole_disk("disk3x"), None);
        assert_eq!(whole_disk("/Users/me/out.img"), None);
    }
}
