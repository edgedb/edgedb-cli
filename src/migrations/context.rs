use crate::commands::parser::MigrationConfig;

pub struct Context {
    schema_dir: PathBuf,
}

impl Context {
    fn from_config(cfg: &MigrationConfig) -> Context {
        Context {
            schema_dir: cfg.schema_dir.clone(),
        }
    }
}
