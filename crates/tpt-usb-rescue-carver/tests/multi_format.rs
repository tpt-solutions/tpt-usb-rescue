//! Carves a mixed image that holds one minimal, structurally valid sample of each new format.

use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, FileType, carve};

fn bmp() -> Vec<u8> {
    let mut d = vec![0u8; 60];
    d[0..2].copy_from_slice(b"BM");
    d[2..6].copy_from_slice(&60u32.to_le_bytes());
    d[10..14].copy_from_slice(&54u32.to_le_bytes());
    d[14..18].copy_from_slice(&40u32.to_le_bytes());
    d
}

fn wav() -> Vec<u8> {
    let mut d = b"RIFF".to_vec();
    d.extend_from_slice(&32u32.to_le_bytes());
    d.extend_from_slice(b"WAVEfmt ");
    d.extend_from_slice(&[0u8; 16]);
    d.extend_from_slice(b"data");
    d.extend_from_slice(&[0u8; 4]);
    d
}

fn mp4() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&16u32.to_be_bytes());
    d.extend_from_slice(b"ftyp");
    d.extend_from_slice(b"isom\0\0\0\0");
    d.extend_from_slice(&8u32.to_be_bytes());
    d.extend_from_slice(b"moov");
    d
}

fn sevenz() -> Vec<u8> {
    let mut d = vec![b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4, 0, 0, 0, 0];
    d.extend_from_slice(&4u64.to_le_bytes());
    d.extend_from_slice(&6u64.to_le_bytes());
    d.extend_from_slice(&[0u8; 4]);
    d.extend_from_slice(&[0u8; 10]);
    let crc = crc32fast::hash(&d[12..32]);
    d[8..12].copy_from_slice(&crc.to_le_bytes());
    d
}

fn sqlite() -> Vec<u8> {
    let mut d = vec![0u8; 4096 * 2];
    d[..16].copy_from_slice(b"SQLite format 3\0");
    d[16..18].copy_from_slice(&4096u16.to_be_bytes());
    d[18] = 1;
    d[19] = 1;
    d[28..32].copy_from_slice(&2u32.to_be_bytes());
    d
}

fn psd() -> Vec<u8> {
    let mut d = b"8BPS\0\x01".to_vec();
    d.extend_from_slice(&[0u8; 6]);
    d.extend_from_slice(&3u16.to_be_bytes()); // channels
    d.extend_from_slice(&16u32.to_be_bytes()); // height
    d.extend_from_slice(&16u32.to_be_bytes()); // width
    d.extend_from_slice(&8u16.to_be_bytes()); // depth
    d.extend_from_slice(&3u16.to_be_bytes()); // RGB
    d
}

#[test]
fn carves_each_new_format_with_exact_bounds_and_full_score() {
    let samples: Vec<(FileType, Vec<u8>)> = vec![
        (FileType::Bmp, bmp()),
        (FileType::Wav, wav()),
        (FileType::Mp4, mp4()),
        (FileType::SevenZip, sevenz()),
        (FileType::Sqlite, sqlite()),
        (FileType::Psd, psd()),
    ];
    let gap = vec![0u8; 512];
    let mut image = Vec::new();
    let mut expected = Vec::new();
    for (ft, bytes) in &samples {
        image.extend_from_slice(&gap);
        expected.push((*ft, image.len() as u64, bytes.len() as u64));
        image.extend_from_slice(bytes);
    }
    image.extend_from_slice(&gap);

    let files = carve(&image, &CarveOptions::default(), &BuiltinValidator);
    for (ft, start, size) in expected {
        let hit = files
            .iter()
            .find(|f| f.file_type == ft)
            .unwrap_or_else(|| panic!("{ft:?} not carved"));
        assert_eq!(hit.start_offset, start, "{ft:?} start");
        if ft == FileType::Psd {
            // PSD has no end marker the carver can find, so the size is an upper bound.
            assert!(hit.size_bytes >= size, "{ft:?} size");
        } else {
            assert_eq!(hit.size_bytes, size, "{ft:?} size");
        }
        assert_eq!(hit.integrity_score, 1.0, "{ft:?} score");
    }
}

#[test]
fn rejects_look_alike_headers_with_wrong_structure() {
    // Same magic as MP4 but no moov/mdat: must not be reported as a valid file.
    let mut fake = Vec::new();
    fake.extend_from_slice(&[0u8; 64]);
    fake.extend_from_slice(b"\0\0\0\x10ftyp????????????????");
    let files = carve(&fake, &CarveOptions::default(), &BuiltinValidator);
    assert!(files.iter().all(|f| f.file_type != FileType::Mp4));
}
