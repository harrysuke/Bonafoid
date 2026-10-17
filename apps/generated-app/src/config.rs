#[derive(Clone)]
pub struct Config {}

impl Config {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}
