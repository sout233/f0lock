mod render;

use super::{Motion, ValueMotion};

#[test]
fn value_transition_retargets_continuously_and_manual_edits_remain_immediate() {
    let mut motion = ValueMotion {
        binding: 1,
        from: 0.2,
        target: 0.2,
        started: 0.0,
        switching: false,
    };

    assert_eq!(motion.update(2, 0.9, 1.0, false), 0.2);
    let visible = motion.update(2, 0.9, 1.15, false);

    assert!(visible > 0.6 && visible < 0.9);
    assert_eq!(motion.update(1, 0.2, 1.15, false), visible);
    assert!(motion.update(1, 0.2, 1.25, false) < visible);
    assert_eq!(motion.update(1, 0.4, 1.25, true), 0.4);
    assert!(!motion.switching);
    assert_eq!(motion.update(1, 0.8, 1.3, false), 0.8);
}

#[test]
fn motion_has_fast_attack_long_tail_and_continuous_reversal() {
    let mut motion = Motion::default();

    assert_eq!(motion.sample(0.0, true), 0.0);
    let early = motion.sample(0.1, true);
    let middle = motion.sample(0.5, true);

    assert!(early > 0.4 && early < 0.5);
    assert!(middle > 0.97 && middle < 1.0);
    assert!(motion.moving(0.9));
    assert_eq!(motion.sample(0.95, true), 1.0);
    assert!(!motion.moving(0.95));
    assert_eq!(motion.sample(1.0, false), 1.0);
    let release = motion.sample(1.1, false);

    assert_eq!(motion.sample(1.1, true), release);
    assert!(motion.sample(1.2, true) > release);

    let mut at_30 = Motion::default();
    let mut at_120 = Motion::default();
    at_30.sample(0.0, true);
    at_120.sample(0.0, true);

    for i in 1..15 {
        at_30.sample(f64::from(i) / 30.0, true);
    }

    for i in 1..60 {
        at_120.sample(f64::from(i) / 120.0, true);
    }

    assert_eq!(at_30.sample(0.5, true), at_120.sample(0.5, true));
}
