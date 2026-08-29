# CPU 伪装

通过 companion 进程把伪造的 `/proc/cpuinfo` 内容 bind mount 到目标应用的挂载命名空间：

- KernelSU 会在挂载后 25–100ms 再执行 `setns` 切换命名空间，模块通过 timerfd 检测并自动重新挂载
- 应用退出后自动卸载，不影响其他应用
- 未配置 `cpu_spoof` 的应用启动时会主动清理可能泄漏到其命名空间的 `/proc/cpuinfo` 挂载（无伪装活跃时开销仅约 13μs）
- 内容来源优先级：`cpu_spoof_custom` > `cpu_spoof` 预设名 > 全局 `default_cpu_spoof` > 在 `[cpu_presets]` 中查找；预设不存在则不做 CPU 伪装

## 怎么配置

### 用预设名 `cpu_spoof`

在 `[cpu_presets]` 里定义预设内容（见[基础配置](./index.md#cpu_presets)），然后在模板或 `[[apps]]` 中引用名字：

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

### 直接写内容 `cpu_spoof_custom`

不想建预设时，可以把完整内容直接写在应用配置里，优先级高于 `cpu_spoof`：

```toml
[[apps]]
package = "com.example.app"
cpu_spoof_custom = """Processor       : AArch64 Processor rev 0 (aarch64)
...
Hardware        : HiSilicon Kirin 9030 Pro"""
```

### 全局兜底 `default_cpu_spoof`

模板和应用都没指定时，回落到全局默认值：

```toml
default_cpu_spoof = "kirin_9030pro"
```

## 注意事项

- `/proc/cpuinfo` 的内容建议写完整，有些应用会解析整段文本，只改 `Hardware` 一行可能不够
- 预设名必须存在于 `[cpu_presets]`，否则该应用不做 CPU 伪装
