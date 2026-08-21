use std::collections::HashMap;

use anyhow::Result;
use serde::Deserialize;

pub const DPI_MIN: u32 = 120;
pub const DPI_MAX: u32 = 640;

fn valid_dpi(value: Option<u32>) -> Option<u32> {
    value.filter(|dpi| (DPI_MIN..=DPI_MAX).contains(dpi))
}

/// `ro.product.*` 会按属性来源顺序读取不同分区的副本。模板字段必须同步这些副本，
/// 否则检测程序可以从另一个分区读回真机值。
const PRODUCT_PARTITION_PREFIXES: &[&str] = &[
    "odm",
    "vendor",
    "system",
    "system_ext",
    "product",
    "bootimage",
];

/// 新版 Android 还为动态内核模块分区导出独立构建信息。
const BUILD_PARTITION_PREFIXES: &[&str] = &[
    "system",
    "system_ext",
    "product",
    "vendor",
    "odm",
    "bootimage",
    "system_dlkm",
    "vendor_dlkm",
    "odm_dlkm",
];

/// Android 构建指纹格式：brand/product/device:release/id/incremental:type/tags
struct FingerprintParts {
    brand: String,
    product: String,
    device: String,
    release: String,
    build_id: String,
    incremental: String,
    build_type: String,
    tags: String,
}

impl FingerprintParts {
    /// 解析标准格式指纹；字段数不符或存在空字段时返回 None（视为未配置指纹，
    /// 不做任何补全）。
    fn parse(fingerprint: &str) -> Option<Self> {
        let (identity, build) = fingerprint.split_once(':')?;
        let (version, variant) = build.split_once(':')?;

        let mut identity_parts = identity.split('/');
        let brand = identity_parts.next()?;
        let product = identity_parts.next()?;
        let device = identity_parts.next()?;
        if identity_parts.next().is_some() {
            return None;
        }

        let mut version_parts = version.split('/');
        let release = version_parts.next()?;
        let build_id = version_parts.next()?;
        let incremental = version_parts.next()?;
        if version_parts.next().is_some() {
            return None;
        }

        let mut variant_parts = variant.split('/');
        let build_type = variant_parts.next()?;
        let tags = variant_parts.next()?;
        if variant_parts.next().is_some()
            || [
                brand,
                product,
                device,
                release,
                build_id,
                incremental,
                build_type,
                tags,
            ]
            .iter()
            .any(|part| part.is_empty())
        {
            return None;
        }

        Some(Self {
            brand: brand.to_string(),
            product: product.to_string(),
            device: device.to_string(),
            release: release.to_string(),
            build_id: build_id.to_string(),
            incremental: incremental.to_string(),
            build_type: build_type.to_string(),
            tags: tags.to_string(),
        })
    }
}

fn has_profile_value(value: &Option<String>) -> bool {
    value.as_ref().is_some_and(|value| !value.is_empty())
}

fn fill_profile_default(target: &mut Option<String>, value: impl Into<String>) {
    if !has_profile_value(target) {
        *target = Some(value.into());
    }
}

/// 顶层字段的有效伪装值：未设置、空串或 `__DELETE__`（走删除列表）返回 None，
/// `__EMPTY__` 归一化为空字符串。
fn field_value(field: &Option<String>) -> Option<String> {
    match field.as_deref() {
        Some(value) if !value.is_empty() && value != "__DELETE__" => {
            Some(if value == "__EMPTY__" {
                String::new()
            } else {
                value.to_string()
            })
        }
        _ => None,
    }
}

/// 为 `ro.product.<suffix>` 写入全部分区副本（bionic prefix routing 可能读取任一副本）。
fn insert_product_family(map: &mut HashMap<String, String>, suffix: &str, value: &str) {
    map.insert(format!("ro.product.{suffix}"), value.to_string());
    for partition in PRODUCT_PARTITION_PREFIXES {
        map.insert(
            format!("ro.product.{partition}.{suffix}"),
            value.to_string(),
        );
    }
}

