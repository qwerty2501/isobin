use nanoid::nanoid;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::fronts::MultiProgress;
use crate::fronts::Progress;
use crate::paths::isobin_config::isobin_config_dir;
use crate::paths::workspace::Workspace;
use crate::paths::workspace::WorkspaceProvider;
use crate::providers::cargo::CargoConfig;
use crate::providers::cargo::CargoInstallTarget;
use crate::providers::cargo::CargoInstallerFactory;
use crate::providers::InstallTarget;
use crate::utils::fs_ext;
use crate::utils::fs_ext::copy_dir;
use std::collections::HashSet;
use std::path::PathBuf;

use super::*;
use std::sync::Arc;

#[derive(PartialEq)]
pub enum InstallMode {
    All,
    SpecificInstallTargetsOnly {
        specific_install_targets: Vec<String>,
    },
}

#[derive(Default)]
pub struct InstallService {
    workspace_provider: WorkspaceProvider,
}

impl InstallService {
    const ISOBIN_CONFIG_FILE_CACHE_NAME: &str = "isobin_cache.v1.json";
    #[allow(unused_variables)]
    pub async fn install(
        &self,
        service_option: ServiceOption,
        install_service_option: InstallServiceOption,
    ) -> Result<()> {
        let isobin_config =
            IsobinConfig::load_from_file(service_option.isobin_config_path()).await?;
        let isobin_config_dir = isobin_config_dir(service_option.isobin_config_path())?;
        let workspace = self
            .workspace_provider
            .base_unique_workspace_dir_from_isobin_config_dir(isobin_config_dir)
            .await?;
        let tmp_workspace = workspace.make_tmp_workspace();
        let isobin_config_file_cache_path = tmp_workspace
            .base_dir()
            .join(Self::ISOBIN_CONFIG_FILE_CACHE_NAME);

        let source_isobin_config = if !install_service_option.force {
            if workspace.base_dir().exists() {
                fs_ext::create_dir_if_not_exists(tmp_workspace.base_dir()).await?;
                copy_dir(
                    workspace.base_dir().clone(),
                    tmp_workspace.base_dir().clone(),
                )
                .await?;
            }
            if isobin_config_file_cache_path.exists() {
                let cache = fs::read(&isobin_config_file_cache_path).await?;
                let isobin_config_cache: IsobinConfig = serde_json::from_slice(&cache)?;
                IsobinConfig::get_need_install_config(
                    &isobin_config,
                    &isobin_config_cache,
                    &tmp_workspace,
                )
                .await?
            } else {
                isobin_config.clone()
            }
        } else {
            isobin_config.clone()
        };

        let mut isobin_config_file_cache =
            fs_ext::open_file_create_if_not_exists(isobin_config_file_cache_path).await?;
        let sirialized_isobin_config = serde_json::to_vec(&isobin_config)?;
        isobin_config_file_cache
            .write_all(&sirialized_isobin_config)
            .await?;

        fs_ext::create_dir_if_not_exists(tmp_workspace.base_dir()).await?;
        let cargo_installer_factory = CargoInstallerFactory::new(tmp_workspace.clone());
        let install_runner_provider = InstallRunnerProvider::default();
        let cargo_runner = install_runner_provider
            .make_cargo_runner(&cargo_installer_factory, source_isobin_config.cargo())
            .await?;
        self.run_each_installs(&workspace, &tmp_workspace, vec![cargo_runner])
            .await
    }

    async fn run_each_installs(
        &self,
        workspace: &Workspace,
        tmp_workspace: &Workspace,
        runners: Vec<Arc<Mutex<dyn InstallRunner>>>,
    ) -> Result<()> {
        let install_runners = runners.clone();
        join_futures!(install_runners
            .into_iter()
            .map(|r| async move { r.lock().await.run_installs().await }))
        .await
        .map_err(InstallServiceError::MultiInstall)?;
        let mut keys = HashSet::new();
        let mut duplicates = vec![];
        let file_name_runners = runners.clone();
        for file_name in join_futures!(file_name_runners
            .into_iter()
            .map(|r| async move { r.lock().await.bin_paths().await }))
        .await
        .map_err(InstallServiceError::MultiInstall)?
        .into_iter()
        .flatten()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
        {
            if !keys.insert(file_name.clone()) {
                duplicates.push(file_name);
            }
        }
        if !duplicates.is_empty() {
            Err(InstallServiceError::new_duplicate_bin(duplicates).into())
        } else {
            let install_bin_path_runners = runners.clone();
            join_futures!(install_bin_path_runners
                .into_iter()
                .map(|r| async move { r.lock().await.install_bin_path().await }))
            .await
            .map_err(InstallServiceError::MultiInstall)?;

            let tmp_dir = workspace.cache_dir().join(nanoid!());
            let need_tmp = workspace.base_dir().exists();
            if need_tmp {
                fs::rename(workspace.base_dir(), &tmp_dir).await?;
            }
            match fs::rename(tmp_workspace.base_dir(), workspace.base_dir()).await {
                Ok(_) => {}
                Err(err) => {
                    if need_tmp {
                        fs::rename(&tmp_dir, workspace.base_dir()).await?;
                    }
                    Err(err)?;
                }
            }
            if need_tmp {
                fs_ext::clean_dir(tmp_dir).await?
            }
            Ok(())
        }
    }
}

#[derive(Getters, new, Clone)]
struct InstallTargetContext<IF: InstallTarget + Clone> {
    target: IF,
    progress: Progress,
}

#[derive(Default)]
pub struct InstallRunnerProvider {
    multi_progress: MultiProgress,
}

