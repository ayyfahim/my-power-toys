//! Parity check for the ZoomIt screen capture (needs an X display).
//! Set MPT_PARITY_EXPECT="x,y,r,g,b" to assert a known pixel of the root window.
//! Without DISPLAY the test is a no-op.

#[test]
fn x11_capture_matches_known_pixel() {
    if std::env::var_os("DISPLAY").is_none() {
        return;
    }
    let cap = mpt_zoom_it::capture::capture_monitor_under_cursor().expect("capture");
    if let Ok(path) = std::env::var("MPT_PARITY_SAVE") {
        cap.image.save(path).unwrap();
    }
    if let Ok(spec) = std::env::var("MPT_PARITY_EXPECT") {
        let v: Vec<u32> = spec.split(',').map(|s| s.trim().parse().unwrap()).collect();
        let p = cap.image.get_pixel(v[0], v[1]);
        assert_eq!(
            [p[0] as u32, p[1] as u32, p[2] as u32],
            [v[2], v[3], v[4]],
            "captured pixel at ({}, {})",
            v[0],
            v[1]
        );
    }
}
