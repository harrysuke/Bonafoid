pub struct AuthRuntime {}

impl AuthRuntime {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for AuthRuntime {
    fn default() -> Self {
        Self::new()
    }
}
