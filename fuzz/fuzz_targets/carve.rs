#![no_main]
use libfuzzer_sys::fuzz_target;
use tpt_usb_rescue_carver::{BuiltinValidator, CarveOptions, carve};

fuzz_target!(|data: &[u8]| {
    let opts = CarveOptions { validate: false, min_score: 0.0 };
    for f in carve(data, &opts, &BuiltinValidator) {
        assert!(f.end_offset as usize <= data.len());
    }
});
