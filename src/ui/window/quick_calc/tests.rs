use super::*;

fn ctx(p: &QuickCalcPalette) -> CalcContext {
    p.context(1.0, None)
}

fn submit(p: &mut QuickCalcPalette, input: &str) -> Option<Submitted> {
    p.input = input.into();
    let c = ctx(p);
    p.submit(&c)
}

#[test]
fn enter_history_and_errors() {
    let mut p = QuickCalcPalette::default();
    submit(&mut p, "$wall = 300").unwrap();
    assert_eq!(
        submit(&mut p, "2*$wall + 3*90").unwrap().value,
        Value::Number(870.0)
    );
    // A leading operator continues from ans, and the history says so.
    submit(&mut p, "/ 2").unwrap();
    assert_eq!(p.history[2].expr, "ans / 2");
    assert_eq!(p.ans, Some(Value::Number(435.0)));
    assert!(p.input.is_empty());
    // An error shows only after Enter, and only until the input changes.
    p.input = "2 +".into();
    assert!(!p.error_shown());
    assert!(submit(&mut p, "2 +").is_none());
    assert!(p.error_shown());
    assert_eq!(p.history.len(), 3);
    p.append(" 3");
    assert!(!p.error_shown());
    // ↑ / ↓ walk the history and come back to an empty input.
    p.history_step(true);
    assert_eq!(p.input, "ans / 2");
    p.history_step(true);
    assert_eq!(p.input, "2*$wall + 3*90");
    p.history_step(false);
    p.history_step(false);
    assert_eq!(p.input, "");
}

#[test]
fn unit_converter() {
    let mut p = QuickCalcPalette {
        unit_value: "3600".into(),
        ..Default::default()
    };
    assert_eq!(p.converted_literal().as_deref(), Some("3.6m"));
    // Inserted, it reads back in the drawing unit.
    p.input = p.converted_literal().unwrap();
    assert_eq!(p.preview(&ctx(&p)), Preview::Ok(Value::Number(3600.0)));
    p.set_unit_cat(UnitCat::Area);
    p.unit_value = "2500000".into();
    assert_eq!(p.converted_literal().as_deref(), Some("2.5"));
    p.set_unit_cat(UnitCat::Angle);
    p.unit_value = "180".into();
    assert!((p.converted().unwrap() - std::f64::consts::PI).abs() < 1e-12);
    p.unit_value = "abc".into();
    assert_eq!(p.converted(), None);
}

#[test]
fn toolbar_switches_close_flyouts() {
    let mut p = QuickCalcPalette::default();
    p.set_keypad_mode(KeypadMode::Scientific);
    p.flyout = Some(Flyout::Trigonometry);
    p.set_keypad_mode(KeypadMode::Standard);
    assert_eq!(p.flyout, None);
    p.toggle_angle_mode();
    assert_eq!(p.mode, AngleMode::Radians);
    p.toggle_angle_mode();
    assert_eq!(p.mode, AngleMode::Degrees);
}
