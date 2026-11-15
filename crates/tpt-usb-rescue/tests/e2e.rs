//! End-to-end tests driving the real binary against synthetic images.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpt-e2e-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_tpt-usb-rescue"))
        .args(args)
        .output()
        .expect("failed to start tpt-usb-rescue")
}

fn os(p: impl AsRef<OsStr>) -> OsString {
    p.as_ref().to_os_string()
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut c = kind.to_vec();
    c.extend_from_slice(body);
    out.extend_from_slice(&crc32fast::hash(&c).to_be_bytes());
}

fn png() -> Vec<u8> {
    let mut d = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    push_chunk(&mut d, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0]);
    push_chunk(&mut d, b"IDAT", &[1, 2, 3, 4, 5, 6, 7, 8]);
    push_chunk(&mut d, b"IEND", &[]);
    d
}

/// Deterministic pseudo-random bytes (LCG) so failures are reproducible.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect()
}

/// 300 KB of noise, a PNG, then 50 KB more noise. The PNG starts at byte 300_000.
fn synthetic_image() -> (Vec<u8>, usize) {
    let mut img = noise(300_000, 1);
    let at = img.len();
    img.extend_from_slice(&png());
    img.extend_from_slice(&noise(50_000, 2));
    (img, at)
}

#[test]
fn image_carve_verify_round_trip() {
    let dir = workdir("roundtrip");
    let (img, png_at) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("recovered.img");
    let map = dir.join("recovered.map");

    let r = run([os("image"), os(&src), os(&out), os("--mapfile"), os(&map)]);
    assert!(
        r.status.success(),
        "image failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert_eq!(
        fs::read(&out).unwrap(),
        img,
        "copy must be byte-identical, with no sector padding"
    );

    let carved = dir.join("carved");
    let r = run([os("carve"), os(&out), os(&carved), os("--validate-media")]);
    assert!(
        r.status.success(),
        "carve failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let index = fs::read_to_string(carved.join("index.csv")).unwrap();
    assert!(index.contains(&format!(",PNG,{png_at},")), "index: {index}");
    assert!(
        index.contains(",1.000,"),
        "PNG should be structurally intact: {index}"
    );

    let r = run([os("verify"), os(&carved), os(&out)]);
    assert!(
        r.status.success(),
        "verify failed: {}",
        String::from_utf8_lossy(&r.stdout)
    );
    assert!(String::from_utf8_lossy(&r.stdout).contains("1 checked, 0 failed"));

    let r = run([os("mapfile"), os(&map), os("--export"), os("ddrescue")]);
    assert!(String::from_utf8_lossy(&r.stdout).contains("+"));
}

#[test]
fn second_image_run_refuses_to_clobber_progress() {
    let dir = workdir("resume-guard");
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");
    let map = dir.join("out.map");

    let first = run([os("image"), os(&src), os(&out), os("--mapfile"), os(&map)]);
    assert!(first.status.success());
    let second = run([os("image"), os(&src), os(&out), os("--mapfile"), os(&map)]);
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("--resume"));

    let resumed = run([
        os("image"),
        os(&src),
        os(&out),
        os("--mapfile"),
        os(&map),
        os("--resume"),
    ]);
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
}

#[test]
fn verify_flags_a_tampered_carved_file() {
    let dir = workdir("tamper");
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");
    let map = dir.join("out.map");
    assert!(
        run([os("image"), os(&src), os(&out), os("--mapfile"), os(&map)])
            .status
            .success()
    );

    let carved = dir.join("carved");
    assert!(run([os("carve"), os(&out), os(&carved)]).status.success());

    // Corrupt the image after carving: the carved copy no longer matches its source range.
    let mut tampered = img.clone();
    tampered[300_010] ^= 0xFF; // inside the 65-byte PNG carved at offset 300_000
    fs::write(&out, &tampered).unwrap();

    let r = run([os("verify"), os(&carved), os(&out)]);
    assert_eq!(
        r.status.code(),
        Some(4),
        "verify mismatch has its own exit code"
    );
    assert!(String::from_utf8_lossy(&r.stdout).contains("MISMATCH"));
}

