//! The date finder upstream takes from the htmldate package.

mod core;
mod extractors;
mod settings;
mod utils;
mod validators;

#[cfg(test)]
mod extractor_cases;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod validator_cases;

pub use self::core::find_date;
