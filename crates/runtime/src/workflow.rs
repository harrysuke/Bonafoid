pub struct WorkflowRuntime {}

impl WorkflowRuntime {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for WorkflowRuntime {
    fn default() -> Self {
        Self::new()
    }
}
