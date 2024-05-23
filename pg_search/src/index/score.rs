use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// A custom score struct for ordering Tantivy results.
/// For use with the `stable` sorting feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIndexScore {
    pub bm25: f32,
    pub key: String,
}

// We do these custom trait impls, because we want these to be sortable so:
// - they're ordered descending by bm25 score.
// - in case of a tie, they're ordered by ascending key.

impl PartialEq for SearchIndexScore {
    fn eq(&self, other: &Self) -> bool {
        self.bm25 == other.bm25 && self.key == other.key
    }
}

impl PartialOrd for SearchIndexScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.bm25 == other.bm25 {
            other.key.partial_cmp(&self.key)
        } else {
            self.bm25.partial_cmp(&other.bm25)
        }
    }
}
