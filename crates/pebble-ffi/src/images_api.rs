use crate::{BackendError, PebbleBackend};

#[derive(Clone, uniffi::Record)]
pub struct ImageAssetRecord {
    pub id: String,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub source_bytes: u64,
    pub model_bytes: u64,
    pub source_path: String,
    pub thumbnail_path: String,
}
impl From<pebble_core::ImageAsset> for ImageAssetRecord {
    fn from(image: pebble_core::ImageAsset) -> Self {
        Self {
            id: image.id,
            mime: image.mime,
            width: image.width,
            height: image.height,
            source_bytes: image.source_bytes,
            model_bytes: image.model_bytes,
            source_path: image.source_path,
            thumbnail_path: image.thumbnail_path,
        }
    }
}
#[uniffi::export]
impl PebbleBackend {
    pub fn import_image(&self, path: String) -> Result<ImageAssetRecord, BackendError> {
        Ok(self.core.import_image(std::path::Path::new(&path))?.into())
    }
    pub fn import_image_data(&self, bytes: Vec<u8>) -> Result<ImageAssetRecord, BackendError> {
        Ok(self.core.import_image_data(&bytes)?.into())
    }
}
