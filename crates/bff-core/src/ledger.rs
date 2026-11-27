//! Token counts as a provider reports them.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodexTokens {
    pub input: i64,
    /// A breakdown of `input`, not an addition to it.
    pub cached_input: i64,
    pub output: i64,
    /// A breakdown of `output`, not an addition to it.
    pub reasoning_output: i64,
    pub total: i64,
}

impl CodexTokens {
    pub fn uncached_input(&self) -> i64 {
        self.input - self.cached_input
    }
}
