use super::*;
use crate::app::ModalKind;
use iced::{mouse, Event, Point, Rectangle, Vector};
use iced_test::Simulator;

fn with_ui_test(test: impl FnOnce() + Send + 'static) {
    let _guard = crate::headless_ui_lock();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(test)
        .expect("spawn UI test")
        .join()
        .unwrap_or_else(|error| std::panic::resume_unwind(error));
}

fn warning(app: &OpenCADStudio) -> Element<'_, Message> {
    crate::ui::modal::modal(
        Space::new().width(Fill).height(Fill),
        "Save warning",
        app.modal_content().expect("conversion warning"),
        Message::ConversionLossBack,
        Vector::ZERO,
        crate::ui::modal::ModalOptions::STANDARD,
    )
}

fn visible(ui: &mut Simulator<'_, Message>, label: &str) -> Rectangle {
    let target = ui
        .find(label)
        .unwrap_or_else(|error| panic!("{label}: {error}"));
    let bounds = target.bounds();
    let clipped = target.visible_bounds().expect("control must be visible");
    assert!(
        clipped.width >= bounds.width - 0.1 && clipped.height >= bounds.height - 0.1,
        "{label} must be fully visible: {bounds:?}, clipped to {clipped:?}"
    );
    bounds
}

fn capture(ui: &mut Simulator<'_, Message>, name: &str) {
    if let Some(directory) = std::env::var_os("OCS_SAVE_WARNING_CAPTURES") {
        assert!(ui
            .snapshot(&Theme::Dark)
            .expect("render warning")
            .matches_image(std::path::PathBuf::from(directory).join(format!("{name}.png")))
            .expect("write warning capture"));
    }
}

#[test]
fn conversion_loss_short_list_keeps_compact_dialog_and_existing_actions() {
    with_ui_test(|| {
        let mut app = OpenCADStudio::new_for_test();
        app.active_modal = Some(ModalKind::ConversionLoss);
        app.conversion_losses = vec!["First diagnostic".into()];
        let mut ui =
            Simulator::with_size(iced::Settings::default(), (1024.0, 768.0), warning(&app));
        let first = visible(&mut ui, "• First diagnostic");
        let save = visible(&mut ui, t!("Save anyway").as_ref());
        assert!(save.y - first.y < 100.0, "short list should stay compact");
        capture(&mut ui, "short");
        ui.click(t!("Save anyway").as_ref()).expect("save button");
        ui.click(t!("Back").as_ref()).expect("back button");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages
            .iter()
            .any(|message| matches!(message, Message::ConversionLossProceed)));
        assert!(messages
            .iter()
            .any(|message| matches!(message, Message::ConversionLossBack)));
    });
}

#[test]
fn conversion_loss_long_list_scrolls_without_moving_actions() {
    with_ui_test(|| {
        let mut app = OpenCADStudio::new_for_test();
        app.active_modal = Some(ModalKind::ConversionLoss);
        app.conversion_losses = (1..=90).map(|i| format!("Diagnostic {i}")).collect();
        for (name, size, extra) in [
            ("many", (1024.0, 768.0), Vector::ZERO),
            ("small", (640.0, 480.0), Vector::ZERO),
            ("shrunk", (420.0, 300.0), Vector::new(300.0, 400.0)),
        ] {
            app.modal_resize = extra;
            let mut ui = Simulator::with_size(iced::Settings::default(), size, warning(&app));
            let save = visible(&mut ui, t!("Save anyway").as_ref());
            let back = visible(&mut ui, t!("Back").as_ref());
            let explanation = t!(
                "Saving as \"%{target}\" cannot keep the following, so it will be lost:",
                target = "",
            );
            let header = visible(&mut ui, explanation.as_ref());
            let first = visible(&mut ui, "• Diagnostic 1");
            assert!(ui
                .find("• Diagnostic 90")
                .expect("last diagnostic retained")
                .visible_bounds()
                .is_none());
            capture(&mut ui, &format!("{name}-top"));
            ui.point_at(first.center());
            ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1000.0 },
            })]);
            visible(&mut ui, "• Diagnostic 90");
            assert_eq!(visible(&mut ui, t!("Save anyway").as_ref()), save);
            assert_eq!(visible(&mut ui, t!("Back").as_ref()), back);
            assert_eq!(visible(&mut ui, explanation.as_ref()), header);
            capture(&mut ui, &format!("{name}-bottom"));
            ui.click(t!("Back").as_ref())
                .expect("back remains clickable");
            assert!(ui
                .into_messages()
                .any(|message| matches!(message, Message::ConversionLossBack)));
        }
    });
}

#[test]
fn conversion_loss_wrapped_and_unbroken_messages_remain_readable() {
    with_ui_test(|| {
        let mut app = OpenCADStudio::new_for_test();
        app.active_modal = Some(ModalKind::ConversionLoss);
        app.conversion_losses = vec![
            "A long diagnostic with multiple words that must wrap inside the available dialog width. ".repeat(6),
            "UnbrokenDiagnostic".repeat(50),
            "Final diagnostic".into(),
        ];
        let mut ui = Simulator::with_size(iced::Settings::default(), (420.0, 300.0), warning(&app));
        visible(&mut ui, t!("Save anyway").as_ref());
        visible(&mut ui, t!("Back").as_ref());
        let first = ui
            .find(format!("• {}", app.conversion_losses[0]).as_str())
            .expect("wrapped diagnostic");
        assert!(first.bounds().height > 30.0);
        assert!(first.bounds().x >= 0.0 && first.bounds().x + first.bounds().width <= 420.0);
        let unbroken = ui
            .find(format!("• {}", app.conversion_losses[1]).as_str())
            .expect("unbroken diagnostic");
        assert!(unbroken.bounds().height > 30.0);
        assert!(
            unbroken.bounds().x >= 0.0 && unbroken.bounds().x + unbroken.bounds().width <= 420.0
        );
        ui.point_at(Point::new(
            first.bounds().center_x(),
            first.visible_bounds().expect("first visible").center_y(),
        ));
        ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1000.0 },
        })]);
        visible(&mut ui, "• Final diagnostic");
        capture(&mut ui, "wrapped-bottom");
    });
}
