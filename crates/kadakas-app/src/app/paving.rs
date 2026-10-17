//! The glue between the paving UI and the app (PAT-06 to PAT-12): what needs a window of the
//! app. The dialogs are in `paving_ui`, the logic in `kadakas_ui_core::paving`.

use super::KadakasApp;
use crate::paving_ui::Request;

impl KadakasApp {
    /// Does what the paving UI asked for.
    pub(super) fn run_paving_requests(&mut self) {
        for r in self.paving_ui.take_requests() {
            match r {
                Request::EditJoints(style) => {
                    self.resource_ui.joints.open(&self.session, style);
                }
            }
        }
    }
}
