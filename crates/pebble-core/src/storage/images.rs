use super::Database;
use crate::{
    Error, ImageAsset,
    images::{self, ImageReference},
};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

pub(crate) fn root(db: &Connection) -> Result<PathBuf, Error> {
    let path: String = db.query_row(
        "SELECT file FROM pragma_database_list WHERE name='main'",
        [],
        |r| r.get(0),
    )?;
    Path::new(&path)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| images::invalid("Asset directory is unavailable."))
}
pub(crate) fn references(db: &Connection, message: &str) -> Result<Vec<ImageReference>, Error> {
    let mut q=db.prepare("SELECT a.reference FROM message_images m JOIN image_assets a ON a.id=m.asset_id WHERE m.message_id=?1 ORDER BY m.position")?;
    q.query_map([message], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str(&r?)?))
        .collect()
}
pub(crate) fn assets(db: &Connection, message: &str) -> Result<Vec<ImageAsset>, Error> {
    let root = root(db)?;
    let mut q=db.prepare("SELECT a.id,a.mime,a.width,a.height,a.source_bytes,a.model_bytes,a.source_name FROM message_images m JOIN image_assets a ON a.id=m.asset_id WHERE m.message_id=?1 ORDER BY m.position")?;
    Ok(q.query_map([message], |r| {
        let id: String = r.get(0)?;
        let folder = root.join("data/assets").join(&id);
        Ok(ImageAsset {
            id,
            mime: r.get(1)?,
            width: r.get(2)?,
            height: r.get(3)?,
            source_bytes: r.get::<_, i64>(4)? as u64,
            model_bytes: r.get::<_, i64>(5)? as u64,
            source_path: folder.join(r.get::<_, String>(6)?).display().to_string(),
            thumbnail_path: folder.join("thumbnail.png").display().to_string(),
        })
    })?
    .collect::<Result<_, _>>()?)
}
pub(crate) fn attach(db: &Connection, message: &str, ids: &[String]) -> Result<(), Error> {
    if ids.len() > images::MAX_IMAGES {
        return Err(images::invalid("Send at most four images in one message."));
    }
    for (position, id) in ids.iter().enumerate() {
        if ids[..position].contains(id) {
            return Err(images::invalid(
                "An image can only appear once in a message.",
            ));
        }
        let reference: String = db
            .query_row(
                "SELECT reference FROM image_assets WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(|_| images::invalid("Imported image is no longer available."))?;
        let r: ImageReference = serde_json::from_str(&reference)?;
        let mut check = [serde_json::json!({"role":"user","content":[images::reference_part(&r)]})];
        images::materialize(&root(db)?, &mut check)?;
        db.execute(
            "INSERT INTO message_images(message_id,asset_id,position) VALUES(?1,?2,?3)",
            params![message, id, position as i64],
        )?;
    }
    Ok(())
}
impl Database {
    pub(crate) fn import_image(&self, bytes: &[u8]) -> Result<ImageAsset, Error> {
        let (asset, reference) = images::import(&self.root, bytes)?;
        self.connection.lock().expect("database lock").execute("INSERT INTO image_assets(id,mime,width,height,source_bytes,model_bytes,source_name,reference) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![asset.id,asset.mime,asset.width,asset.height,asset.source_bytes as i64,asset.model_bytes as i64,Path::new(&asset.source_path).file_name().unwrap().to_str().unwrap(),serde_json::to_string(&reference)?])?;
        Ok(asset)
    }
}
