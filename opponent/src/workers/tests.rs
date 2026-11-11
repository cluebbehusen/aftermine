use super::*;

impl Staffing {
    /// The worked nodes with the Harvesters each wants.
    pub(crate) fn crews(&self) -> &[(TilePos, usize)] {
        &self.worked
    }
}

impl Routes<'_> {
    /// Where a route steps from `tile`, as from a source or past one: as
    /// [`step`](Self::step) takes it, and as ranking every closer tile of a
    /// footprint's ring took it.
    pub(crate) fn steps_from(&self, tile: TilePos, source: bool) -> [Option<(u16, TilePos)>; 2] {
        let distance = self.distances.get(tile).copied().unwrap_or(UNREACHABLE);
        let open = |tile: TilePos| self.map.component(tile).is_some();
        let legal = |next: &TilePos| {
            let (dx, dy) = (next.x - tile.x, next.y - tile.y);
            source || dx == 0 || dy == 0 || (open(tile.offset(dx, 0)) && open(tile.offset(0, dy)))
        };
        let from = doubled(tile);
        let ranked = ring(tile, (1, 1))
            .filter(legal)
            .map(|next| {
                (
                    self.distances.get(next).copied().unwrap_or(UNREACHABLE),
                    next,
                )
            })
            .filter(|(closer, _)| *closer < distance)
            .min_by_key(|(closer, next)| (*closer, self.frame.rank(from, doubled(*next))));
        [self.step(tile, distance, source), ranked]
    }
}
