#![no_main]
use libfuzzer_sys::fuzz_target;
use tpt_usb_rescue_fs::fat32::Fat32;
use tpt_usb_rescue_fs::Filesystem;

fuzz_target!(|data: &[u8]| {
    if let Ok(fs) = Fat32::open(data, 0) {
        for e in fs.entries() {
            let _ = fs.read(&e);
        }
    }
});
