use std::ops::ControlFlow;

use roaring::{MultiOps, RoaringBitmap};

use crate::{Index, WordCandidate};

use super::RankingRuleImpl;

pub struct Word {
    first_iteration: bool,
}

impl Word {
    pub fn new(words: &mut Vec<WordCandidate>) -> Self {
        // Since the default strategy is to pop the words from
        // the biggest frequency to the lowest we're going to
        // sort all the words by frequency in advance.
        // Later on we'll simply be able to pop the last one.

        // We're also going to cache the key as making the union of all typos is not that fast
        words.sort_by_cached_key(|candidates| candidates.typos.as_slice().union().len());

        Self {
            first_iteration: true,
        }
    }
}

impl RankingRuleImpl for Word {
    fn name(&self) -> &'static str {
        "word"
    }

    fn next(
        &mut self,
        _pred: Option<&dyn RankingRuleImpl>,
        words: &mut Vec<WordCandidate>,
        _index: &Index,
    ) -> ControlFlow<RoaringBitmap, ()> {
        // for the first iteration we returns the intersection of every words
        if self.first_iteration {
            self.first_iteration = false;
            // Nothing to do for the first iteration
            ControlFlow::Continue(())
        } else {
            words.pop();
            if words.is_empty() {
                return ControlFlow::Break(RoaringBitmap::new());
            }
            ControlFlow::Continue(())
        }
    }

    fn current_results(&self, words: &Vec<WordCandidate>) -> RoaringBitmap {
        words
            .iter()
            .map(|word| word.typos.as_slice().union())
            .intersection()
    }
}

#[cfg(test)]
mod test {
    use crate::Index;

    use super::*;

    #[test]
    fn test_words_rr() {
        let index = Index::construct(Vec::new());

        // let's say we're working with "le beau chien"
        let mut words = vec![
            // "le" should be present in a tons of documents and will be first to be evicted
            WordCandidate {
                original: String::from("le"),
                index: 0,
                typos: vec![RoaringBitmap::from_sorted_iter(0..1000).unwrap()],
            },
            // "beau" is present in a bunch of documents but only 4 overlaps with "le"
            WordCandidate {
                original: String::from("beau"),
                index: 1,
                // where I shove my stuff must not matter
                typos: vec![
                    RoaringBitmap::from_sorted_iter(0..2).unwrap(),
                    RoaringBitmap::from_sorted_iter(100..102).unwrap(),
                    RoaringBitmap::from_sorted_iter(1000..1030).unwrap(),
                ],
            },
            WordCandidate {
                original: String::from("chien"),
                index: 2,
                typos: vec![RoaringBitmap::from_sorted_iter(
                    (1..3).chain(98..101).chain(1028..1030),
                )
                .unwrap()],
            },
        ];
        let mut rr = Word::new(&mut words);
        // after calling new, the words should be sorted from the less frequent to the most frequent one:
        let ordering: Vec<_> = words
            .iter()
            .map(|word| (&word.original, word.typos.as_slice().union().len()))
            .collect();
        insta::assert_debug_snapshot!(ordering, @r###"
        [
            (
                "chien",
                7,
            ),
            (
                "beau",
                34,
            ),
            (
                "le",
                1000,
            ),
        ]
        "###);

        let control = rr.next(None, &mut words, &index);
        // the ranking rule should be able to continue
        insta::assert_debug_snapshot!(control, @r###"
        Continue(
            (),
        )
        "###);
        // and the first bucket should only contains the union of everything
        let bucket = rr.current_results(&words);
        insta::assert_debug_snapshot!(bucket, @"RoaringBitmap<[1, 100]>");

        // we should filter our candidates before doing a second call here, but just to be
        // sure it did a whole uninon between the next two words we're going to keep it
        // full. However, that should never happens in prod.
        let control = rr.next(None, &mut words, &index);
        insta::assert_debug_snapshot!(control, @r###"
        Continue(
            (),
        )
        "###);
        // after running the ranking rule a second time we should have dropped the
        // less significant word: "le"
        let second_bucket = rr.current_results(&words);
        assert!(words.iter().all(|word| word.typos[0].len() != 1000));
        // The second bucket should then contains the union between "beau" and "chien"
        insta::assert_debug_snapshot!(second_bucket, @"RoaringBitmap<[1, 100, 1028, 1029]>");

        // this time we're going to do our job and filter the universe before calling next
        Index::cleanup(&bucket, &mut words);
        Index::cleanup(&second_bucket, &mut words);
        let control = rr.next(None, &mut words, &index);
        insta::assert_debug_snapshot!(control, @r###"
        Continue(
            (),
        )
        "###);
        // Then "beau" must be dropped
        // The third and last bucket should then contains only "chien" WITHOUT the previous returned results
        let third_bucket = rr.current_results(&words);
        insta::assert_debug_snapshot!(third_bucket, @"RoaringBitmap<[2, 98, 99]>");

        // Even without proper cleanup, the words ranking rule shouldn't take a look at what is inside the candidates
        // and just drop the last one + return Break([])
        let control = rr.next(None, &mut words, &index);
        insta::assert_debug_snapshot!(control, @r###"
        Break(
            RoaringBitmap<[]>,
        )
        "###);
        // Doing an extraneous call to current_results shouldn't crash either
        let empty = rr.current_results(&words);
        insta::assert_debug_snapshot!(empty, @"RoaringBitmap<[]>");
    }
}
