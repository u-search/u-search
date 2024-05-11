mod ranking_rules;

use std::{borrow::Cow, ops::ControlFlow};

use fst::{Automaton, IntoStreamer, Map, MapBuilder, Streamer};
use ranking_rules::{typo::Typo, word::Word, RankingRule, RankingRuleImpl};
use roaring::RoaringBitmap;
use text_distance::DamerauLevenshtein;

use crate::ranking_rules::exact::Exact;

pub struct Index<'a> {
    documents: Vec<Cow<'a, str>>,
    // we cannot work on serialized bitmap yet thus we're going to load everything in RAM
    bitmaps: Vec<RoaringBitmap>,
    fst: Map<Cow<'a, [u8]>>,
}

type Id = u32;

impl<'a> Index<'a> {
    pub fn construct(documents: &[&str], writer: &mut impl std::io::Write) -> std::io::Result<()> {
        println!("Extracting and normalizing the words...");
        let now = std::time::Instant::now();
        let mut words = documents
            .iter()
            .enumerate()
            .flat_map(|(id, document)| {
                document
                    .split_whitespace()
                    .map(move |word| (id as Id, normalize(word)))
            })
            .collect::<Vec<(Id, String)>>();
        println!("Took {:?}", now.elapsed());

        println!("Sorting the words...");
        let now = std::time::Instant::now();
        words.sort_unstable_by(|(_, left), (_, right)| left.cmp(right));
        println!("Took {:?}", now.elapsed());

        println!("Building the reverse indexes and the fst...");
        let now = std::time::Instant::now();

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
        println!("Took {:?}", now.elapsed());

        println!("Writing the documents...");
        let now = std::time::Instant::now();
        writer.write_all(documents.len().to_be_bytes().as_slice())?;
        for document in documents {
            Self::write_slice(writer, document.as_bytes())?;
        }
        println!("Took {:?}", now.elapsed());

        println!("Writing the bitmaps...");
        let now = std::time::Instant::now();
        writer.write_all(bitmaps.len().to_be_bytes().as_slice())?;
        for bitmap in bitmaps {
            bitmap.serialize_into(&mut *writer)?;
        }
        println!("Took {:?}", now.elapsed());

        println!("Writing the fst...");
        let now = std::time::Instant::now();
        // cannot fail since we were writing in memory
        let fst = build.into_inner().unwrap();
        Self::write_slice(writer, &fst)?;
        println!("Took {:?}", now.elapsed());

        Ok(())
    }

    fn write_slice(writer: &mut impl std::io::Write, slice: &[u8]) -> std::io::Result<()> {
        writer.write_all(slice.len().to_be_bytes().as_slice())?;
        writer.write_all(slice)?;
        Ok(())
    }

    fn read_size_from_bytes(bytes: &mut &[u8]) -> Option<usize> {
        let (size, b) = bytes.split_first_chunk::<8>()?;
        *bytes = b;
        Some(usize::from_be_bytes(*size))
    }

