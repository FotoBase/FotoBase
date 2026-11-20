use std::collections::BTreeMap;

use crate::{
    CancelToken, ExportError, ExportPlugin, PluginInfo, ProgressSink, UploadJob, UploadReport,
};

/// Upload targets keyed by plugin id; the one place the UI lists targets from.
#[derive(Default)]
pub struct Registry {
    plugins: BTreeMap<String, Box<dyn ExportPlugin>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Box<dyn ExportPlugin>) -> Result<(), ExportError> {
        let id = plugin.info().id;
        if self.plugins.contains_key(&id) {
            return Err(ExportError::DuplicatePlugin(id));
        }
        self.plugins.insert(id, plugin);
        Ok(())
    }

    /// Registered plugins, sorted by id.
    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins.values().map(|p| p.info()).collect()
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut (dyn ExportPlugin + 'static)> {
        self.plugins.get_mut(id).map(|p| &mut **p)
    }

    pub async fn upload(
        &mut self,
        id: &str,
        job: UploadJob,
        progress: &dyn ProgressSink,
        cancel: &CancelToken,
    ) -> Result<UploadReport, ExportError> {
        let plugin = self.get_mut(id).ok_or_else(|| ExportError::UnknownPlugin(id.to_owned()))?;
        plugin.upload(job, progress, cancel).await
    }
}
