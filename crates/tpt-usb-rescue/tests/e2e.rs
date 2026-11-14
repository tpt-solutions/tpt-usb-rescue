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
    assert!(!r.status.success());
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