/// 为 `ro.build.<suffix>` 写入全部分区副本（含动态内核模块分区）。
fn insert_build_family(map: &mut HashMap<String, String>, suffix: &str, value: &str) {
    map.insert(format!("ro.build.{suffix}"), value.to_string());
    for partition in BUILD_PARTITION_PREFIXES {
        map.insert(format!("ro.{partition}.build.{suffix}"), value.to_string());
    }
}

fn delete_product_family(delete_props: &mut Vec<String>, suffix: &str) {
    delete_props.push(format!("ro.product.{suffix}"));
    for partition in PRODUCT_PARTITION_PREFIXES {
        delete_props.push(format!("ro.product.{partition}.{suffix}"));
    }
}

fn delete_build_family(delete_props: &mut Vec<String>, suffix: &str) {
    delete_props.push(format!("ro.build.{suffix}"));
    for partition in BUILD_PARTITION_PREFIXES {
        delete_props.push(format!("ro.{partition}.build.{suffix}"));
    }
}

/// custom_props 中已知属性族的 key 自动整族展开；未知 key 不展开，
/// 随后的精确 key 写入仍具有最高优先级。
fn expand_known_custom_property(map: &mut HashMap<String, String>, key: &str, value: &str) {
    match key {
        "ro.product.manufacturer" => insert_product_family(map, "manufacturer", value),
        "ro.product.brand" => insert_product_family(map, "brand", value),
        "ro.product.model" => insert_product_family(map, "model", value),
        "ro.product.name" => insert_product_family(map, "name", value),
        "ro.product.device" => insert_product_family(map, "device", value),
        "ro.product.marketname" => insert_product_family(map, "marketname", value),
        "ro.build.fingerprint" => insert_build_family(map, "fingerprint", value),
        "ro.build.id" => insert_build_family(map, "id", value),
        "ro.build.version.release" => insert_build_family(map, "version.release", value),
        "ro.build.version.sdk" => insert_build_family(map, "version.sdk", value),
        "ro.build.version.incremental" => insert_build_family(map, "version.incremental", value),
        "ro.build.type" => insert_build_family(map, "type", value),
        "ro.build.tags" => insert_build_family(map, "tags", value),
        _ => {}
    }
}

