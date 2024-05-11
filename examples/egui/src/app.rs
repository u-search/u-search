use egui::ScrollArea;

pub struct TemplateApp {
    index: zearch::Index<'static>,
    query: String,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let database = std::include_bytes!("../database.zearch");
        Self {
            index: zearch::Index::from_bytes(database).unwrap(),
            query: String::new(),
        }
    }
}

impl TemplateApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }
}

impl eframe::App for TemplateApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Zearch + wasm demo");

            ui.horizontal(|ui| {
                ui.label("Write a french city name: ");
                ui.text_edit_singleline(&mut self.query);
            });

            ui.separator();

            let now = std::time::Instant::now();

            let search = zearch::Search::new(&self.query);
            let results = self.index.search(&search);
            ui.label(format!("Processed the search in {:?}", now.elapsed()));

            ScrollArea::vertical().show(ui, |ui| {
                for result in results {
                    ui.label(result);
                }
            });
        });
    }
}
