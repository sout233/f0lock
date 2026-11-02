use crate::{
    band::{Band, BandControl},
    filter::Coefficients,
};

#[test]
fn q_follow_tracks_octaves_in_both_directions_independently_of_point_ratio() {
    for follow in [-2.0_f32, -1.0, 0.0, 1.0, 2.0] {
        for pitch in [110.0_f32, 220.0, 440.0] {
            for ratio in [1.0, 3.0] {
                let mut band = Band::new(48000.0);
                let control = BandControl {
                    exists: true,
                    gain: -12.0,
                    q: 3.0,
                    q_follow: follow,
                    ratio,
                    ..BandControl::default()
                };

                band.process(
                    [0.0; 2], [0.0; 2], control, pitch, true, 48000.0, 0.01, [1.0; 2],
                );

                let expected = 3.0 * (pitch / 220.0).powf(follow);
                assert!((band.q - expected).abs() < 1e-5);
                let coefficients = Coefficients::bell(48000.0, pitch * ratio, expected, -12.0);
                assert!((band.coefficients.k - coefficients.k).abs() < 1e-5);
                assert_eq!(band.values()[2], band.q);
            }
        }
    }
}

#[test]
fn q_follow_smooths_pitch_changes_and_holds_q_during_release() {
    let mut band = Band::new(48000.0);
    let control = BandControl {
        exists: true,
        gain: -6.0,
        q_follow: 1.0,
        ..BandControl::default()
    };
    band.process(
        [0.0; 2], [0.0; 2], control, 220.0, true, 48000.0, 0.1, [1.0; 2],
    );
    band.process(
        [0.0; 2], [0.0; 2], control, 440.0, true, 48000.0, 0.1, [1.0; 2],
    );
    assert!((band.q - 3.3).abs() < 1e-5);

    let held_q = band.q;
    band.process(
        [0.0; 2], [0.0; 2], control, 150.0, false, 48000.0, 0.1, [0.01; 2],
    );
    assert_eq!(band.q, held_q);

    for _ in 0..200 {
        let output = band.process(
            [0.01; 2], [0.01; 2], control, 440.0, true, 48000.0, 0.1, [0.01; 2],
        );
        assert!(output.iter().all(|sample| sample.is_finite()));
    }
    assert!((band.q - 6.0).abs() < 1e-4);
}

#[test]
fn q_follow_uses_the_existing_q_range_at_extreme_pitches() {
    for (pitch, expected) in [(20.0, 0.3), (2000.0, 16.0)] {
        let mut band = Band::new(48000.0);
        let control = BandControl {
            exists: true,
            q_follow: 2.0,
            ..BandControl::default()
        };
        band.process(
            [0.0; 2], [0.0; 2], control, pitch, true, 48000.0, 0.01, [1.0; 2],
        );
        assert_eq!(band.q, expected);
    }
}
