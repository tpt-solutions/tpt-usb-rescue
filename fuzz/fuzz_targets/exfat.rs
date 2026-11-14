#![no_main]
use libfuzzer_sys::fuzz_target;
use tpt_usb_rescue_fs::Filesystem;
use tpt_usb_rescue_fs::exfat::ExFat;

fuzz_target!(|data: &[u8]| {
    if let Ok(fs) = ExFat::open(data, 0) {
        for e in fs.entries() {
            let _ = fs.read(&e);
        }
    }
});
