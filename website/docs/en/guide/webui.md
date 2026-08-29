# WebUI

Device Faker provides a modern web-based management interface. Open it directly from the Magisk / KernelSU / APatch module page — no need to hand-edit TOML.

## How to Open

After installing the module, find **Device Faker** in your module manager and tap the "WebUI" or "Settings" entry.

::: info Compatibility
The WebUI is implemented using the pure KernelSU WebUI API (the old WebUI-X API has been removed). It works with the WebUI entry points in KernelSU, Magisk and APatch.
:::

## Page Overview

| Page | Main Features |
|------|---------------|
| **Apps** | View installed apps, toggle system apps, see config status, edit app config |
| **Templates** | Create/edit/delete device templates, apply to multiple packages, import/export, rename |
| **Settings** | Global toggles, module info, language switch |
| **Status** | Runtime overview, log summary, version info |

## App Management

### App List and Filtering

- Lists installed apps and shows whether each one is currently configured
- Toggle "Show system apps" to configure system apps too
- Search by package name or app label

### Multi-user App Display

Multi-user instances of the same app (e.g. `com.foo` and `com.foo@999`) are **grouped by package name**, making it easy to configure them per user.

### Editing App Config

Tap an app to enter the config editor:

- Fill in fields directly (manufacturer, brand, model, device codename, etc.)
- Apply an existing [device template](./configuration/index.md#method-one-device-templates) in one click
- Set toggles such as `companion_resetprop`, `force_denylist_unmount`, `dpi`, `cpu_spoof`
- Add custom properties in the `custom_props` section

### Applying a Template to an App

In the app editor, choose "Apply template" to populate all fields from a template at once, then tweak as needed.

## Template Management

### Create and Edit Templates

- Fill in device fields and save as a named template
- Saved templates can be reused across multiple apps

### Apply to Multiple Packages

When editing a template, add package names to its `packages` list. Those packages will automatically use the template once saved.

### Import / Export

- Export all current templates to a file for backup
- Import a backup file to restore templates

### Online Template Library

The WebUI has a built-in online template source. Import community templates with one click:

- Repository: [device_faker_config](https://github.com/Seyud/device_faker_config)
- Browse by brand/model category
- Search support

::: tip Contribute
If you have a well-organized device config, submit it to `device_faker_config` to help others.
:::

### Rename Existing Templates

Templates can be renamed. After renaming, all app configs that reference the template are automatically updated.

## Gestures and Navigation

- **Swipe back**: swipe from the edge to go back one level in multi-level pages (app → editor → field details)
- **Swipe between tabs**: quickly switch between bottom tabs with left/right swipes

## Multiple Languages

The WebUI supports language switching. Currently available:

- 简体中文
- English
- Türkçe

The UI language follows the system language by default, and can also be switched manually in the WebUI Settings page.

## Next Steps

- [Basic Configuration](./configuration/index.md) — understand the TOML fields behind the WebUI
- [FAQ](./faq.md) — check here first if the WebUI won't open or shows a blank screen
