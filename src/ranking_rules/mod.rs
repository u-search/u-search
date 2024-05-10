use std::ops::ControlFlow;

use roaring::RoaringBitmap;

use crate::{Index, WordCandidate};

pub mod exact;
pub mod typo;
pub mod word;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankingRule {
    Word,
    Typo,
    Exact,
}

pub trait RankingRuleImpl {
    /// For debugging/logging purposes
    fn name(&self) -> &'static str;

    /// 1. Do your shit with the words candidates
    /// 2. Let me know if I should pass the word candidates to the next ranking rules:
    ///    - ControlFlow::Continue(()) means yes
    ///    - ControlFlow::Break(_) means no and I should insert your results to the bucket sort + call you again
    fn next(
        &mut self,
        prev: Option<&dyn RankingRuleImpl>,
        words: &mut Vec<WordCandidate>,
        index: &Index,
    ) -> ControlFlow<RoaringBitmap, ()>;

    /// Can be called if you returned a `Continue` right before, but there is no ranking rules after you
    /// so we're simply going to insert your results in the bucket sort and call you again.
    fn current_results(&self, words: &Vec<WordCandidate>) -> RoaringBitmap;

    /// If your ranking rule uses any kind of caches then it should remove the `used` elements from it.
    fn cleanup(&mut self, _used: &RoaringBitmap) {
        ()
    }
}
