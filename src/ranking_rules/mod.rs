use std::ops::ControlFlow;

use roaring::RoaringBitmap;

use crate::WordCandidate;

// pub mod typo;
pub mod word;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankingRule {
    Word,
    Typo,
    Exact,
}

pub trait RankingRuleImpl {
    /// 1. Do your shit with the words candidates
    /// 2. Let me know if I should pass the word candidates to the next ranking rules:
    ///    - ControlFlow::Continue(()) means yes
    ///    - ControlFlow::Break(_) means no and I should insert your results to the bucket sort + call you again
    fn next(&mut self, words: &mut Vec<WordCandidate>) -> ControlFlow<RoaringBitmap, ()>;

    /// Can be called if you returned a `Continue` right before, but there is no ranking rules after you
    /// so we're simply going to insert your results in the bucket sort and call you again.
    fn current_results(&mut self, words: &Vec<WordCandidate>) -> RoaringBitmap;
}
