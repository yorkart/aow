static CATALOG: aow_i18n_core::Catalog = aow_i18n_core::Catalog::new(
    include_str!("../locales/en-US.json"),
    include_str!("../locales/zh-CN.json"),
);
pub fn text(key: &str) -> &'static str {
    CATALOG.text(key)
}
pub fn message(key: &str, parameters: &[(&str, String)]) -> String {
    CATALOG.message(key, parameters)
}
