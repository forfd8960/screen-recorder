//! Integration test for `PtsNormalizer` (T019).
//!
//! Scenario: 5 video PTS values with a 2-second gap in the middle.
//! Asserts that all output times are non-negative and correctly offset.

use screen_recorder::encode::sync::PtsNormalizer;

#[test]
fn pts_normalizer_five_values_with_two_second_gap() {
    //          0     1     2               3     4
    // input:  10.0  11.0  12.0            14.5  15.5
    // expected: 0.0   1.0   2.0             4.5   5.5
    let input: [f64; 5] = [10.0, 11.0, 12.0, 14.5, 15.5];
    let expected: [f64; 5] = [0.0, 1.0, 2.0, 4.5, 5.5];

    let mut n = PtsNormalizer::new();
    let output: Vec<f64> = input.iter().map(|&pts| n.normalize_secs(pts)).collect();

    // Values must match expected within floating-point epsilon
    for (i, (got, want)) in output.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - want).abs() < 1e-9,
            "index {i}: got {got:.6}, expected {want:.6}"
        );
    }

    // All values must be non-negative
    for (i, &v) in output.iter().enumerate() {
        assert!(v >= 0.0, "index {i}: value {v} is negative");
    }

    // Values must be monotonically non-decreasing
    for window in output.windows(2) {
        let (prev, next) = (window[0], window[1]);
        assert!(
            prev <= next,
            "not monotonically non-decreasing: {prev:.6} > {next:.6}"
        );
    }
}
