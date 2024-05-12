use std::io;

use speedy::Readable;
use zearch::{Index, Search};

#[derive(Readable)]
pub struct Names {
    pub names: Vec<String>,
}

fn main() {
    println!("Loading data from disk...");
    let now = std::time::Instant::now();
    let names = Names::read_from_file("names_for_tamo.bin").unwrap();
    println!("Done in {:?}", now.elapsed());

    let mut index = Vec::new();
    println!("Constructing the index...");
    let now = std::time::Instant::now();
    Index::construct(
        &names.names.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        &mut index,
    )
    .unwrap();
    println!("Done in {:?}", now.elapsed());
    let index = Index::from_bytes(&index).unwrap();

    loop {
        println!();
        println!();
        println!("Searching for :");
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => (),
            Err(error) => {
                println!("error: {error}");
                break;
            }
        }

        let now = std::time::Instant::now();
        let ret = index.search(&Search::new(&input));

        println!("Found (in {:?}):", now.elapsed());
        for id in ret {
            println!("{}", index.get_document(id).unwrap());
        }
    }
}