/// 机型模板
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceTemplate {
    /// 包名列表
    #[serde(default)]
    pub packages: Vec<String>,
    /// 设备信息
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub brand: Option<String>,
    #[serde(default)]
    pub marketname: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub hardware: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,
    #[serde(default)]
    pub characteristics: Option<String>,
    /// Android 版本伪装（如 "15", "14"）
    #[serde(default)]
    pub android_version: Option<String>,
    /// SDK 版本伪装（如 35, 34）
    #[serde(default)]
    pub sdk_int: Option<u32>,
    /// 临时覆盖系统显示密度（120–640），由 companion 使用 `wm density` 应用
    #[serde(default)]
    pub dpi: Option<u32>,
    /// 自定义属性映射表
    #[serde(default)]
    pub custom_props: Option<HashMap<String, String>>,
    /// 是否为匹配的应用强制执行 FORCE_DENYLIST_UNMOUNT（默认继承全局设置）
    #[serde(default)]
    pub force_denylist_unmount: Option<bool>,
    /// CPU 伪装预设名称（引用 [cpu_presets]）
    #[serde(default)]
    pub cpu_spoof: Option<String>,
    /// 自定义 CPU 伪装内容（优先级高于 cpu_spoof）
    #[serde(default)]
    pub cpu_spoof_custom: Option<String>,
    /// 要从 /proc/self/maps 中清除的属性映射模式列表（默认继承全局设置）
    #[serde(default)]
    pub hide_maps: Option<Vec<String>>,
    /// 是否跳过 COW 属性伪造，所有属性直接交给 companion resetprop 处理
    /// true 时 getprop（独立进程）和进程内读取一致，适用于属性一致性对比检测
    #[serde(default)]
    pub companion_resetprop: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub package: String,
    /// 直接指定设备信息
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub brand: Option<String>,
    #[serde(default)]
    pub marketname: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub hardware: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,
    #[serde(default)]
    pub characteristics: Option<String>,
    /// Android 版本伪装（如 "15", "14"）
    #[serde(default)]
    pub android_version: Option<String>,
    /// SDK 版本伪装（如 35, 34）
    #[serde(default)]
    pub sdk_int: Option<u32>,
    /// 临时覆盖系统显示密度（120–640），由 companion 使用 `wm density` 应用
    #[serde(default)]
    pub dpi: Option<u32>,
    /// 自定义属性映射表
    #[serde(default)]
    pub custom_props: Option<HashMap<String, String>>,
    /// 是否为该应用强制执行 FORCE_DENYLIST_UNMOUNT（默认继承全局设置）
    #[serde(default)]
    pub force_denylist_unmount: Option<bool>,
    /// CPU 伪装预设名称（引用 [cpu_presets]）
    #[serde(default)]
    pub cpu_spoof: Option<String>,
    /// 自定义 CPU 伪装内容（优先级高于 cpu_spoof）
    #[serde(default)]
    pub cpu_spoof_custom: Option<String>,
    /// 要从 /proc/self/maps 中清除的属性映射模式列表（默认继承全局设置）
    #[serde(default)]
    pub hide_maps: Option<Vec<String>>,
    /// 是否跳过 COW 属性伪造，所有属性直接交给 companion resetprop 处理
    /// true 时 getprop（独立进程）和进程内读取一致，适用于属性一致性对比检测
    #[serde(default)]
    pub companion_resetprop: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    /// 是否默认启用 FORCE_DENYLIST_UNMOUNT（避免模块挂载痕迹）
    #[serde(default)]
    pub default_force_denylist_unmount: bool,
    /// 是否启用调试日志（默认关闭以提高隐蔽性）
    #[serde(default)]
    pub debug: bool,
    /// 机型设备模板定义
    #[serde(default)]
    pub templates: HashMap<String, DeviceTemplate>,
    /// 应用配置
    #[serde(default)]
    pub apps: Vec<AppConfig>,
    /// 全局默认 CPU 伪装预设名称
    #[serde(default)]
    pub default_cpu_spoof: Option<String>,
    /// CPU 伪装预设表
    #[serde(default)]
    pub cpu_presets: HashMap<String, String>,
    /// 要从 /proc/self/maps 中清除的属性映射模式列表（默认不启用）
    #[serde(default)]
    pub default_hide_maps: Vec<String>,
}

impl Config {
    pub fn from_toml(content: &str) -> Result<Self> {
        Ok(toml::from_str(content)?)
    }

    /// 查找包名对应的应用配置（优先）或模板配置
    pub fn get_app_config(&self, package_name: &str) -> Option<&AppConfig> {
        self.apps.iter().find(|app| app.package == package_name)
    }

    /// 查找包名对应的模板（从模板的 packages 列表中查找）
    pub fn find_template_for_package(&self, package_name: &str) -> Option<&DeviceTemplate> {
        self.templates
            .values()
            .find(|template| template.packages.iter().any(|pkg| pkg == package_name))
    }

