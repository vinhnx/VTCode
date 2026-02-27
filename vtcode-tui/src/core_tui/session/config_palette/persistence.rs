use super::ConfigPalette;

impl ConfigPalette {
    pub fn apply_changes(&mut self) -> anyhow::Result<()> {
        if self.modified {
            self.config_manager.save_config(&self.config)?;
            self.modified = false;
        }
        Ok(())
    }
}
