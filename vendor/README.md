# Vendored dependencies

These sources are committed directly so building f0lock needs no additional
repositories, submodules, or dependency preparation script. Cargo downloads the
remaining registry dependencies normally.

| Directory | Upstream | Revision | License |
| --- | --- | --- | --- |
| `nice-plug` | https://codeberg.org/RustAudio/nice-plug | `7197104c0d5c1a65be0cf53055efa7278f5df887` | ISC |
| `pitch-estimate` | https://github.com/hey-jj/pitch-estimate | `62be8786694cfedad287d119af3207aa14ddbc9f` | MIT |

The nice-plug sources already include the changes recorded in
[`nice-plug-ui-dpi.patch`](../docs/patches/nice-plug-ui-dpi.patch). Do not apply this
patch again. Keep these fixes when updating upstream sources.

`baseview` and `egui-baseview` are also patched local sources. Their individual
license files and upstream metadata are retained in their directories.

`pitch-estimate` additionally exposes allocation-free MPM candidate enumeration
and YIN difference/CMNDF curves. Its original `detect()` paths are unchanged.
These APIs support f0lock's [streaming pitch detection](../docs/pitch-detection.md).