    /// 获取应用的最终配置（优先查找直接配置，其次查找模板的 packages 列表）
    pub fn get_merged_config(&self, package_name: &str) -> Option<MergedAppConfig> {
        // 优先查找直接配置的应用
        if let Some(app) = self.get_app_config(package_name) {
            let mut merged = MergedAppConfig {
                manufacturer: app.manufacturer.clone(),
                brand: app.brand.clone(),
                marketname: app.marketname.clone(),
                model: app.model.clone(),
                name: app.name.clone(),
                device: app.device.clone(),
                product: app.product.clone(),
                hardware: app.hardware.clone(),
                fingerprint: app.fingerprint.clone(),
                build_id: app.build_id.clone(),
                characteristics: app.characteristics.clone(),
                android_version: app.android_version.clone(),
                sdk_int: app.sdk_int,
                dpi: valid_dpi(app.dpi),
                custom_props: app.custom_props.clone(),
                force_denylist_unmount: app
                    .force_denylist_unmount
                    .unwrap_or(self.default_force_denylist_unmount),
                cpu_spoof: app.cpu_spoof.clone(),
                cpu_spoof_custom: app.cpu_spoof_custom.clone(),
                cpuinfo_content: None,
                hide_maps: app
                    .hide_maps
                    .clone()
                    .unwrap_or_else(|| self.default_hide_maps.clone()),
                companion_resetprop: app.companion_resetprop.unwrap_or(false),
            };
            merged.complete_device_profile();
            merged.cpuinfo_content = merged.resolve_cpuinfo(self);
            return Some(merged);
        }

        // 如果没有直接配置，查找模板的 packages 列表
        if let Some(template) = self.find_template_for_package(package_name) {
            let mut merged = MergedAppConfig {
                manufacturer: template.manufacturer.clone(),
                brand: template.brand.clone(),
                marketname: template.marketname.clone(),
                model: template.model.clone(),
                name: template.name.clone(),
                device: template.device.clone(),
                product: template.product.clone(),
                hardware: template.hardware.clone(),
                fingerprint: template.fingerprint.clone(),
                build_id: template.build_id.clone(),
                characteristics: template.characteristics.clone(),
                android_version: template.android_version.clone(),
                sdk_int: template.sdk_int,
                dpi: valid_dpi(template.dpi),
                custom_props: template.custom_props.clone(),
                force_denylist_unmount: template
                    .force_denylist_unmount
                    .unwrap_or(self.default_force_denylist_unmount),
                cpu_spoof: template.cpu_spoof.clone(),
                cpu_spoof_custom: template.cpu_spoof_custom.clone(),
                cpuinfo_content: None,
                hide_maps: template
                    .hide_maps
                    .clone()
                    .unwrap_or_else(|| self.default_hide_maps.clone()),
                companion_resetprop: template.companion_resetprop.unwrap_or(false),
            };
            merged.complete_device_profile();
            merged.cpuinfo_content = merged.resolve_cpuinfo(self);
            return Some(merged);
        }

        None
    }

