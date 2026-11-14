//! Carving throughput on a synthetic 64 MiB image with a PNG every 4 MiB.
//!
//! Run with `cargo bench -p tpt-usb-rescue-carver`. This is a plain timing harness, so it adds no
//! dependencies.

use std::time::Instant;
use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, carve};

fn png() -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&crc32fast::hash(&c).to_be_bytes());
    }
    let mut d = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut d, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
    chunk(&mut d, b"IDAT", &[1, 2, 3, 4]);
    chunk(&mut d, b"IEND", &[]);
    d
}

fn main() {
    const SIZE: usize = 64 << 20;
    const EVERY: usize = 4 << 20;
    let sample = png();
    let mut img = Vec::with_capacity(SIZE);
    let mut x: u32 = 1;
    let mut next_sample = 0usize;
    while img.len() < SIZE {
        if img.len() >= next_sample {
            img.extend_from_slice(&sample);
            next_sample += EVERY;
        }
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        img.push((x >> 24) as u8);
    }

    let started = Instant::now();
    let files = carve(&img, &CarveOptions::default(), &BuiltinValidator);
    let secs = started.elapsed().as_secs_f64();
    let mib = img.len() as f64 / (1 << 20) as f64;
    println!(
        "carve: {mib:.0} MiB, {} files in {secs:.3} s ({:.1} MiB/s)",
        files.len(),
        mib / secs
    );
}
