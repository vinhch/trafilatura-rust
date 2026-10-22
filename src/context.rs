//! The per-call context every ported function takes where upstream passes `options`.

use std::borrow::Cow;

use crate::cancel::Call;
use crate::settings::Extractor;

/// The options of one extraction and the `Call` that can stop it. Where upstream copies and edits
/// its options (`r_options = copy(options)`), the port clones the `Extractor` into `Cow::Owned` and
/// builds a new `Ctx` around the same `Call`.
#[derive(Clone, Debug)]
pub(crate) struct Ctx<'a> {
    /// The options of this call.
    pub options: Cow<'a, Extractor>,
    /// Cancellation and deadline of this call.
    pub call: &'a Call<'a>,
}
