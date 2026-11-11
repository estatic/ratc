use atc_core::state::TransponderMode;

#[test]
fn transponder_codes_decode_as_before() {
    let modes: Vec<_> = (0..=3).map(TransponderMode::from_code).collect();
    assert_eq!(
        modes,
        [
            Some(TransponderMode::Off),
            Some(TransponderMode::Standby),
            Some(TransponderMode::On),
            Some(TransponderMode::Test),
        ]
    );
    assert_eq!(TransponderMode::from_code(4), None);
}
