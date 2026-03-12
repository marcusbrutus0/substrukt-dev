use std::path::PathBuf;

use minijinja::Environment;
use minijinja_autoreload::AutoReloader;

use crate::audit::AuditLogger;
use crate::config::Config;

pub fn create_reloader(
    schemas_dir: PathBuf,
    audit_logger: AuditLogger,
    config: Config,
) -> AutoReloader {
    AutoReloader::new(move |notifier| {
        let mut env = Environment::new();

        // Debug: load from filesystem with hot-reload
        // Release: embed templates into the binary
        #[cfg(debug_assertions)]
        {
            env.set_loader(minijinja::path_loader("templates/"));
            notifier.set_fast_reload(true);
        }

        #[cfg(not(debug_assertions))]
        {
            let _ = notifier;
            minijinja_embed::load_templates!(&mut env);
        }

        // Default base_template — overridden to "_partial.html" for htmx requests
        env.add_global("base_template", minijinja::Value::from("base.html"));
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

        let audit_for_tpl = audit_logger.clone();
        let config_for_tpl = config.clone();
        env.add_function("get_publish_state", move || -> minijinja::Value {
            let audit = audit_for_tpl.clone();
            let handle = tokio::runtime::Handle::current();
            // Spawn a thread to bridge async→sync, works on both multi-thread and current-thread runtimes
            let (staging_dirty, production_dirty) = std::thread::spawn(move || {
                handle.block_on(async {
                    let s = audit.is_dirty("staging").await.unwrap_or(false);
                    let p = audit.is_dirty("production").await.unwrap_or(false);
                    (s, p)
                })
            })
            .join()
            .unwrap_or((false, false));
            minijinja::context! {
                staging_configured => config_for_tpl.staging_webhook_url.is_some(),
                production_configured => config_for_tpl.production_webhook_url.is_some(),
                staging_dirty => staging_dirty,
                production_dirty => production_dirty,
            }
        });

        Ok(env)
    })
}

/// Returns the base template name based on whether this is an htmx request.
pub fn base_for_htmx(is_htmx: bool) -> &'static str {
    if is_htmx {
        "_partial.html"
    } else {
        "base.html"
    }
}