    fn read_slice_from_bytes<'b>(bytes: &mut &'b [u8]) -> Option<&'b [u8]> {
        let size = Self::read_size_from_bytes(bytes)?;
        if bytes.len() < size {
            return None;
        }
        let ret = &bytes[..size];
        *bytes = &bytes[size..];

        Some(ret)
    }

    pub fn from_bytes(mut bytes: &'a [u8]) -> Option<Self> {
        // 1. Read the documents
        println!("Reading the documents...");
        let now = std::time::Instant::now();
        let mut documents = Vec::new();
        let nb_documents = Self::read_size_from_bytes(&mut bytes)?;
        for _ in 0..nb_documents {
            let document = Self::read_slice_from_bytes(&mut bytes)?;
            documents.push(Cow::Borrowed(std::str::from_utf8(document).ok()?));
        }
        println!("Took {:?}", now.elapsed());

        // 2. Read the bitmap
        println!("Reading the bitmaps...");
        let now = std::time::Instant::now();
        let nb_bitmaps = Self::read_size_from_bytes(&mut bytes)?;
        let mut bitmaps = Vec::new();
        for _ in 0..nb_bitmaps {
            let bitmap = RoaringBitmap::deserialize_from(&mut bytes).unwrap();
            bitmaps.push(bitmap);
        }
        println!("Took {:?}", now.elapsed());

        // 3. Read the fst
        println!("Reading the fst...");
        let now = std::time::Instant::now();
        let fst = Self::read_slice_from_bytes(&mut bytes)?;
        let fst = Map::new(Cow::Borrowed(fst)).ok()?;
        println!("Took {:?}", now.elapsed());

        Some(Self {
            documents,
            bitmaps,
            fst,
        })
    }

    pub fn move_in_memory(self) -> Index<'static> {
        Index {
            documents: self
                .documents
                .into_iter()
                .map(|document| Cow::Owned(document.into_owned()))
                .collect(),
            bitmaps: self.bitmaps,
            fst: self
                .fst
                .map_data(|data| Cow::Owned(data.into_owned()))
                .unwrap(),
        }
    }

    pub fn new_in_memory(documents: &[&str]) -> Option<Index<'static>> {
        let mut index = Vec::new();
        Self::construct(documents, &mut index).ok()?;
        let index = Index::from_bytes(&index)?;
        Some(index.move_in_memory())
    }

    // the lifetime is not tied to 'a here, if 'a was made static by shoving
    // the Self on ram the ref would expire when the struct is dropped.
    // Here the lifetime is tied to the reference to self.
    pub fn search<'b>(&'b self, search: &Search) -> Vec<&'b str> {
        // contains all the buckets
        let mut res: Vec<RoaringBitmap> = Vec::new();
        let mut candidates = self.get_candidates(&search);

        // TODO: returns random results maybe?
        if candidates.len() == 0 {
            return Vec::new();
        }

        let mut ranking_rules: Vec<Box<dyn RankingRuleImpl>> = search
            .ranking_rules
            .iter()
            .map(|ranking_rule| match ranking_rule {
                RankingRule::Word => {
                    Box::new(Word::new(&mut candidates)) as Box<dyn RankingRuleImpl>
                }
                RankingRule::Typo => Box::new(Typo::new(&candidates)) as Box<dyn RankingRuleImpl>,
                RankingRule::Exact => Box::new(Exact::new()) as Box<dyn RankingRuleImpl>,
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
                    self
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
                        let bucket = ranking_rule.current_results(&candidates);
                        Self::cleanup(&bucket, &mut candidates);
                        ranking_rules.iter_mut().for_each(|rr| rr.cleanup(&bucket));
                        res.push(bucket);
                    } else {
                        // we advance and do nothing
                        current_ranking_rule += 1;
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
                    ranking_rules.iter_mut().for_each(|rr| rr.cleanup(&bucket));
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
        let words: Vec<_> = search
            .input
            .split_whitespace()
            .map(|word| (word, normalize(word)))
            .filter(|(_word, normalized)| !normalized.is_empty())
            .collect();
        let mut ret = Vec::with_capacity(words.len());

        for (index, (word, normalized)) in words.iter().enumerate() {
            let mut candidates =
                WordCandidate::new(word.to_string(), normalized.to_string(), index);

            // enable 1 typo every 3 letters maxed at 3 typos
            let typo = (normalized.len() / 3).min(3);
            let lev = fst::automaton::Levenshtein::new(normalized, typo as u32).unwrap();

            // if we're at the last word we should also run a prefix search
            if index == words.len() - 1 {
                let mut stream = self.fst.search(lev.starts_with()).into_stream();
                while let Some((matched, id)) = stream.next() {
                    candidates.insert_with_maybe_typo(
                        std::str::from_utf8(matched).unwrap(),
                        &self.bitmaps[id as usize],
                    );
                }
            } else {
                let mut stream = self.fst.search(lev).into_stream();
                while let Some((matched, id)) = stream.next() {
                    candidates.insert_with_maybe_typo(
                        std::str::from_utf8(matched).unwrap(),
                        &self.bitmaps[id as usize],
                    );
                }
            }

            ret.push(candidates);
        }

        ret
    }
}

#[derive(Debug)]
pub(crate) struct WordCandidate {
    // the original string
    original: String,
    // normalized string
    normalized: String,
    // its index in the phrase
    index: usize,
    // the number of documuents its contained in
    typos: Vec<RoaringBitmap>,
}

impl WordCandidate {
    pub fn new(original: String, normalized: String, index: usize) -> Self {
        Self {
            original,
            normalized,
            index,
            // we have a maximum of 3 typos
            typos: vec![RoaringBitmap::new(); 4],
        }
    }

    // Since the fst::Automaton doesn't tells us which automaton matched and with how many typos or prefixes
    // we need to recompute the stuff ourselves and insert our shit in the right cell
    pub fn insert_with_maybe_typo(&mut self, other: &str, bitmap: &RoaringBitmap) {
        // TODO: why is this crate taking ownership of my value to do a read only operation :(
        let distance = DamerauLevenshtein {
            src: self.normalized.clone(),
            // if we did a prefix query we shouldn't count the extra letters as typo
            tar: other[0..other.len().min(self.normalized.len())].to_string(),
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
    ranking_rules: Vec<RankingRule>,
}

impl<'a> Search<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            limit: 10,
            ranking_rules: vec![RankingRule::Word, RankingRule::Typo, RankingRule::Exact],
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
    use core::slice::SlicePattern;

    use super::*;

    fn create_small_index() -> Index<'static> {
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
        Index::new_in_memory(names.as_slice()).unwrap()
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
