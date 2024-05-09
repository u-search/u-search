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

    println!("Constructing the index...");
    let now = std::time::Instant::now();
    let index = Index::construct(names.names);
    println!("Done in {:?}", now.elapsed());

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
        let ret = index.search(Search::new(&input));

        println!("Found (in {:?}):", now.elapsed());
        for name in ret {
            println!("{name}");
        }
    }
}
