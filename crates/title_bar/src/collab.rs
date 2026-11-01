use gpui::{AnyElement, IntoElement, Window};
use ui::prelude::*;

use crate::TitleBar;

impl TitleBar {
    pub(crate) fn render_collaborator_list(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        gpui::Empty
    }

    pub(crate) fn render_call_controls(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        Vec::new()
    }
}
