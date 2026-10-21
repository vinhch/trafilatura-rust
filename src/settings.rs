//! Defaults ported from upstream `settings.cfg`.

/// Largest input, compressed or not, that decoding accepts, in bytes.
// upstream: settings.cfg::MAX_FILE_SIZE
pub const MAX_FILE_SIZE: usize = 20_000_000;

/// Smallest input worth processing, in bytes.
// upstream: settings.cfg::MIN_FILE_SIZE
pub const MIN_FILE_SIZE: usize = 10;
