use roaring::RoaringBitmap;

pub mod word;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankingRule {
    Word,
    Prefix,
    Typo,
    Exact,
}

pub trait RankingRuleImpl {
    fn next(&mut self, universe: &RoaringBitmap) -> Option<RoaringBitmap>;
}
