# 高级用法

## 属性伪造机制

所有应用统一走同一执行流（无需选择模式）：

```
① JNI 覆写 Build 静态字段 → ② COW 或 companion resetprop → ③ DPI 伪装 → ④ CPU 伪装 → ⑤ DlClose 卸载模块
```

- **COW（默认）**：通过 mmap COW 重映射属性区文件，直接覆写属性内存，覆盖 `__system_property_get` / `__system_property_read_callback` 的 native 读取；无 GOT/PLT 修改；**只影响当前进程**的内存映射；模块写完立即 DlClose，零驻留
- **companion resetprop（`companion_resetprop = true`）**：全部属性经 companion 进程直写属性区（`skip_svc`，绕过 property_service），**全系统读取一致**；应用退出或退后台约 2 秒后自动恢复原始值，回到前台重新应用

## 调试与日志

```toml
debug = true
```

启用后输出 Info 级别日志（关闭时仅 Error），写入 `/data/adb/device_faker/logs/device_faker.log`。正常使用建议保持关闭，以免留下不必要的运行痕迹。

## 完整配置示例

```toml
# ── 全局设置 ──────────────────────────────────────────────
debug = false                        # 调试日志（默认关闭）
default_force_denylist_unmount = false
default_cpu_spoof = "kirin_9030pro"  # 全局默认 CPU 预设

# ── CPU 伪装预设表 ────────────────────────────────────────
[cpu_presets]
kirin_9030pro = """Processor       : AArch64 Processor rev 0 (aarch64)
Features        : fp asimd evtstrm aes pmull sha1 sha2 crc32
...
Hardware        : HiSilicon Kirin 9030 Pro"""

# ── 机型模板 ──────────────────────────────────────────────
[templates.redmagic_9_pro]
packages = [
    "com.mobilelegends.mi",
  # 仅对 userId=999 生效
  # "com.mobilelegends.mi@999",
    "com.supercell.brawlstars",
]
manufacturer = "Nubia"
brand = "REDMAGIC"
model = "NX809J"
device = "REDMAGIC 11 PRO"
product = "NX809J"
fingerprint = "REDMAGIC/NX809J-UN/NX809J:16/BP2A.250605.031.A3/20251017.000000:user/release-keys"
build_id = "BP2A.250605.031.A3"
cpu_spoof = "kirin_9030pro"  # 模板内所有包名启用 CPU 伪装

# ── 直接配置 ──────────────────────────────────────────────
[[apps]]
package = "com.omarea.vtools"
manufacturer = "Xiaomi"
brand = "Xiaomi"
model = "2509FPN0BC"
device = "Xiaomi 15 Pro"
product = "popsicle"
name = "popsicle"
android_version = "15"
sdk_int = 35
dpi = 420                       # 临时屏幕密度（120–640）
force_denylist_unmount = true  # 覆盖全局默认，仅对该应用启用

[[apps]]
package = "com.example.detected.app"
companion_resetprop = true     # 全系统属性一致（应用退出后自动恢复）
manufacturer = "Custom"

[apps.custom_props]
"ro.custom.property" = "custom_value"
"ro.debug.mode" = "__DELETE__"
```
