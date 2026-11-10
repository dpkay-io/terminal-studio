use egui::{Color32, Context, FullOutput, Pos2, RawInput, Rect, Shape, Ui};

const TEST_WIDTH: f32 = 800.0;
const TEST_HEIGHT: f32 = 600.0;

pub fn ctx_with_fonts() -> Context {
    let ctx = Context::default();
    crate::fonts::install_fonts(&ctx);
    let _ = run_ui(&ctx, RawInput::default(), |_| {});
    ctx
}

pub fn run_ui(ctx: &Context, mut input: RawInput, mut f: impl FnMut(&mut Ui)) -> FullOutput {
    input.screen_rect = Some(Rect::from_min_size(
        Pos2::ZERO,
        egui::vec2(TEST_WIDTH, TEST_HEIGHT),
    ));
    ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| f(ui));
    })
}

pub fn click_at(ctx: &Context, pos: Pos2, mut f: impl FnMut(&mut Ui)) -> FullOutput {
    let mut moved = RawInput::default();
    moved.events.push(egui::Event::PointerMoved(pos));
    let _ = run_ui(ctx, moved, &mut f);

    let mut pressed = RawInput::default();
    pressed.events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    let _ = run_ui(ctx, pressed, &mut f);

    let mut released = RawInput::default();
    released.events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    run_ui(ctx, released, f)
}

pub fn filled_rects(out: &FullOutput) -> Vec<(Rect, Color32)> {
    fn collect(shape: &Shape, rects: &mut Vec<(Rect, Color32)>) {
        match shape {
            Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => {
                rects.push((rect.rect, rect.fill));
            }
            Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, rects);
                }
            }
            _ => {}
        }
    }

    let mut rects = Vec::new();
    for clipped in &out.shapes {
        collect(&clipped.shape, &mut rects);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_click_registers() {
        let ctx = ctx_with_fonts();
        let mut clicked = false;
        let _ = click_at(&ctx, egui::pos2(30.0, 18.0), |ui| {
            if ui.button("Click").clicked() {
                clicked = true;
            }
        });
        assert!(clicked);
    }
}
