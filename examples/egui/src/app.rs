use egui::ScrollArea;

pub struct TemplateApp {
    index: zearch::Index<'static>,
    query: String,
    #[cfg(not(target_arch = "wasm32"))]
    processing_time: std::time::Duration,
    limit: usize,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let database = std::include_bytes!("../database.zearch");
        Self {
            index: zearch::Index::from_bytes(database).unwrap(),
            query: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            processing_time: std::time::Duration::from_secs(0),
            limit: 10,
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

            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Write a french city name: ");
                changed |= ui.text_edit_singleline(&mut self.query).changed();
            });

            #[cfg(not(target_arch = "wasm32"))]
            ui.label(format!(
                "Processed the search in {:?}",
                self.processing_time
            ));
            changed |= ui
                .add(egui::Slider::new(&mut self.limit, 1..=50).text("limit"))
                .changed();

            ui.separator();

            #[cfg(not(target_arch = "wasm32"))]
            let now = std::time::Instant::now();
            let mut search = zearch::Search::new(&self.query);
            let results = self.index.search(search.with_limit(self.limit));

            // Ideally we shouldn't run the search for every frame
            // but I have other stuff to do before optimizing that
            #[cfg(not(target_arch = "wasm32"))]
            if changed {
                self.processing_time = now.elapsed();
            }

            ScrollArea::vertical().show(ui, |ui| {
                for result in results {
                    ui.label(result);
                }
            });
        });
    }
}
