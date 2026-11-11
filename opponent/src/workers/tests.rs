use super::*;

impl Staffing {
    /// The worked nodes with the Harvesters each wants.
    pub(crate) fn crews(&self) -> &[(TilePos, usize)] {
        &self.worked
    }
}
