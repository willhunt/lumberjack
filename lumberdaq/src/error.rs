// Best practise: https://www.youtube.com/watch?v=j-VQCYP7wyw
// Consider this for custom errors: https://crates.io/crates/thiserror

pub type Result<T> = core::result::Result<T, Error>;
pub type Error = Box<dyn std::error::Error>;