    /// 构建合并配置的系统属性映射
    /// 空字符串会被忽略，不会添加到映射中
    /// __DELETE__ 标记的属性会被记录到 delete_props 中
    pub fn build_merged_property_map(merged: &MergedAppConfig) -> HashMap<String, String> {
        let mut map = HashMap::new();

        // 分区副本同步：bionic prefix routing 可能从任一分区的 prop_area 读取，
        // 检测程序也可以直接指定分区变体读回真机值，必须整族写入。
        if let Some(manufacturer) = field_value(&merged.manufacturer) {
            insert_product_family(&mut map, "manufacturer", &manufacturer);
        }
        if let Some(brand) = field_value(&merged.brand) {
            insert_product_family(&mut map, "brand", &brand);
        }
        if let Some(marketname) = field_value(&merged.marketname) {
            insert_product_family(&mut map, "marketname", &marketname);
            // OnePlus/OPPO 设备读 ro.vendor.oplus.market.name 而非 ro.product.marketname
            map.insert(
                "ro.vendor.oplus.market.name".to_string(),
                marketname.clone(),
            );
        }
        if let Some(model) = field_value(&merged.model) {
            insert_product_family(&mut map, "model", &model);
        }

        // Build.PRODUCT 的属性来源是 ro.product.name；product 与 name 已在
        // complete_device_profile 中同步为同一个产品代号，这里取非空值写入。
        if let Some(product) = field_value(&merged.product).or_else(|| field_value(&merged.name)) {
            insert_product_family(&mut map, "name", &product);
            map.insert("ro.build.product".to_string(), product.clone());
        }

        if let Some(device) = field_value(&merged.device) {
            insert_product_family(&mut map, "device", &device);
        }

        if let Some(hardware) = field_value(&merged.hardware) {
            map.insert("ro.hardware".to_string(), hardware);
        }

        if let Some(fingerprint) = field_value(&merged.fingerprint) {
            insert_build_family(&mut map, "fingerprint", &fingerprint);
        }

        if let Some(build_id) = field_value(&merged.build_id) {
            insert_build_family(&mut map, "id", &build_id);
        }

        if let Some(characteristics) = field_value(&merged.characteristics) {
            map.insert(
                "ro.build.characteristics".to_string(),
                characteristics.clone(),
            );
        }

        // Android 版本伪装属性
        if let Some(android_version) = field_value(&merged.android_version) {
            insert_build_family(&mut map, "version.release", &android_version);
        }

        if let Some(sdk_int) = merged.sdk_int {
            let sdk_str = sdk_int.to_string();
            insert_build_family(&mut map, "version.sdk", &sdk_str);
        }

        // 自定义属性：已知属性族先整族展开，精确 key 随后覆盖（优先级最高）
        if let Some(custom_props) = &merged.custom_props {
            for (key, value) in custom_props {
                if value == "__DELETE__" {
                    continue;
                }
                let final_value = if value == "__EMPTY__" { "" } else { value };
                expand_known_custom_property(&mut map, key, final_value);
            }
            for (key, value) in custom_props {
                if value == "__DELETE__" {
                    continue;
                }
                let final_value = if value == "__EMPTY__" { "" } else { value };
                map.insert(key.clone(), final_value.to_string());
            }
        }

        map
    }

    /// 构建需要删除的属性列表（用于 companion 模式）。
    /// 与 build_merged_property_map 的属性族展开保持对称：
    /// `__DELETE__` 删除整个属性族（含全部分区副本）。
    pub fn build_delete_props_list(merged: &MergedAppConfig) -> Vec<String> {
        let mut delete_props = Vec::new();

        if merged.brand.as_ref().is_some_and(|s| s == "__DELETE__") {
            delete_product_family(&mut delete_props, "brand");
        }
        if merged
            .manufacturer
            .as_ref()
            .is_some_and(|s| s == "__DELETE__")
        {
            delete_product_family(&mut delete_props, "manufacturer");
        }
        if merged.model.as_ref().is_some_and(|s| s == "__DELETE__") {
            delete_product_family(&mut delete_props, "model");
        }
        if merged.name.as_ref().is_some_and(|s| s == "__DELETE__") {
            delete_product_family(&mut delete_props, "name");
            delete_props.push("ro.build.product".to_string());
        }
        if merged.device.as_ref().is_some_and(|s| s == "__DELETE__") {
            delete_product_family(&mut delete_props, "device");
        }
        if merged
            .marketname
            .as_ref()
            .is_some_and(|s| s == "__DELETE__")
        {
            delete_product_family(&mut delete_props, "marketname");
            delete_props.push("ro.vendor.oplus.market.name".to_string());
        }
        if merged
            .fingerprint
            .as_ref()
            .is_some_and(|s| s == "__DELETE__")
        {
            delete_build_family(&mut delete_props, "fingerprint");
        }
        if merged.build_id.as_ref().is_some_and(|s| s == "__DELETE__") {
            delete_build_family(&mut delete_props, "id");
        }
        if merged
            .characteristics
            .as_ref()
            .is_some_and(|s| s == "__DELETE__")
        {
            delete_props.push("ro.build.characteristics".to_string());
        }
        if merged.hardware.as_ref().is_some_and(|s| s == "__DELETE__") {
            delete_props.push("ro.hardware".to_string());
        }

        if let Some(custom_props) = &merged.custom_props {
            for (key, value) in custom_props {
                if value == "__DELETE__" {
                    delete_props.push(key.clone());
                }
            }
        }

        delete_props
    }
}

