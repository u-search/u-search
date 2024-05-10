mod ranking_rules;

use std::ops::ControlFlow;

use fst::{IntoStreamer, Map, MapBuilder, Streamer};
use ranking_rules::{typo::Typo, word::Word, RankingRule, RankingRuleImpl};
use roaring::RoaringBitmap;
use text_distance::DamerauLevenshtein;

pub struct Index {
    documents: Vec<String>,
    fst: Map<Vec<u8>>,
    bitmaps: Vec<RoaringBitmap>,
}

type Id = u32;

impl Index {
    pub fn construct(documents: Vec<String>) -> Self {
        let mut words = documents
            .iter()
            .enumerate()
            .flat_map(|(id, document)| {
                document
                    .split_whitespace()
                    .map(move |word| (id as Id, normalize(word)))
            })
            .collect::<Vec<(Id, String)>>();
        words.sort_unstable_by(|(_, left), (_, right)| left.cmp(right));

        let mut build = MapBuilder::memory();

        let mut last_word = None;
        let mut bitmaps = Vec::new();

        for (id, word) in words.iter() {
            if Some(word) != last_word {
                bitmaps.push(RoaringBitmap::from_sorted_iter(Some(*id)).unwrap());
                build.insert(word, (bitmaps.len() - 1) as u64).unwrap();
            } else {
                bitmaps.last_mut().unwrap().insert(*id);
            }

            last_word = Some(word);
        }

        Index {
            documents,
            fst: build.into_map(),
            bitmaps,
        }
    }

    pub fn search<'a>(&'a self, search: &Search) -> Vec<&'a str> {
        // TODO: returns random results maybe?
        if search.words.len() == 0 {
            return Vec::new();
        }

        // contains all the buckets
        let mut res: Vec<RoaringBitmap> = Vec::new();

        let mut candidates = self.get_candidates(&search);
        // let universe = candidates.as_slice().union();

        let mut ranking_rules: Vec<Box<dyn RankingRuleImpl>> = search
            .ranking_rules
            .iter()
            .map(|ranking_rule| match ranking_rule {
                RankingRule::Word => {
                    Box::new(Word::new(&mut candidates)) as Box<dyn RankingRuleImpl>
                }
                RankingRule::Typo => Box::new(Typo::new(&candidates)) as Box<dyn RankingRuleImpl>,
                _ => panic!(),
                // RankingRule::Prefix => Box::new(todo!()) as Box<dyn RankingRuleImpl>,
                // RankingRule::Exact => Box::new(todo!()) as Box<dyn RankingRuleImpl>,
            })
            .collect();
        let ranking_rules_len = ranking_rules.len();

        let mut current_ranking_rule = 0;

        macro_rules! next {
            () => {
                {
                // we cannot borrow twice the list of ranking rules thus we'll cheat a little
                let current = &mut ranking_rules[current_ranking_rule];
                // we detach the lifetime from the vec, this allow us to borrow the previous element safely
                let current: &'static mut Box<dyn RankingRuleImpl> = unsafe { std::mem::transmute(current) };
                current.next(
                    ranking_rules.get(current_ranking_rule - 1).map(|rr| &**rr),
                    &mut candidates,
                )
                }
            };
        }

        while res.iter().map(|bucket| bucket.len()).sum::<u64>() < search.limit as u64 {
            let next = next!();
            let ranking_rule = &mut ranking_rules[current_ranking_rule];

            match next {
                // We want to advance
                ControlFlow::Continue(()) => {
                    if current_ranking_rule == ranking_rules_len - 1 {
                        // there is no ranking rule to continue, get the bucket of the current one and call it again
                        // println!("there is no ranking rule to continue, get the bucket of the current one ({}) and call it again", ranking_rule.name());
                        let bucket = ranking_rule.current_results(&candidates);
                        Self::cleanup(&bucket, &mut candidates);
                        res.push(bucket);
                    } else {
                        // we advance and do nothing
                        current_ranking_rule += 1;
                        // println!(
                        //     "advance from {} to {}",
                        //     ranking_rules[current_ranking_rule - 1].name(),
                        //     ranking_rules[current_ranking_rule].name()
                        // );
                    }
                }
                // We want to get back one ranking rule behind
                ControlFlow::Break(bucket) if bucket.is_empty() => {
                    // if we're at the first ranking rule and there is nothing left to sort, exit
                    if current_ranking_rule == 0 {
                        break;
                    }
                    current_ranking_rule -= 1;
                    res.push(bucket);
                }
                // We want to push that bucket and continue our life with the next ranking rule if there is one
                ControlFlow::Break(bucket) => {
                    Self::cleanup(&bucket, &mut candidates);
                    res.push(bucket);
                }
            }
        }

        res.iter()
            .flat_map(|bitmap| {
                bitmap
                    .iter()
                    .map(|idx| self.documents[idx as usize].as_ref())
            })
            .take(search.limit)
            .collect()
    }

    fn cleanup(used: &RoaringBitmap, candidates: &mut [WordCandidate]) {
        for candidate in candidates.iter_mut() {
            for typo in candidate.typos.iter_mut() {
                *typo -= used;
            }
        }
    }

    fn get_candidates(&self, search: &Search) -> Vec<WordCandidate> {
        let mut ret = Vec::with_capacity(search.words.len());

        for word in search.words.iter() {
            // enable 1 typo every 3 letters maxed at 3 typos
            let typo = (word.len() / 3).min(3);
            let lev = fst::automaton::Levenshtein::new(word, typo as u32).unwrap();

            let mut candidates = WordCandidate::new(word.to_string());

            let mut stream = self.fst.search(lev).into_stream();
            while let Some((matched, id)) = stream.next() {
                candidates.insert_with_maybe_typo(
                    std::str::from_utf8(matched).unwrap(),
                    &self.bitmaps[id as usize],
                );
            }

            ret.push(candidates);
        }

        // TODO add one extra fake word for the prefix search
        /*
        let mut stream = self.fst.search(lev.starts_with()).into_stream();
        while let Some((_matched, id)) = stream.next() {
            bitmap |= &self.bitmaps[id as usize];
        }
        */

        ret
    }
}

