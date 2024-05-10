//! The role of the exact ranking rule is to come back
//! over all the results we're going to return and rank
//! up the one that have 0 or almost no typos on their **original string**.
//! Using the original string is way slower than going through the fst
//! but it also greatly improve the user experience since typing a name
//! with an accent and getting the misspeled version first make you want
//! to kill someone for example.
//! Since it's the last ranking rule, its bucket shouldn't be that big
//! thus it's not a problem to spend a lot of time going through all
//! the IDs of the previous ranking rule.
use std::ops::ControlFlow;

use roaring::RoaringBitmap;
use text_distance::DamerauLevenshtein;

use crate::{Index, WordCandidate};

use super::RankingRuleImpl;

pub struct Exact {
    buckets: Vec<RoaringBitmap>,
}

impl Exact {
    pub fn new() -> Self {
        Self {
            buckets: Vec::new(),
        }
    }
}

impl RankingRuleImpl for Exact {
    fn name(&self) -> &'static str {
        "exact"
    }

    fn next(
        &mut self,
        prev: Option<&dyn RankingRuleImpl>,
        words: &mut Vec<WordCandidate>,
        index: &Index,
    ) -> ControlFlow<RoaringBitmap, ()> {
        // We're the last ranking rule, we should always break

        if self.buckets.is_empty() {
            let current = prev.unwrap().current_results(words);
            let mut words: Vec<&WordCandidate> = words.iter().collect();

            words.sort_by_key(|word| word.index);

            // we won't generate more than 4 buckets
            self.buckets = vec![RoaringBitmap::new(); 4];

            for id in current.iter() {
                let mut distance = 0;

                let mut words = words.iter().peekable();
                for (id, word) in index.documents[id as usize].split_whitespace().enumerate() {
                    match words.peek() {
                        Some(WordCandidate {
                            original, index, ..
                        }) if *index == id => {
                            distance += DamerauLevenshtein {
                                src: original.to_string(),
                                tar: word.to_string(),
                                restricted: true,
                            }
                            .distance();
                        }
                        // we're not looking at the same word
                        Some(_) => continue,
                        None => break,
                    }
                }

                let idx = distance.min(3);
                self.buckets[idx].insert(id as u32);
            }
            self.buckets.retain(|bucket| !bucket.is_empty());
            self.buckets.reverse();
        }

        match self.buckets.pop() {
            Some(bucket) => ControlFlow::Break(bucket),
            // we have nothing to return and the previous ranking rule doesn't either
            None => ControlFlow::Break(RoaringBitmap::new()),
        }
    }

    fn current_results(&self, _words: &Vec<WordCandidate>) -> RoaringBitmap {
        self.buckets.first().cloned().unwrap_or_default()
    }

    fn cleanup(&mut self, used: &RoaringBitmap) {
        for bucket in self.buckets.iter_mut() {
            *bucket -= used;
        }
    }
}