/// 合并后的应用配置（模板 + 直接配置）
#[derive(Debug, Clone)]
pub struct MergedAppConfig {
    pub manufacturer: Option<String>,
    pub brand: Option<String>,
    pub marketname: Option<String>,
    pub model: Option<String>,
    pub name: Option<String>,
    pub device: Option<String>,
    pub product: Option<String>,
    pub hardware: Option<String>,
    pub fingerprint: Option<String>,
    pub build_id: Option<String>,
    pub characteristics: Option<String>,
    pub android_version: Option<String>,
    pub sdk_int: Option<u32>,
    /// 最终显示密度覆盖值，超出 120–640 的配置会被忽略
    pub dpi: Option<u32>,
    pub custom_props: Option<HashMap<String, String>>,
    pub force_denylist_unmount: bool,
    /// CPU 伪装预设名称
    pub cpu_spoof: Option<String>,
    /// 自定义 CPU 伪装内容
    pub cpu_spoof_custom: Option<String>,
    /// 最终要挂载到 /proc/cpuinfo 的内容（已解析完成）
    pub cpuinfo_content: Option<String>,
    /// 要从 /proc/self/maps 中清除的属性映射模式列表
    pub hide_maps: Vec<String>,
    /// 是否跳过 COW，所有属性走 companion resetprop（默认 false）
    pub companion_resetprop: bool,
}

impl MergedAppConfig {
    /// 用构建指纹补齐未显式填写的标准字段，并同步指纹内携带的构建信息。
    ///
    /// 显式模板字段始终优先；指纹只负责补空，不会覆盖用户主动设置的
    /// 品牌、设备代号等。指纹携带的 incremental/type/tags 以 custom_props
    /// 条目形式补入（不覆盖用户显式配置），随后按属性族展开生效。
    fn complete_device_profile(&mut self) {
        let parsed = self
            .fingerprint
            .as_deref()
            .and_then(FingerprintParts::parse);

        if let Some(parts) = &parsed {
            fill_profile_default(&mut self.brand, parts.brand.clone());
            fill_profile_default(&mut self.product, parts.product.clone());
            fill_profile_default(&mut self.device, parts.device.clone());
            fill_profile_default(&mut self.android_version, parts.release.clone());
            fill_profile_default(&mut self.build_id, parts.build_id.clone());
        }

        // Build.PRODUCT 的属性来源是 ro.product.name，而本项目的 product 与
        // name 语义相同（产品代号）。两者必须同步，否则 JNI 字段与系统属性
        // 会分别显示新旧值。
        if !has_profile_value(&self.product)
            && let Some(name) = self.name.as_ref().filter(|value| !value.is_empty())
        {
            self.product = Some(name.clone());
        }
        if !has_profile_value(&self.name)
            && let Some(product) = self.product.as_ref().filter(|value| !value.is_empty())
        {
            self.name = Some(product.clone());
        }
        if !has_profile_value(&self.device)
            && let Some(product) = self.product.as_ref().filter(|value| !value.is_empty())
        {
            self.device = Some(product.clone());
        }

        if let Some(parts) = parsed {
            let props = self.custom_props.get_or_insert_with(HashMap::new);
            props
                .entry("ro.build.version.incremental".to_string())
                .or_insert(parts.incremental);
            props
                .entry("ro.build.type".to_string())
                .or_insert(parts.build_type);
            props
                .entry("ro.build.tags".to_string())
                .or_insert(parts.tags);
        }
    }

    /// 计算最终 CPU 伪装内容
    pub fn resolve_cpuinfo(&self, config: &Config) -> Option<String> {
        if let Some(custom) = &self.cpu_spoof_custom
            && !custom.is_empty()
        {
            return Some(custom.clone());
        }

        let preset_name = self
            .cpu_spoof
            .as_ref()
            .or(config.default_cpu_spoof.as_ref())?;

        config.cpu_presets.get(preset_name).cloned()
    }
}
