use crate::App;

impl App {
    /// The active global arrangement tool, for the M7b E0 walkthrough.
    pub fn arrangement_tool_name(&self) -> &'static str {
        self.active_arrangement_tool_name()
    }
}
