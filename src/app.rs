use std::path::PathBuf;

use axum::{Router, routing::get};

use crate::{
    api, frontend,
    runtime::RuntimeContext,
    storage::{Database, TemplateStore, WorkflowStore, seed_bundled_templates},
    util::ensure_dir,
};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub workflows_dir: PathBuf,
    pub templates_dir: PathBuf,
    pub database_path: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            workflows_dir: root.join("workflows"),
            templates_dir: root.join("templates"),
            database_path: root.join(".silverbond").join("silverbond.sqlite"),
            root,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    pub paths: AppPaths,
    pub seed_bundled_templates: bool,
}

impl ApplicationConfig {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self {
            paths: AppPaths::from_root(root),
            seed_bundled_templates: false,
        }
    }

    pub fn from_cli_environment() -> anyhow::Result<Self> {
        let root = std::env::var_os("SILVERBOND_ROOT")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        Ok(Self::from_root(root))
    }
}

#[derive(Clone)]
pub struct AppState {
    pub paths: AppPaths,
    pub workflows: WorkflowStore,
    pub templates: TemplateStore,
    pub runtime: RuntimeContext,
}

pub struct Application {
    state: AppState,
    pub paths: AppPaths,
}

impl Application {
    pub async fn boot(config: ApplicationConfig) -> anyhow::Result<Self> {
        let paths = config.paths;
        ensure_dir(&paths.workflows_dir)?;
        ensure_dir(&paths.templates_dir)?;
        ensure_dir(
            paths
                .database_path
                .parent()
                .expect("database parent exists"),
        )?;
        if config.seed_bundled_templates {
            seed_bundled_templates(&paths.templates_dir)?;
        }

        let workflows = WorkflowStore::new(paths.workflows_dir.clone());
        let templates = TemplateStore::new(paths.templates_dir.clone());
        let db = Database::new(paths.database_path.clone());
        db.init().await?;
        let runtime = RuntimeContext::new(db);

        let state = AppState {
            paths: paths.clone(),
            workflows,
            templates,
            runtime,
        };

        Ok(Self { state, paths })
    }

    pub fn router(&self) -> Router {
        api::router(self.state.clone())
            .route(
                "/",
                get(|| async { frontend::serve("/".to_string()).await }),
            )
            .route(
                "/{*path}",
                get(
                    |axum::extract::Path(path): axum::extract::Path<String>| async move {
                        frontend::serve(path).await
                    },
                ),
            )
    }
}
