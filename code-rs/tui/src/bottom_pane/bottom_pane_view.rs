use crate::chatwidget::BackgroundOrderTicket;
use crate::user_approval_widget::ApprovalRequest;
use code_core::protocol::ReviewDecision;
use crossterm::event::KeyEvent;
use std::any::Any;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ChatComposer;
use super::BottomPane;
use super::CancellationEvent;

/// Type to use for a method that may require a redraw of the UI.
pub(crate) enum ConditionalUpdate {
    #[allow(dead_code)]
    NeedsRedraw,
    NoRedraw,
}

/// Trait implemented by every view that can be shown in the bottom pane.
pub(crate) trait BottomPaneView<'a> {
    /// Handle a key event while the view is active. A redraw is always
    /// scheduled after this call.
    fn handle_key_event(&mut self, _pane: &mut BottomPane<'a>, _key_event: KeyEvent) {}

    /// Return `true` if the view has finished and should be removed.
    fn is_complete(&self) -> bool {
        false
    }

    /// Handle Ctrl-C while this view is active.
    fn on_ctrl_c(&mut self, _pane: &mut BottomPane<'a>) -> CancellationEvent {
        CancellationEvent::Ignored
    }

    /// Return the desired height of the view.
    fn desired_height(&self, width: u16) -> u16;

    /// Render the view: this will be displayed in place of the composer.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Render the view when the caller maintains a persistent composer instance.
    /// Default implementation falls back to `render` for views that do not need
    /// direct access to the composer.
    fn render_with_composer(
        &self,
        area: Rect,
        buf: &mut Buffer,
        composer: &ChatComposer,
    ) {
        let _ = composer;
        self.render(area, buf);
    }

    /// Allow read-only downcasting for views that expose additional APIs.
    #[allow(dead_code)]
    fn as_any(&self) -> Option<&dyn Any> {
        None
    }

    /// Update the status indicator text.
    fn update_status_text(&mut self, _text: String) -> ConditionalUpdate {
        ConditionalUpdate::NoRedraw
    }

    /// Allow downcasting for views that expose additional APIs.
    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    /// Called when task completes to check if the view should be hidden.
    fn should_hide_when_task_is_done(&mut self) -> bool {
        false
    }

    /// Try to handle approval request; return the original value if not
    /// consumed.
    fn try_consume_approval_request(
        &mut self,
        request: ApprovalRequest,
        ticket: BackgroundOrderTicket,
    ) -> Option<(ApprovalRequest, BackgroundOrderTicket)> {
        Some((request, ticket))
    }

    /// Resolve a pending approval from a non-keyboard source.
    fn resolve_approval_decision(
        &mut self,
        _approval_id: &str,
        _decision: ReviewDecision,
    ) -> bool {
        false
    }

    /// Handle pasted text while this view is active. Return whether a redraw
    /// is needed. Default: ignore paste.
    fn handle_paste(&mut self, _text: String) -> ConditionalUpdate {
        ConditionalUpdate::NoRedraw
    }

    /// Handle pasted text when a persistent composer is available.
    /// Default implementation forwards to `handle_paste` to preserve
    /// existing behaviour for views that do not interact with the composer.
    fn handle_paste_with_composer(
        &mut self,
        _composer: &mut ChatComposer,
        text: String,
    ) -> ConditionalUpdate {
        self.handle_paste(text)
    }
}