#[test]
fn analyze_reports_no_volumes_on_plain_noise() {
    let dir = workdir("analyze");
    let src = dir.join("noise.img");
    fs::write(&src, noise(64 * 1024, 9)).unwrap();
    let r = run([os("analyze"), os(&src)]);
    assert!(r.status.success());
    let text = String::from_utf8_lossy(&r.stdout).to_string();
    assert!(text.contains("volumes: none detected"), "{text}");
}

#[test]
fn resume_refuses_a_truncated_or_missing_image() {
    let dir = workdir("resume-truncated");
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");
    let map = dir.join("out.map");
    assert!(
        run([os("image"), os(&src), os(&out), os("--mapfile"), os(&map)])
            .status
            .success()
    );

    let resume = || {
        run([
            os("image"),
            os(&src),
            os(&out),
            os("--mapfile"),
            os(&map),
            os("--resume"),
        ])
    };

    // Truncated image: the map says everything is recovered, so resuming would hide zeros.
    fs::OpenOptions::new()
        .write(true)
        .open(&out)
        .unwrap()
        .set_len(1000)
        .unwrap();
    let r = resume();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("cannot resume"));
    assert_eq!(
        fs::metadata(&out).unwrap().len(),
        1000,
        "image must be left alone"
    );

    // Missing image.
    fs::remove_file(&out).unwrap();
    let r = resume();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("cannot resume"));
    assert!(!out.exists());
}

#[test]
fn mapfile_export_never_overwrites() {
    let dir = workdir("export-overwrite");
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");
    let map = dir.join("out.map");
    assert!(
        run([os("image"), os(&src), os(&out), os("--mapfile"), os(&map)])
            .status
            .success()
    );
    let dest = dir.join("map.toml");
    let export = |d: &PathBuf| {
        run([
            os("mapfile"),
            os(&map),
            os("--export"),
            os("toml"),
            os("--out"),
            os(d),
        ])
    };
    assert!(export(&dest).status.success());
    let second = export(&dest);
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already exists"));
    // Pointing --out at the mapfile itself must not destroy it either.
    assert!(!export(&map).status.success());
    assert!(fs::metadata(&map).unwrap().len() > 24);
}

#[test]
fn mapfile_defaults_to_the_output_name_plus_map() {
    let dir = workdir("default-map");
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");
    let r = run([os("image"), os(&src), os(&out), os("--quiet")]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(dir.join("out.img.map").exists());
    assert_eq!(fs::read(&out).unwrap(), img);
    // `--quiet` keeps stderr free of progress lines.
    assert!(
        r.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );

    // The default map is found again, so a repeat run is refused and --resume works.
    let again = run([os("image"), os(&src), os(&out)]);
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("--resume"));
}

#[test]
fn usage_errors_exit_with_two() {
    let r = run([os("image")]);
    assert_eq!(r.status.code(), Some(2));
}

#[test]
fn hash_sidecar_is_written_and_checked_by_verify() {
    let dir = workdir("hash-sidecar");
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");
    let r = run([os("image"), os(&src), os(&out), os("--hash"), os("-q")]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let sidecar = fs::read_to_string(dir.join("out.img.sha256")).unwrap();
    assert!(sidecar.trim_end().ends_with("  out.img"), "{sidecar}");
    assert_eq!(sidecar.split_whitespace().next().unwrap().len(), 64);

    let carved = dir.join("carved");
    assert!(run([os("carve"), os(&out), os(&carved)]).status.success());
    let ok = run([os("verify"), os(&carved), os(&out)]);
    assert_eq!(ok.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&ok.stdout).contains("matches its recorded hash"));

    // Change a byte outside every carved file: only the recorded hash can notice.
    let mut changed = img.clone();
    changed[10] ^= 0xFF;
    fs::write(&out, &changed).unwrap();
    let bad = run([os("verify"), os(&carved), os(&out)]);
    assert_eq!(bad.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&bad.stdout).contains("IMAGE CHANGED"));
}

