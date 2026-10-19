use crate::{AppError, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use storyfold_core::{Project, ProjectEdit, apply_edit};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
    pub data: PathBuf,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}
impl Store {
    pub fn new(root: impl AsRef<Path>, data: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        fs::create_dir_all(data.as_ref())?;
        let data = data.as_ref().canonicalize()?;
        let s = Self {
            root,
            data,
            locks: Default::default(),
        };
        s.directory(&s.data.join("projects"))?;
        Ok(s)
    }
    pub fn project_dir(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id.len() > 60
            || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(AppError::bad("Invalid project identifier."));
        }
        let projects = self.data.join("projects");
        reject_link(&projects)?;
        let p = projects.join(id);
        reject_link(&p)?;
        Ok(p)
    }
    pub fn directory(&self, path: &Path) -> Result<()> {
        let base = if path.starts_with(&self.data) {
            &self.data
        } else if path.starts_with(&self.root) {
            &self.root
        } else {
            return Err(AppError::bad("Folder is outside the application."));
        };
        let relative = path
            .strip_prefix(base)
            .map_err(|_| AppError::bad("Invalid folder."))?;
        let mut cursor = base.clone();
        for part in relative.components() {
            let Component::Normal(part) = part else {
                return Err(AppError::bad("Invalid folder path."));
            };
            cursor.push(part);
            reject_link(&cursor)?;
            if !cursor.exists() {
                fs::create_dir(&cursor)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&cursor, fs::Permissions::from_mode(0o700))?;
                }
            }
            if !cursor.is_dir() {
                return Err(AppError::bad("Expected a normal folder."));
            }
        }
        Ok(())
    }
    pub fn asset(&self, id: &str, name: &str) -> Result<PathBuf> {
        if name.is_empty()
            || name.starts_with('.')
            || name.contains(['/', '\\'])
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return Err(AppError::bad("Invalid media path."));
        }
        let p = self.project_dir(id)?.join(name);
        reject_link(&p)?;
        Ok(p)
    }
    pub async fn read(&self, id: &str) -> Result<Project> {
        let p: Project = read_json(&self.asset(id, "project.json")?).await?;
        if p.id != id || p.cameras.is_empty() || p.cameras.len() > 3 || p.cameras[0].id != "A" {
            return Err(AppError::bad(
                "Saved project has invalid cameras or identity.",
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for c in &p.cameras {
            if !["A", "B", "C"].contains(&c.id.as_str())
                || !seen.insert(&c.id)
                || !c.duration.is_finite()
                || c.duration < 0.
                || c.duration > 7200.
                || !c.offset.is_finite()
                || c.offset.abs() > 7200.
            {
                return Err(AppError::bad("Saved project has invalid camera timing."));
            }
            self.asset(id, &c.source_path)?;
            self.asset(id, &c.proxy_path)?;
            if let Some(v) = &c.virtual_crop
                && (![v.scale, v.x, v.y].iter().all(|v| v.is_finite())
                    || !(1.0..=5.).contains(&v.scale)
                    || !(0.0..=1.).contains(&v.x)
                    || !(0.0..=1.).contains(&v.y))
            {
                return Err(AppError::bad("Invalid camera crop."));
            }
        }
        p.export_settings.validate().map_err(AppError::bad)?;
        Ok(p)
    }
    async fn lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .await
            .entry(id.into())
            .or_default()
            .clone()
    }
    pub async fn save(&self, p: &Project) -> Result<()> {
        let lock = self.lock(&p.id).await;
        let _guard = lock.lock().await;
        let dir = self.project_dir(&p.id)?;
        self.directory(&dir)?;
        write_json(&self.asset(&p.id, "project.json")?, p).await
    }
    pub async fn edit(&self, id: &str, input: ProjectEdit) -> Result<Project> {
        let lock = self.lock(id).await;
        let _guard = lock.lock().await;
        let old = self.read(id).await?;
        let file = self.asset(id, "project.json")?;
        let original = fs::read(&file)?;
        if serde_json::from_slice::<Project>(&original)? != old {
            return Err(AppError::conflict(
                "This project changed outside the app. Reload before saving.",
            ));
        }
        if old.revision != input.revision {
            return Err(AppError::conflict(
                "This project changed in another window. Reload before saving.",
            ));
        }
        let next = apply_edit(&old, input).map_err(AppError::bad)?;
        let backups = self.project_dir(id)?.join("backups");
        self.directory(&backups)?;
        write_atomic(
            &backups.join(format!(
                "revision-{}-{}.json",
                old.revision,
                uuid::Uuid::new_v4()
            )),
            &original,
        )?;
        if fs::read(&file)? != original {
            return Err(AppError::conflict(
                "This project changed outside the app. Reload before saving.",
            ));
        }
        write_json(&file, &next).await?;
        Ok(next)
    }
}
pub fn reject_link(path: &Path) -> Result<()> {
    if let Ok(m) = fs::symlink_metadata(path)
        && m.file_type().is_symlink()
    {
        return Err(AppError::bad("Linked project paths are not supported."));
    }
    Ok(())
}
pub async fn read_json<T: DeserializeOwned>(file: &Path) -> Result<T> {
    reject_link(file)?;
    if fs::metadata(file)?.len() > 64 * 1024 * 1024 {
        return Err(AppError::bad("Project JSON is too large."));
    }
    Ok(serde_json::from_slice(&fs::read(file)?)?)
}
pub async fn write_json<T: Serialize + ?Sized>(file: &Path, data: &T) -> Result<()> {
    write_atomic(file, &serde_json::to_vec_pretty(data)?)
}
fn write_atomic(file: &Path, data: &[u8]) -> Result<()> {
    reject_link(file)?;
    let parent = file
        .parent()
        .ok_or_else(|| AppError::bad("Missing project folder."))?;
    fs::create_dir_all(parent)?;
    reject_link(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(file).map_err(|e| AppError::from(e.error))?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
