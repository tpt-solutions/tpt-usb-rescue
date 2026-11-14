#![no_main]
use libfuzzer_sys::fuzz_target;
use tpt_usb_rescue_fs::Filesystem;
use tpt_usb_rescue_fs::ntfs::Ntfs;

fuzz_target!(|data: &[u8]| {
    if let Ok(fs) = Ntfs::open(data, 0) {
        for e in fs.entries() {
            let _ = fs.read(&e);
        }
    }
});
