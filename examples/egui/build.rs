fn main() {
    println!("cargo::rerun-if-changed=dataset.csv");

    let dataset = std::fs::File::open("dataset.csv").unwrap();
    let mut database = std::fs::File::create("database.zearch").unwrap();

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .from_reader(dataset);
    let mut city_names = Vec::new();

    for result in reader.records() {
        let record = result.unwrap();
        dbg!(&record);
        let name = record.get(1).unwrap();
        city_names.push(name.to_string());
    }

    zearch::Index::construct(city_names.as_slice(), &mut database).unwrap();
    database.sync_all().unwrap();
}
