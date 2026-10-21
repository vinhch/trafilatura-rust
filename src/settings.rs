//! Defaults ported from upstream `settings.cfg`.

/// Largest input, compressed or not, that decoding accepts, in bytes.
// upstream: settings.cfg::MAX_FILE_SIZE
pub const MAX_FILE_SIZE: usize = 20_000_000;

/// Smallest input worth processing, in bytes.
// upstream: settings.cfg::MIN_FILE_SIZE
pub const MIN_FILE_SIZE: usize = 10;

/// Element count above which parsing fails with `Rejection::TreeTooLarge`, clones html5ever builds
/// included. Added by this crate, not an upstream constant (deviation D-5).
pub const DEFAULT_MAX_NODES: usize = 1_000_000;
