//! Headless egui frames for unit tests.
//!
//! A frame's output carries texture uploads (the font atlas) for a renderer
//! to apply; epaint asserts (in debug builds) that they are never dropped
//! unapplied. Tests have no renderer, so these run a frame and clear them,
//! leaving the rest of the output (shapes, viewport commands) to check.

/// One frame whose UI is drawn into the root `Ui`.
pub(crate) fn run_ui(
    ctx: &egui::Context,
    input: egui::RawInput,
    run: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    without_texture_uploads(ctx.run_ui(input, run))
}

/// One frame of code that works on the `Context` (areas, viewport commands).
pub(crate) fn run_pass(
    ctx: &egui::Context,
    input: egui::RawInput,
    run: impl FnMut(&egui::Context),
) -> egui::FullOutput {
    without_texture_uploads(ctx.run_pass(input, run))
}

fn without_texture_uploads(mut output: egui::FullOutput) -> egui::FullOutput {
    output.textures_delta.clear();
    output
}
