use std::ops::ControlFlow;

use roaring::{MultiOps, RoaringBitmap};

use crate::WordCandidate;

use super::RankingRuleImpl;

pub struct Typo {
    first_iteration: bool,
    typo_allowed: usize,
    max_typos: usize,
}

impl Typo {
    pub fn new(words: &[WordCandidate]) -> Self {
        Self {
            first_iteration: true,
            typo_allowed: 0,
            max_typos: words
                .iter()
                .map(|word| word.typos.len())
                .max()
                .unwrap_or_default(),
        }
    }
}

impl RankingRuleImpl for Typo {
    fn name(&self) -> &'static str {
        "typo"
    }

    fn next(
        &mut self,
        _prev: Option<&dyn RankingRuleImpl>,
        _words: &mut Vec<WordCandidate>,
    ) -> ControlFlow<RoaringBitmap, ()> {
        // for the first iteration we returns the intersection of every words
        if self.first_iteration {
            self.first_iteration = false;
            // Nothing to do for the first iteration
            ControlFlow::Continue(())
        } else {
            self.typo_allowed += 1;
            if self.max_typos <= self.typo_allowed {
                // we can reset ourselves, if we're called again it'll be from the previous ranking rule
                self.typo_allowed = 0;
                ControlFlow::Break(RoaringBitmap::new())
            } else {
                ControlFlow::Continue(())
            }
        }
    }

    fn current_results(&self, words: &Vec<WordCandidate>) -> RoaringBitmap {
        words
            .iter()
            .map(|word| word.typos.iter().take(self.typo_allowed).union())
            .intersection()
    }
}
