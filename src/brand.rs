//! 构建时从项目根目录的 branding.json 读取的用户可见名称与版本。
pub const DISPLAY_NAME: &str = env!("GUA_BRAND_DISPLAY_NAME");
pub const PACKAGE_NAME: &str = env!("GUA_BRAND_PACKAGE_NAME");
pub const VERSION: &str = env!("GUA_BRAND_VERSION");
