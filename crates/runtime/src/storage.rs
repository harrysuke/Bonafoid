pub struct StorageRuntime {}

impl StorageRuntime {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for StorageRuntime {
    fn default() -> Self {
        Self::new()
    }
}
