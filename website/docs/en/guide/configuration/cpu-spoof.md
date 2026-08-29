# CPU Spoofing

The companion process bind-mounts fake `/proc/cpuinfo` content into the target app's mount namespace:

- KernelSU performs `setns` 25–100ms after mounting; the module detects it via a timerfd and re-mounts automatically
- Automatically unmounted when the app exits; other apps are unaffected
- Apps without `cpu_spoof` configured proactively clean up `/proc/cpuinfo` mounts that may have leaked into their namespace (overhead is only ~13μs when no spoofing is active)
- Content source priority: `cpu_spoof_custom` > `cpu_spoof` preset name > global `default_cpu_spoof` > lookup in `[cpu_presets]`; if the preset doesn't exist, no CPU spoofing is done

## How to Configure

### Using a preset name `cpu_spoof`

Define the preset content in `[cpu_presets]` (see [Basic Configuration](./index.md#cpu_presets)), then reference the name from a template or `[[apps]]`:

```toml
[cpu_presets]
kirin_9030pro = """Processor       : AArch64 Processor rev 0 (aarch64)
Features        : fp asimd evtstrm aes pmull sha1 sha2 crc32
...
Hardware        : HiSilicon Kirin 9030 Pro"""

[templates.redmagic_9_pro]
packages = ["com.mobilelegends.mi"]
cpu_spoof = "kirin_9030pro"
```

### Writing raw content `cpu_spoof_custom`

If you'd rather not create a preset, put the full content directly in the app config. This takes priority over `cpu_spoof`:

```toml
[[apps]]
package = "com.example.app"
cpu_spoof_custom = """Processor       : AArch64 Processor rev 0 (aarch64)
...
Hardware        : HiSilicon Kirin 9030 Pro"""
```

### Global fallback `default_cpu_spoof`

When neither the template nor the app specifies CPU spoofing, the global default is used:

```toml
default_cpu_spoof = "kirin_9030pro"
```

## Notes

- Write the `/proc/cpuinfo` content in full — some apps parse the entire block, so changing only the `Hardware` line may not be enough
- The preset name must exist in `[cpu_presets]`, otherwise that app gets no CPU spoofing
