//! Document-level settings.

use crate::model::dimension::{Dimension, Twips};

use super::identifiers::RevisionSaveId;

#[derive(Clone, Debug)]
pub struct DocumentSettings {
    /// Default tab stop interval (OOXML default: 720 twips = 0.5 inch).
    pub default_tab_stop: Dimension<Twips>,
    /// Whether even/odd headers/footers are enabled.
    pub even_and_odd_headers: bool,
    /// The rsid of the original editing session that created this document.
    pub rsid_root: Option<RevisionSaveId>,
    /// All revision save IDs recorded in this document's history.
    pub rsids: Vec<RevisionSaveId>,
}

impl Default for DocumentSettings {
    fn default() -> Self {
        Self {
            // §17.15.1.25: when `w:defaultTabStop` is omitted the default tab
            // stop interval is 720 twips (0.5"). A derived `Default` would give
            // 0 here — wrong per spec, and it would silently collapse default
            // tabs for any consumer that reads this field.
            default_tab_stop: Dimension::new(720),
            even_and_odd_headers: false,
            rsid_root: None,
            rsids: Vec::new(),
        }
    }
}
