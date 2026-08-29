# Advanced Usage

## Property Spoofing Mechanism

All apps go through the same unified flow (no mode selection needed):

```
① JNI overwrite of Build static fields → ② COW or companion resetprop → ③ DPI spoofing → ④ CPU spoofing → ⑤ DlClose unload
```

- **COW (default)**: remaps the property-area file with mmap COW and overwrites property memory in place, covering native reads via `__system_property_get` / `__system_property_read_callback`; no GOT/PLT modification; **only affects the current process's** memory mapping; the module calls DlClose right after writing, leaving zero resident footprint
- **Companion resetprop (`companion_resetprop = true`)**: all properties are written directly to the property area by the companion process (`skip_svc`, bypassing property_service), so **reads are consistent system-wide**; original values are restored automatically ~2 seconds after the app exits or goes to background, and re-applied on return to foreground

## Debugging and Logs

```toml
debug = true
```

When enabled, Info-level logs are output (Error-only when disabled), written to `/data/adb/device_faker/logs/device_faker.log`. Keep it disabled during normal use to avoid leaving unnecessary traces.

## Complete Configuration Example

```toml
# ── Global settings ───────────────────────────────────────
debug = false                        # debug logging (off by default)
default_force_denylist_unmount = false
default_cpu_spoof = "kirin_9030pro"  # global default CPU preset

# ── CPU spoofing presets ──────────────────────────────────
[cpu_presets]
kirin_9030pro = """Processor       : AArch64 Processor rev 0 (aarch64)
Features        : fp asimd evtstrm aes pmull sha1 sha2 crc32
...
Hardware        : HiSilicon Kirin 9030 Pro"""

# ── Device templates ──────────────────────────────────────
[templates.redmagic_9_pro]
packages = [
    "com.mobilelegends.mi",
  # Only applies to userId=999
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
cpu_spoof = "kirin_9030pro"  # CPU spoofing for all packages in this template

# ── Direct configuration ──────────────────────────────────
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
dpi = 420                       # temporary display density (120–640)
force_denylist_unmount = true  # overrides the global default, this app only

[[apps]]
package = "com.example.detected.app"
companion_resetprop = true     # system-wide consistent properties (restored automatically on app exit)
manufacturer = "Custom"

[apps.custom_props]
"ro.custom.property" = "custom_value"
"ro.debug.mode" = "__DELETE__"
```
