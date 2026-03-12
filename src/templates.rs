use std::path::PathBuf;

use minijinja::{Environment, path_loader};
use minijinja_autoreload::AutoReloader;

pub fn create_reloader(schemas_dir: PathBuf) -> AutoReloader {
    AutoReloader::new(move |notifier| {
        let mut env = Environment::new();
        env.set_loader(path_loader("templates/"));
        let sd = schemas_dir.clone();
        env.add_function("get_nav_schemas", move || -> Vec<minijinja::Value> {
            let schemas = crate::schema::list_schemas(&sd).unwrap_or_default();
            schemas
                .iter()
                .map(|s| {
                    minijinja::context! {
                        title => s.meta.title,
                        slug => s.meta.slug,
                    }
                })
                .collect()
        });
        notifier.set_fast_reload(true);
        Ok(env)
    })
}
