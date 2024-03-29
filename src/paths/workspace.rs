use crate::utils::serde_ext::Json;

use super::*;
use project::Project;

use nanoid::nanoid;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Getters, Clone, PartialEq, Debug)]
pub struct Workspace {
    id: String,
    base_dir: PathBuf,
    cache_dir: PathBuf,
    bin_dir: PathBuf,
    home_dir: PathBuf,
}

impl Workspace {
    pub fn new(id: String, base_unique_workspace_dir: PathBuf, unique_cache_dir: PathBuf) -> Self {
        let home_dir = base_unique_workspace_dir.join("home");
        let bin_dir = home_dir.join("bin");
        Self {
            id,
            bin_dir,
            home_dir,
            cache_dir: unique_cache_dir,
            base_dir: base_unique_workspace_dir,
        }
    }
    pub fn make_tmp_workspace(&self) -> Self {
        Self::new(
            self.id().clone(),
            self.cache_dir().join(nanoid!()),
            self.cache_dir().clone(),
        )
    }
}

#[derive(Default)]
pub struct WorkspaceProvider {
    project: Project,
}

impl WorkspaceProvider {
    pub async fn base_unique_workspace_dir_from_isobin_manifest_dir(
        &self,
        isobin_manifest_dir: impl AsRef<Path>,
    ) -> Result<Workspace> {
        let mut workspace_path_map =
            WorkspacePathMap::parse_from_dir(self.project.data_local_dir()).await?;
        let id = if let Some(id) = workspace_path_map
            .workspace_path_map
            .get(isobin_manifest_dir.as_ref().to_str().unwrap())
        {
            id.into()
        } else {
            let id = nanoid!();
            workspace_path_map.workspace_path_map.insert(
                isobin_manifest_dir.as_ref().to_str().unwrap().into(),
                id.to_string(),
            );
            WorkspacePathMap::save_to_dir(&workspace_path_map, self.project.data_local_dir())
                .await?;
            id
        };
        let base_unique_workspace_dir = self.project.data_local_dir().join(&id);
        let unique_cache_dir = self.project.cache_dir().join(&id);
        Ok(Workspace::new(
            id,
            base_unique_workspace_dir,
            unique_cache_dir,
        ))
    }
    pub async fn remove_isobin_manifest_dir_from_workspace_map(
        &self,
        isobin_manifest_dir: impl AsRef<Path>,
    ) -> Result<()> {
        let mut workspace_path_map =
            WorkspacePathMap::parse_from_dir(self.project.data_local_dir()).await?;
        workspace_path_map
            .workspace_path_map
            .remove(isobin_manifest_dir.as_ref().to_str().unwrap());
        WorkspacePathMap::save_to_dir(&workspace_path_map, self.project.data_local_dir()).await
    }
}

#[derive(Deserialize, Serialize, Default, Debug)]
struct WorkspacePathMap {
    #[serde(default, flatten)]
    workspace_path_map: HashMap<String, String>,
}

impl WorkspacePathMap {
    const WORKSPACE_PATH_MAP_FILE_NAME: &'static str = "workspace_map.v1.json";
    async fn parse_from_dir(dir: impl AsRef<Path>) -> Result<WorkspacePathMap> {
        let workspace_path_map_file_path = dir.as_ref().join(Self::WORKSPACE_PATH_MAP_FILE_NAME);
        if workspace_path_map_file_path.exists() {
            Ok(Json::parse_from_file(workspace_path_map_file_path).await?)
        } else {
            Ok(WorkspacePathMap::default())
        }
    }

    async fn save_to_dir(workspace_path_map: &Self, dir: impl AsRef<Path>) -> Result<()> {
        let workspace_path_map_file_path = dir.as_ref().join(Self::WORKSPACE_PATH_MAP_FILE_NAME);
        Json::save_to_file(workspace_path_map, workspace_path_map_file_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest]
    #[case(
        "332334", 
        "/home/user_name/.local/share/332334".into(),
        "/home/user_name/.cache/332334".into(),
        Workspace{
            cache_dir:"/home/user_name/.cache/332334".into(),
            base_dir:"/home/user_name/.local/share/332334".into(),
            bin_dir:"/home/user_name/.local/share/332334/home/bin".into(),
            home_dir:"/home/user_name/.local/share/332334/home".into(),
            id:"332334".into(),
        }
    )]
    fn workspace_new_works(
        #[case] id: &str,
        #[case] base_unique_workspace_dir: PathBuf,
        #[case] unique_cache_dir: PathBuf,
        #[case] expected: Workspace,
    ) {
        let actual = Workspace::new(id.into(), base_unique_workspace_dir, unique_cache_dir);
        pretty_assertions::assert_eq!(expected, actual);
    }
}
