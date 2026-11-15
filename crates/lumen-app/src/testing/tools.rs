use crate::App;

impl App {
    pub fn arrangement_tool_name(&self) -> &'static str {
        self.active_arrangement_tool_name()
    }
}