/// Makes a source image, images it, and carves it. Returns (work dir, image, carved dir).
fn imaged_and_carved(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = workdir(name);
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");
    assert!(
        run([os("image"), os(&src), os(&out), os("-q")])
            .status
            .success()
    );
    let carved = dir.join("carved");
    assert!(run([os("carve"), os(&out), os(&carved)]).status.success());
    (dir, out, carved)
}

#[test]
fn json_output_is_one_object_on_stdout_and_text_goes_to_stderr() {
    let dir = workdir("json-image");
    let (img, _) = synthetic_image();
    let src = dir.join("source.img");
    fs::write(&src, &img).unwrap();
    let out = dir.join("out.img");

    let r = run([os("--json"), os("image"), os(&src), os(&out), os("-q")]);
    assert_eq!(r.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&r.stdout).to_string();
    assert_eq!(
        stdout.lines().count(),
        1,
        "stdout must be a single JSON line: {stdout}"
    );
    assert!(
        stdout.starts_with(r#"{"ok":true,"command":"image""#),
        "{stdout}"
    );
    assert!(stdout.contains(&format!(
        r#""recovered_bytes":{}"#,
        img.len().div_ceil(512) * 512
    )));
    assert!(stdout.contains(r#""unrecovered_bytes":0"#));
    assert!(String::from_utf8_lossy(&r.stderr).contains("pass 1"));

    let m = run([os("--json"), os("mapfile"), os(dir.join("out.img.map"))]);
    let text = String::from_utf8_lossy(&m.stdout).to_string();
    assert!(
        text.contains(r#""command":"mapfile""#) && text.contains(r#""bad_runs":[]"#),
        "{text}"
    );
}

#[test]
fn json_verify_reports_each_file_and_the_failure_exit_code() {
    let (_dir, img, carved) = imaged_and_carved("json-verify");
    let ok = run([os("--json"), os("verify"), os(&carved), os(&img)]);
    assert_eq!(ok.status.code(), Some(0));
    let text = String::from_utf8_lossy(&ok.stdout).to_string();
    assert!(
        text.contains(r#""ok":true"#) && text.contains(r#""result":"ok""#),
        "{text}"
    );

    // Alter a carved file: it no longer matches the digest in the index or the image.
    let victim = fs::read_dir(&carved)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "png"))
        .unwrap();
    let mut bytes = fs::read(&victim).unwrap();
    bytes[10] ^= 0xFF;
    fs::write(&victim, bytes).unwrap();

    let bad = run([os("--json"), os("verify"), os(&carved), os(&img)]);
    assert_eq!(bad.status.code(), Some(4));
    let text = String::from_utf8_lossy(&bad.stdout).to_string();
    assert!(
        text.contains(r#""ok":false"#) && text.contains(r#""result":"mismatch""#),
        "{text}"
    );
    assert!(text.contains(r#""exit_code":4"#));
}

#[test]
fn json_errors_and_unsupported_commands() {
    let dir = workdir("json-errors");
    let r = run([
        os("--json"),
        os("verify"),
        os(dir.join("nope")),
        os(dir.join("nope.img")),
    ]);
    assert_eq!(r.status.code(), Some(1));
    let text = String::from_utf8_lossy(&r.stdout).to_string();
    assert!(text.starts_with(r#"{"ok":false,"error":""#), "{text}");
    assert!(text.contains(r#""interrupted":false"#));

    let a = run([
        os("--json"),
        os("mapfile"),
        os(dir.join("x.map")),
        os("--export"),
        os("ddrescue"),
        os("--out"),
        os(dir.join("x.ddr")),
    ]);
    assert_eq!(a.status.code(), Some(2));
    assert!(a.stdout.is_empty());
    assert!(String::from_utf8_lossy(&a.stderr).contains("not supported"));
}

#[test]
fn carve_index_records_a_sha256_per_file() {
    let (_dir, _img, carved) = imaged_and_carved("index-digest");
    let index = fs::read_to_string(carved.join("index.csv")).unwrap();
    let mut lines = index.lines();
    assert!(lines.next().unwrap().ends_with(",file,sha256"));
    let mut rows = 0;
    for line in lines {
        let cols: Vec<&str> = line.split(',').collect();
        let digest = cols[8];
        assert_eq!(digest.len(), 64, "{line}");
        assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
        rows += 1;
    }
    assert!(rows > 0);
}

#[test]
fn completions_are_generated_for_each_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let r = run([os("completions"), os(shell)]);
        assert_eq!(r.status.code(), Some(0), "{shell}");
        let text = String::from_utf8_lossy(&r.stdout).to_string();
        assert!(text.contains("list-devices"), "{shell}: {text}");
    }
    assert_eq!(run([os("completions"), os("nope")]).status.code(), Some(2));
}

#[test]
fn man_pages_are_written_one_per_command() {
    let stdout = run([os("man")]);
    assert_eq!(stdout.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&stdout.stdout).contains(".TH tpt-usb-rescue 1"));

    let dir = workdir("man").join("pages");
    let r = run([os("man"), os("--out-dir"), os(&dir)]);
    assert_eq!(r.status.code(), Some(0));
    for page in [
        "tpt-usb-rescue.1",
        "tpt-usb-rescue-image.1",
        "tpt-usb-rescue-verify.1",
    ] {
        assert!(dir.join(page).exists(), "{page}");
    }
    assert!(!dir.join("tpt-usb-rescue-help.1").exists());
    let image = fs::read_to_string(dir.join("tpt-usb-rescue-image.1")).unwrap();
    assert!(
        image.contains("chunk") && image.contains("timeout"),
        "options belong on the page"
    );
}

#[test]
fn practice_filesystem_image_extracts_its_files() {
    // examples/practice-fat32.img is shipped for people to try `analyze --extract` on.
    let img = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/practice-fat32.img");
    let dir = workdir("practice-fat32");
    let r = run([
        os("analyze"),
        os(&img),
        os("--extract"),
        os(dir.join("vol")),
    ]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let text = String::from_utf8_lossy(&r.stdout).to_string();
    assert!(
        text.contains("FAT32 at byte offset 0") && text.contains("1 deleted"),
        "{text}"
    );

    let vol = dir.join("vol").join("volume_0");
    assert_eq!(fs::read(vol.join("HELLO.TXT")).unwrap(), b"hello world");
    assert_eq!(fs::read(vol.join("LongName.txt")).unwrap(), b"long");
    assert_eq!(
        fs::read(vol.join("DOCS").join("README.TXT")).unwrap(),
        b"data"
    );
    // A deleted entry loses its first character on a FAT volume, and is marked by the prefix.
    assert_eq!(fs::read(vol.join("DELETED__ECRET.DAT")).unwrap().len(), 300);
}

/// Copies a practice image into a fresh work folder and returns its path.
fn practice_copy(name: &str, image: &str) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(image);
    let dir = workdir(name);
    let dst = dir.join(image);
    fs::copy(&src, &dst).unwrap();
    dst
}

#[test]
fn practice_exfat_image_extracts_its_files() {
    let img = practice_copy("practice-exfat", "practice-exfat.img");
    let dir = img.parent().unwrap().to_path_buf();
    let r = run([
        os("analyze"),
        os(&img),
        os("--extract"),
        os(dir.join("vol")),
    ]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let text = String::from_utf8_lossy(&r.stdout).to_string();
    assert!(
        text.contains("exFAT at byte offset 0") && text.contains("1 deleted"),
        "{text}"
    );

    let vol = dir.join("vol").join("volume_0");
    assert_eq!(fs::read(vol.join("big").join("deep.txt")).unwrap(), b"deep");
    assert_eq!(fs::read(vol.join("photos").join("a.jpg")).unwrap(), b"JPG");
    // note.txt spans two clusters: 600 bytes, starting with 'n' and ending with 'm'.
    let note = fs::read(vol.join("note.txt")).unwrap();
    assert_eq!((note.len(), note[0], note[599]), (600, b'n', b'm'));
    // The deleted file keeps its size; its name is marked with the DELETED_ prefix.
    assert_eq!(fs::read(vol.join("DELETED_gone.bin")).unwrap().len(), 200);
}

#[test]
fn practice_ntfs_image_extracts_its_files() {
    let img = practice_copy("practice-ntfs", "practice-ntfs.img");
    let dir = img.parent().unwrap().to_path_buf();
    let r = run([
        os("analyze"),
        os(&img),
        os("--extract"),
        os(dir.join("vol")),
    ]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let text = String::from_utf8_lossy(&r.stdout).to_string();
    assert!(
        text.contains("NTFS at byte offset 0") && text.contains("1 deleted"),
        "{text}"
    );

    let vol = dir.join("vol").join("volume_0");
    assert_eq!(fs::read(vol.join("HELLO.TXT")).unwrap(), b"hello ntfs");
    assert_eq!(
        fs::read(vol.join("DOCS").join("NOTE.TXT")).unwrap(),
        vec![b'n'; 600]
    );
    assert_eq!(fs::read(vol.join("DELETED_OLD.BIN")).unwrap(), b"old");
}

#[test]
fn analyze_json_is_one_object_and_text_goes_to_stderr() {
    let img = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/practice-fat32.img");
    let r = run([os("--json"), os("analyze"), os(&img)]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let json = String::from_utf8_lossy(&r.stdout).to_string();
    assert_eq!(
        json.trim().lines().count(),
        1,
        "stdout must be one line: {json}"
    );
    assert!(
        json.starts_with(r#"{"ok":true,"command":"analyze""#),
        "{json}"
    );
    assert!(json.contains(r#""kind":"FAT32""#), "{json}");
    assert!(json.contains(r#""path":"/HELLO.TXT""#), "{json}");
    assert!(
        json.contains(r#""deleted":true"#),
        "the practice image has a deleted file: {json}"
    );
    assert!(json.contains(r#""error":null"#), "{json}");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert!(
        err.contains("FAT32 at byte offset 0"),
        "text belongs on stderr: {err}"
    );
}

#[test]
fn hostile_long_name_cannot_write_outside_the_output_folder() {
    // Rename LongName.txt inside a copy of the practice image to `..\..\xx.txt`. The name has the
    // same length and the short name is untouched, so the long-name checksum still matches and the
    // entry is still read.
    let img = practice_copy("hostile-name", "practice-fat32.img");
    let dir = img.parent().unwrap().to_path_buf();
    let mut bytes = fs::read(&img).unwrap();

    // A long-name entry keeps its 12 characters in three fields (5, 6 and 1 of them), with the
    // attribute bytes in between. The first piece starts with "Long" at byte 1 of its entry.
    let long: Vec<u16> = "LongName.txt".encode_utf16().collect();
    let long4: Vec<u8> = "Long".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let entry = bytes
        .windows(long4.len())
        .position(|w| w == long4.as_slice())
        .expect("practice image holds a long name")
        - 1;
    let slots: Vec<usize> = (0..5)
        .map(|i| entry + 1 + 2 * i)
        .chain((0..6).map(|i| entry + 14 + 2 * i))
        .chain(std::iter::once(entry + 28))
        .collect();
    let read_name = |bytes: &[u8]| -> Vec<u16> {
        slots
            .iter()
            .map(|&o| u16::from_le_bytes([bytes[o], bytes[o + 1]]))
            .collect()
    };
    assert_eq!(read_name(&bytes), long, "the entry is LongName.txt");

    let hostile: Vec<u16> = "..\\..\\xx.txt".encode_utf16().collect();
    assert_eq!(hostile.len(), long.len());
    for (&o, c) in slots.iter().zip(&hostile) {
        bytes[o..o + 2].copy_from_slice(&c.to_le_bytes());
    }
    fs::write(&img, &bytes).unwrap();

    let r = run([
        os("analyze"),
        os(&img),
        os("--extract"),
        os(dir.join("vol")),
    ]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let vol = dir.join("vol").join("volume_0");
    assert_eq!(fs::read(vol.join("xx.txt")).unwrap(), b"long");
    assert!(
        !dir.join("xx.txt").exists(),
        "the file must not escape the output folder"
    );
    assert!(
        !dir.join("vol").join("xx.txt").exists(),
        "nor sit outside the volume folder"
    );
}
