use fst::{automaton::Subsequence, IntoStreamer, Map, MapBuilder};
use roaring::RoaringBitmap;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io")]
    Io,
}

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

    pub fn search<'a>(&'a self, input: &str, limit: usize) -> Vec<&'a str> {
        let words: Vec<_> = input
            .split_whitespace()
            .map(|word| normalize(word))
            .filter(|word| !word.is_empty())
            .collect();
        if words.len() == 0 {
            return Vec::new();
        }

        // this'll stores all the buckets we've retrieved
        let mut res: Vec<RoaringBitmap> = Vec::new();
        let mut already_in;

        let per_word: Vec<&RoaringBitmap> = words
            .iter()
            .filter_map(|word| self.fst.get(word))
            .map(|idx| &self.bitmaps[idx as usize])
            .collect();
        // first return everything that contains exactly all words without typos
        let first = per_word
            .clone()
            .into_iter()
            .cloned()
            .reduce(|acc, other| acc & other)
            .unwrap();
        already_in = first.clone();
        res.push(first);

        'finish: {
            if already_in.len() as usize >= limit {
                break 'finish;
            }

            // Second: return everything that contains all the word + the last word as a prefix
            let mut prefix = per_word.clone();
            prefix.pop();
            let mut prefix = prefix
                .into_iter()
                .cloned()
                .reduce(|acc, other| acc & other)
                .unwrap();
            let matcher = Subsequence::new(words.last().unwrap());
            let last_prefix = self
                .fst
                .search(&matcher)
                .into_stream()
                .into_str_vec()
                .unwrap()
                .into_iter()
                .map(|(_, idx)| &self.bitmaps[idx as usize])
                .fold(RoaringBitmap::new(), |acc, bitmap| acc | bitmap);
            prefix &= last_prefix;
            // let mut prefix = prefix.difference();
            prefix -= &already_in;
            already_in |= &prefix;
            res.push(prefix);

            if already_in.len() as usize >= limit {
                break 'finish;
            }
        }

        // TODO: in the first bucket we may have exact matches that we should put above the other
        // => with everything we normalized, not doing that can be very frustrating
        res.iter()
            .flat_map(|bitmap| {
                bitmap
                    .iter()
                    .map(|idx| self.documents[idx as usize].as_ref())
            })
            .take(limit)
            .collect()
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
