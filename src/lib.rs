mod ranking_rules;

use fst::{Automaton, IntoStreamer, Map, MapBuilder, Streamer};
use ranking_rules::{word::Word, RankingRule, RankingRuleImpl};
use roaring::{MultiOps, RoaringBitmap};

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

        let candidates = self.get_candidates(&search);
        let universe = candidates.as_slice().union();

        let mut ranking_rules: Vec<Box<dyn RankingRuleImpl>> = search
            .ranking_rules
            .iter()
            .map(|ranking_rule| match ranking_rule {
                RankingRule::Word => Box::new(Word::new(&candidates)) as Box<dyn RankingRuleImpl>,
                _ => panic!(),
                // RankingRule::Prefix => Box::new(todo!()) as Box<dyn RankingRuleImpl>,
                // RankingRule::Typo => Box::new(todo!()) as Box<dyn RankingRuleImpl>,
                // RankingRule::Exact => Box::new(todo!()) as Box<dyn RankingRuleImpl>,
            })
            .collect();

        let mut current_ranking_rule = 0;
        // store the current universe of each ranking rules
        let mut universes = vec![universe];

        while res.iter().map(|bucket| bucket.len()).sum::<u64>() < search.limit as u64 {
            let ranking_rule = &mut ranking_rules[current_ranking_rule];
            let next = ranking_rule.next(&universes[current_ranking_rule]);

            if next.is_none() {
                // if we're at the first ranking rule and there is nothing to sort, we don't have anything left to sort
                if current_ranking_rule == 0 {
                    break;
                }
                // else, we finished our current ranking rules and should come back one level above
                current_ranking_rule -= 1;
                let next = universes.pop().unwrap();
                universes[current_ranking_rule] -= &next;
                res.push(next);
            }

            let next = next.unwrap();

            // if we generated a bucket of one element we can skip the rest of the bucket, they won't be able to sort anything
            // or if we're already at the last ranking rule, we shouldn't advance
            if next.len() == 1 || current_ranking_rule + 1 == ranking_rules.len() {
                // everything that was sorted by the current ranking rule should be removed
                // from the current one
                universes[current_ranking_rule] -= &next;
                res.push(next);
                // we stay on the same ranking rule
                continue;
            }

            universes.push(next);
            current_ranking_rule += 1;
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

    fn get_candidates(&self, search: &Search) -> Vec<RoaringBitmap> {
        let mut ret = Vec::with_capacity(search.words.len());

        for (idx, word) in search.words.iter().enumerate() {
            // enable 1 typo every 3 letters maxed at 3 typos
            let typo = (word.len() / 3).min(3);
            let lev = fst::automaton::Levenshtein::new(word, typo as u32).unwrap();

            let mut bitmap = RoaringBitmap::new();
            // For the last word we enable the prefix search
            if idx == search.words.len() - 1 {
                let mut stream = self.fst.search(lev.starts_with()).into_stream();
                while let Some((_matched, id)) = stream.next() {
                    bitmap |= &self.bitmaps[id as usize];
                }
            } else {
                let mut stream = self.fst.search(lev).into_stream();
                while let Some((_matched, id)) = stream.next() {
                    bitmap |= &self.bitmaps[id as usize];
                }
            }

            ret.push(bitmap);
        }

        ret
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
            ranking_rules: vec![RankingRule::Word],
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