#[derive(Debug)]
pub(crate) struct WordCandidate {
    original: String,
    typos: Vec<RoaringBitmap>,
}

impl WordCandidate {
    pub fn new(original: String) -> Self {
        Self {
            original,
            // we have a maximum of 3 typos
            typos: vec![RoaringBitmap::new(); 4],
        }
    }

    // Since the fst::Automaton doesn't tells us which automaton matched and with how many typos or prefixes
    // we need to recompute the stuff ourselves and insert our shit in the right cell
    pub fn insert_with_maybe_typo(&mut self, other: &str, bitmap: &RoaringBitmap) {
        // TODO: why is this crate taking ownership of my value to do a read only operation :(
        let distance = DamerauLevenshtein {
            src: self.original.clone(),
            tar: other.to_string(),
            restricted: true,
        }
        .distance();

        // distance shouldn't be able to go over 3 but we don't want any crash so let's ensure that
        let distance = distance.min(3);
        self.typos[distance] |= bitmap;
    }
}

pub struct Search<'a> {
    input: &'a str,
    limit: usize,
    words: Vec<String>,
    ranking_rules: Vec<RankingRule>,
}

impl<'a> Search<'a> {
    pub fn new(input: &'a str) -> Self {
        let words: Vec<_> = input
            .split_whitespace()
            .map(|word| normalize(word))
            .filter(|word| !word.is_empty())
            .collect();

        Self {
            input,
            limit: 10,
            words,
            ranking_rules: vec![RankingRule::Word, RankingRule::Typo],
        }
    }
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c.to_ascii_lowercase() {
            'á' | 'â' | 'à' | 'ä' => Some('a'),
            'é' | 'ê' | 'è' | 'ë' => Some('e'),
            'í' | 'î' | 'ì' | 'ï' => Some('i'),
            'ó' | 'ô' | 'ò' | 'ö' => Some('o'),
            'ú' | 'û' | 'ù' | 'ü' => Some('u'),
            c if c.is_ascii_punctuation() || !c.is_ascii_graphic() || c.is_ascii_control() => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    fn create_small_index() -> Index {
        let names = [
            "Tamo le plus beau",
            "kefir le bon petit chien",
            "kefir le beau chien",
            "tamo est très beau aussi",
            "le plus beau c'est kefir",
            "mais il est un peu con",
            "le petit kefir",
            "kefirounet se prends pour un poney",
            "kefirounet a un gros nez",
            "kefir est un demi poney",
            "le double kef",
            "les keftas c'est bon aussi",
        ];
        Index::construct(names.into_iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn test_search_with_only_word() {
        let index = create_small_index();
        let mut search = Search::new("tamo");
        search.ranking_rules = vec![RankingRule::Word];

        insta::assert_debug_snapshot!(index.search(&search), @r###"
        [
            "Tamo le plus beau",
            "tamo est très beau aussi",
        ]
        "###);

        // "tamo est" was matched first and then tamo alone
        let mut search = Search::new("tamo est");
        search.ranking_rules = vec![RankingRule::Word];
        insta::assert_debug_snapshot!(index.search(&search), @r###"
        [
            "tamo est très beau aussi",
            "Tamo le plus beau",
        ]
        "###);

        // "kefir" was removed right after we found no matches for both matches
        // and thus no prefix search was ran and we missed kefirounet
        let mut search = Search::new("beau kefir");
        search.ranking_rules = vec![RankingRule::Word];
        insta::assert_debug_snapshot!(index.search(&search), @r###"
        [
            "kefir le beau chien",
            "le plus beau c'est kefir",
            "Tamo le plus beau",
            "tamo est très beau aussi",
        ]
        "###);
    }
}
