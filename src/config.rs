use super::*;
use async_std::io::ReadExt;
use async_std::{fs::File, path::Path};

use crate::errors::{IsobinConfigError, Result};
use providers::cargo::CargoConfig;
use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct IsobinConfig {
    #[serde(default)]
    cargo: CargoConfig,
}

impl IsobinConfig {
    #[allow(dead_code)]
    pub async fn from_path(path: impl AsRef<Path>) -> Result<IsobinConfig> {
        let path = path.as_ref();
        let mut file = File::open(path)
            .await
            .map_err(|e| IsobinConfigError::new_read_isobin_config(e.into()))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .await
            .map_err(|e| IsobinConfigError::new_read_isobin_config(e.into()))?;
        let file_extension = Self::get_file_extension(path)?;
        Self::from_str(&content, file_extension)
    }

    fn get_file_extension(path: impl AsRef<Path>) -> Result<ConfigFileExtensions> {
        let extension = path
            .as_ref()
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or(IsobinConfigError::NothingFileExtension)?;

        const TOML_EXTENSION: &str = "toml";
        const YAML_EXTENSION: &str = "yaml";
        const YML_EXTENSION: &str = "yml";
        match extension {
            TOML_EXTENSION => Ok(ConfigFileExtensions::Toml),
            YML_EXTENSION | YAML_EXTENSION => Ok(ConfigFileExtensions::Yaml),
            _ => Err(IsobinConfigError::new_unknown_file_extension(extension.to_string()).into()),
        }
    }

    fn from_str(s: &str, file_extension: ConfigFileExtensions) -> Result<IsobinConfig> {
        match file_extension {
            ConfigFileExtensions::Toml => Self::from_toml_str(s),
            ConfigFileExtensions::Yaml => todo!(),
        }
    }

    fn from_toml_str(s: &str) -> Result<IsobinConfig> {
        let tool_config: IsobinConfig =
            toml::from_str(s).map_err(|e| IsobinConfigError::new_parse_isobin_config(e.into()))?;
        Ok(tool_config)
    }
}

#[derive(PartialEq, Debug)]
enum ConfigFileExtensions {
    Yaml,
    Toml,
}

#[cfg(test)]
mod tests {
    use super::*;
    use providers::cargo::{CargoInstallDependency, CargoInstallDependencyDetail};

    #[fixture]
    fn cargo_install_dependencies() -> Vec<(String, CargoInstallDependency)> {
        [
            ("comrak", CargoInstallDependency::Simple("1.0".into())),
            ("cargo-make", CargoInstallDependency::Simple("2.0".into())),
        ]
        .into_iter()
        .map(|(name, v)| (name.to_string(), v))
        .collect()
    }

    #[fixture]
    fn tool_config(
        cargo_install_dependencies: Vec<(String, CargoInstallDependency)>,
    ) -> IsobinConfig {
        IsobinConfig {
            cargo: CargoConfig::new(cargo_install_dependencies.into_iter().collect()),
        }
    }

    #[fixture]
    #[allow(clippy::field_reassign_with_default)]
    fn table_cargos() -> Vec<(String, CargoInstallDependency)> {
        let mut cargos = vec![];
        let mut comrak_dependency_detail = CargoInstallDependencyDetail::default();
        comrak_dependency_detail.version = Some("1.0".into());
        comrak_dependency_detail.git = Some("git@github.com:kivikakk/comrak.git".into());
        cargos.push((
            "comrak".to_string(),
            CargoInstallDependency::Detailed(comrak_dependency_detail),
        ));

        let mut cargo_make_dependency_detail = CargoInstallDependencyDetail::default();
        cargo_make_dependency_detail.version = Some("2.0".into());
        cargos.push((
            "cargo-make".to_string(),
            CargoInstallDependency::Detailed(cargo_make_dependency_detail),
        ));
        cargos
    }

    #[fixture]
    fn empty_cargos() -> Vec<(String, CargoInstallDependency)> {
        return vec![];
    }

    #[rstest]
    #[case(tool_config(cargo_install_dependencies()),include_str!("testdata/tool_config_from_str_works/default_load.toml"))]
    #[case(tool_config(table_cargos()),include_str!("testdata/tool_config_from_str_works/description_load.toml"))]
    #[case(tool_config(empty_cargos()),include_str!("testdata/tool_config_from_str_works/empty.toml"))]
    #[case(tool_config(empty_cargos()),include_str!("testdata/tool_config_from_str_works/empty_cargo.toml"))]
    fn tool_config_from_str_works(#[case] expected: IsobinConfig, #[case] config_toml_str: &str) {
        let actual = IsobinConfig::from_toml_str(config_toml_str).unwrap();
        pretty_assertions::assert_eq!(expected, actual);
    }

    #[rstest]
    #[case("foo.yaml", ConfigFileExtensions::Yaml)]
    #[case("foo.yml", ConfigFileExtensions::Yaml)]
    #[case("foo.toml", ConfigFileExtensions::Toml)]
    fn get_config_file_extension_works(#[case] path: &str, #[case] expected: ConfigFileExtensions) {
        let actual = IsobinConfig::get_file_extension(path).unwrap();
        pretty_assertions::assert_eq!(expected, actual);
    }
}