impl InstallRunnerProvider {
    pub async fn make_cargo_runner(
        &self,
        cargo_installer: &CargoInstallerFactory,
        cargo_config: &CargoConfig,
    ) -> Result<Arc<Mutex<dyn InstallRunner>>> {
        let install_targets = cargo_config
            .installs()
            .iter()
            .map(|(name, install_dependency)| {
                CargoInstallTarget::new(name.into(), install_dependency.clone())
            })
            .collect::<Vec<_>>();
        self.make_runner(cargo_installer, install_targets).await
    }

    async fn make_runner<IF: providers::InstallerFactory>(
        &self,
        installer_factory: &IF,
        targets: Vec<IF::InstallTarget>,
    ) -> Result<Arc<Mutex<dyn InstallRunner>>> {
        let core_installer = installer_factory.create_core_installer().await?;
        let bin_path_installer = installer_factory.create_bin_path_installer().await?;
        let contexts = targets
            .into_iter()
            .map(|target| {
                let progress = self.multi_progress.make_progress(&target);
                let context = InstallTargetContext::new(target, progress);
                context.progress().prepare()?;
                Ok(context)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Arc::new(Mutex::new(InstallRunnerImpl::new(
            core_installer,
            bin_path_installer,
            contexts,
        ))))
    }
}

#[async_trait]
pub trait InstallRunner: 'static + Sync + Send {
    fn provider_type(&self) -> providers::ProviderKind;
    async fn run_installs(&self) -> Result<()>;
    async fn bin_paths(&self) -> Result<Vec<PathBuf>>;
    async fn install_bin_path(&self) -> Result<()>;
}

#[derive(new)]
struct InstallRunnerImpl<
    IT: providers::InstallTarget,
    CI: providers::CoreInstaller<InstallTarget = IT>,
    BI: providers::BinPathInstaller<InstallTarget = IT>,
> {
    core_installer: CI,
    bin_path_installer: BI,
    contexts: Vec<InstallTargetContext<IT>>,
}

impl<
        IT: providers::InstallTarget,
        CI: providers::CoreInstaller<InstallTarget = IT>,
        BI: providers::BinPathInstaller<InstallTarget = IT>,
    > InstallRunnerImpl<IT, CI, BI>
{
    async fn run_sequential_installs(&self) -> Result<()> {
        for context in self.contexts.iter() {
            Self::install(self.core_installer.clone(), context.clone()).await?;
        }
        Ok(())
    }
    async fn run_parallel_installs(&self) -> Result<()> {
        join_futures!(self
            .contexts
            .iter()
            .map(|target| { Self::install(self.core_installer.clone(), target.clone()) }))
        .await
        .map_err(InstallServiceError::MultiInstall)?;
        Ok(())
    }
    async fn install(core_installer: CI, install_context: InstallTargetContext<IT>) -> Result<()> {
        install_context.progress().start()?;
        match core_installer.install(install_context.target()).await {
            Ok(_) => {
                install_context.progress().done()?;
                Ok(())
            }
            Err(err) => {
                install_context.progress().failed()?;
                Err(err)
            }
        }
    }
}

#[async_trait]
impl<
        IT: providers::InstallTarget,
        CI: providers::CoreInstaller<InstallTarget = IT>,
        BI: providers::BinPathInstaller<InstallTarget = IT>,
    > InstallRunner for InstallRunnerImpl<IT, CI, BI>
{
    fn provider_type(&self) -> providers::ProviderKind {
        self.core_installer.provider_kind()
    }

    async fn run_installs(&self) -> Result<()> {
        match self.core_installer.multi_install_mode() {
            providers::MultiInstallMode::Parallel => self.run_parallel_installs().await,
            providers::MultiInstallMode::Sequential => self.run_sequential_installs().await,
        }
    }
    async fn bin_paths(&self) -> Result<Vec<PathBuf>> {
        let bin_paths = join_futures!(self.contexts.iter().map(|context| {
            let bin_path_installer = self.bin_path_installer.clone();
            let target = context.target().clone();
            async move { bin_path_installer.bin_paths(target).await }
        }))
        .await
        .map_err(InstallServiceError::MultiInstall)?;
        Ok(bin_paths.into_iter().flatten().collect())
    }
    async fn install_bin_path(&self) -> Result<()> {
        join_futures!(self.contexts.iter().map(|context| {
            let bin_path_installer = self.bin_path_installer.clone();
            let target = context.target().clone();
            async move { bin_path_installer.install_bin_path(target).await }
        }))
        .await
        .map_err(InstallServiceError::MultiInstall)?;
        Ok(())
    }
}

#[derive(Getters)]
pub struct InstallServiceOption {
    force: bool,
    mode: InstallMode,
}

#[derive(Default)]
pub struct InstallServiceOptionBuilder {
    force: bool,
    mode: Option<InstallMode>,
}

impl InstallServiceOptionBuilder {
    pub fn mode(mut self, mode: InstallMode) -> Self {
        self.mode = Some(mode);
        self
    }
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
    pub fn build(self) -> InstallServiceOption {
        InstallServiceOption {
            force: self.force,
            mode: self.mode.unwrap_or(InstallMode::All),
        }
    }
}

#[derive(thiserror::Error, Debug, new)]
pub enum InstallServiceError {
    #[error("{0:#?}")]
    MultiInstall(Vec<Error>),

    #[error("{provider}/{name}:\n{error_message}")]
    Install {
        provider: String,
        name: String,
        error_message: String,
        error: Error,
    },

    #[error("duplicate bins:\n{0:#?}")]
    DuplicateBin(Vec<String>),
}
