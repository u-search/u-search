use roaring::{MultiOps, RoaringBitmap};

use super::RankingRuleImpl;

pub struct Word<'a> {
    words_candidates: Vec<&'a RoaringBitmap>,
    first_iteration: bool,
}

impl<'a> Word<'a> {
    pub fn new(words: &'a [RoaringBitmap]) -> Self {
        let mut words: Vec<_> = words.iter().collect();
        // Since the default strategy is to pop the words from
        // the biggest frequency to the lowest we're going to
        // sort all the words by frequency in advance.
        // Later on we'll simply be able to pop the last one.
        words.sort_unstable_by_key(|word| word.len());

        Self {
            words_candidates: words,
            first_iteration: true,
        }
    }
}

impl<'a> RankingRuleImpl for Word<'a> {
    fn next(&mut self, universe: &RoaringBitmap) -> Option<RoaringBitmap> {
        // for the first iteration we returns the intersection of every words
        if self.first_iteration {
            self.first_iteration = false;
            // cloning here is cheap because we clone a Vec of ref
            Some(self.words_candidates.clone().intersection() & universe)
        } else {
            self.words_candidates.pop()?;
            if self.words_candidates.is_empty() {
                return None;
            }
            Some(self.words_candidates.clone().intersection() & universe)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_words_rr() {
        // let's say we're working with "le beau chien"
        let words = vec![
            // "le" should be present in a tons of documents
            RoaringBitmap::from_sorted_iter(0..1000).unwrap(),
            // "beau" is present in a bunch of documents but only 4 overlaps with "le"
            RoaringBitmap::from_sorted_iter((0..2).chain(100..102).chain(1000..1030)).unwrap(),
            // "chien" is present in 4 documents, and "chienne" in two other documents
            RoaringBitmap::from_sorted_iter((1..3).chain(98..101).chain(1028..1030)).unwrap(),
        ];
        let mut universe = words.as_slice().union();

        let mut rr = Word::new(&words);

        // The first bucket should only contains the union of everything
        let bucket = rr.next(&universe).unwrap();
        insta::assert_debug_snapshot!(bucket, @"RoaringBitmap<[1, 100]>");

        // we should filter our universe before doing a second call here, but just to be
        // sure it did a whole uninon between the next two words we're going to keep it
        // full. However, that should never happens in prod.
        let bucket = rr.next(&universe).unwrap();
        // after running the ranking rule a second time we should have dropped the
        // less significant word: "le"
        assert!(rr
            .words_candidates
            .iter()
            .all(|b| b.len() != words[0].len()));
        // The second bucket should then contains the union between "beau" and "chien"
        insta::assert_debug_snapshot!(bucket, @"RoaringBitmap<[1, 100, 1028, 1029]>");

        // this time we're going to do our job and filter the universe before calling next
        universe -= bucket;
        let bucket = rr.next(&universe).unwrap();
        // Then "beau" must be dropped
        assert!(rr
            .words_candidates
            .iter()
            .all(|b| b.len() != words[1].len()));
        // The third and last bucket should then contains only "chien" WITHOUT the previous returned results
        insta::assert_debug_snapshot!(bucket, @"RoaringBitmap<[2, 98, 99]>");

        assert!(rr.next(&universe).is_none());
    }
}
