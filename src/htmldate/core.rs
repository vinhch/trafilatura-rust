use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::settings::DateParams;

/// The publication date of the page at `root`, `YYYY-MM-DD`; `None` when it has none.
// upstream: htmldate/core.py::find_date
pub fn find_date(
    _tree: &mut Tree,
    _root: NodeId,
    _params: &DateParams,
    _url: Option<&str>,
    _call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    // todo(phase 4c): htmldate
    Ok(None)
}
