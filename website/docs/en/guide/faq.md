# FAQ

## Installation

### The installer says "No Zygisk implementation found"

Device Faker does **not** support Magisk's built-in Zygisk. You must install a third-party Zygisk implementation (**ZygiskNext** recommended) first, reboot, then install this module.

### Does it support Magisk's built-in Zygisk?

No. The built-in implementation does not meet this module's requirements.

### Does it support 32-bit devices or Android 11 and below?

The module targets **arm64 + API 31 (Android 12)**. 32-bit devices and Android 11 or lower are not supported.

### The module is installed but has no effect

1. Is a Zygisk implementation installed and enabled?
2. Did you **reboot** after installing?
3. Is the config path correct: `/data/adb/device_faker/config/config.toml`?
4. Is the target app's package name spelled correctly?

### Will updating the module delete my config?

Installing a new zip over the existing one preserves your config by default (unless you pressed Volume Up at the prompt to use the default config).

::: warning Uninstalling deletes your config
Uninstalling the module runs `rm -rf /data/adb/device_faker`, which deletes the config directory. Back up your config first if you want to keep it.
:::

## Configuration

### Changes don't take effect

- Config is **hot-reloaded**; after editing, **restart the target app**, not the system
- Check `config.toml` for syntax errors: TOML is sensitive to quotes and indentation; a single missing quote can cause the whole file to fail parsing
- Check that the target app actually matches an `[[apps]]` entry or a template `packages` list

### /data/adb/device_faker/config/config.toml is missing

On first install, if you chose Volume Up to use the default config, the path above is correct. If the directory does not exist, the module may not have finished installing or you did not reboot.

### How does priority work?

```
[[apps]] direct configuration > template packages list > global defaults
```

Once an `[[apps]]` entry matches, it **entirely replaces** the template; unset fields do not fall through.

### How do I target only one user (work profile / multi-account)?

Append `@userId` to the package name:

```toml
[[apps]]
package = "com.example.app@999"
model = "FakeModel"
```

Match order: `@999` is tried first, then falls back to the version without the suffix.

### What happens if the config is wrong?

- An `[[apps]]` entry missing `package`: the whole config fails to parse, no app is spoofed
- An invalid field value: that field is silently ignored (unknown fields are also ignored)
- A broken template: only apps matching that template are affected

## Properties and Performance

### Can `getprop` read the spoofed values?

By default, **no**. Spoofing only applies inside the target app's process, and `getprop` runs as a separate process that still reads the real values.

Set `companion_resetprop = true` for that app if you need `getprop` to see the spoofed values too. Note that this makes the spoofed values system-wide; they are restored automatically when the app exits.

### What's the difference between `__DELETE__` and leaving a field empty?

| Usage | Behavior |
|-------|----------|
| Structured field omitted or `""` | Leave the field unchanged |
| `custom_props` `"key" = ""` | Set the property to an empty string |
| `custom_props` `"key" = "__EMPTY__"` | Set the property to an empty string |
| `custom_props` `"key" = "__DELETE__"` | Delete the property |

### Does it slow down app launch?

Barely. Spoofing happens once during app launch and usually takes milliseconds — you generally won't notice the difference.

## WebUI

### WebUI won't open / blank screen

1. Make sure the module is enabled and you have rebooted
2. Make sure your manager supports WebUI (KernelSU / recent Magisk / APatch)
3. Try clearing the manager's cache or restarting the manager
4. Check the logs: `/data/adb/device_faker/logs/device_faker.log`

### WebUI can't swipe

Make sure your module version is ≥ v1.5.0. v1.5.0 and later improved swipe-back navigation and page transition animations.

### Online template library fails to load

- Check the device's network connection
- Template source: https://github.com/Seyud/device_faker_config
- Accessing GitHub may require a network tool in some regions

## Properties and System

### Does spoofing affect other apps?

No. Spoofing is per-app; only configured apps are modified. Other apps and system processes remain unchanged (unless `companion_resetprop = true` is enabled).

### Does DPI restore when the app goes to the background?

Yes. DPI changes are foreground-gated. About 2 seconds after the target app goes to the background or exits, the companion restores the original override and reapplies it when the app returns to the foreground.

### When should I enable `companion_resetprop`?

Only when you need subprocesses (e.g. `getprop`, `Runtime.exec`) or system services to also read the spoofed values. Most scenarios work fine with the default COW mode.
