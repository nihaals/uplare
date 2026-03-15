use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacOsConfig {
    pub install_homebrew: bool,
    pub apps: Vec<MacOsApp>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum MacOsApp {
    HomebrewCask(HomebrewCaskApp),
    MacAppStoreApp(MacAppStoreApp),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseMacOsApp {
    pub app_paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomebrewCaskApp {
    pub cask_name: String,
    #[serde(flatten)]
    pub base: BaseMacOsApp,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacAppStoreApp {
    pub app_store_id: u64,
    #[serde(flatten)]
    pub base: BaseMacOsApp,
}
