use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::db::Database;

use super::model::Hook;

pub(crate) const MAX_CONCURRENT_HOOKS: usize = 2;

pub struct HookRunner {
    hooks: Vec<Hook>,
    semaphore: Arc<Semaphore>,
    db: Option<Arc<Database>>,
}

impl HookRunner {
    pub fn new(hooks: Vec<Hook>) -> Self {
        Self {
            hooks,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_HOOKS)),
            db: None,
        }
    }

    pub fn with_db(mut self, db: Arc<Database>) -> Self {
        self.db = Some(db);
        self
    }

    pub fn hooks(&self) -> &[Hook] {
        &self.hooks
    }
}

mod on_message;
mod schedule;
