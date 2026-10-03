#!/usr/bin/env bash
# Installs Mesa's lavapipe, a Vulkan driver on the CPU, on a Linux runner without a GPU, so the GPU checks run in
# place of being skipped.
#
# No package in the Ubuntu archive ships a lavapipe driver any more, since noble-updates moved to Mesa 25.2.8, which
# dropped it. This takes the prebuilt Mesa that wgpu's own CI runs on instead. A newer Mesa needs a matching tag from
# https://github.com/gfx-rs/ci-build/releases, and the SHA-256 of its tarball, which the release lists beside the asset.
#
# The driver lands under `$RUNNER_TEMP`, or a temporary directory outside GitHub Actions. The two variables that select
# it are appended to `$GITHUB_ENV` when that is set, and printed as `export` lines otherwise.
set -euo pipefail

mesa_version="26.1.3"
ci_build_tag="build29"
mesa_sha256="63915836cf582bdbf13cf41c75ac335f7ec9c8b4886fe32e76b748ff8c6393c6"

root="${RUNNER_TEMP:-$(mktemp -d)}"

# The Vulkan loader, and `vulkaninfo` for the summary below.
sudo apt-get update
sudo apt-get install -y --no-install-recommends libvulkan1 vulkan-tools

curl -L --retry 5 --fail \
    "https://github.com/gfx-rs/ci-build/releases/download/$ci_build_tag/mesa-$mesa_version-linux-x86_64.tar.xz" \
    -o "$root/mesa.tar.xz"
echo "$mesa_sha256  $root/mesa.tar.xz" | sha256sum --check --quiet -
mkdir -p "$root/mesa"
tar xpf "$root/mesa.tar.xz" -C "$root/mesa"

lib_dir="$root/mesa/lib/x86_64-linux-gnu"
test -f "$lib_dir/libvulkan_lvp.so"

# The bundled driver manifest holds paths from the machine Mesa was built on.
cat > "$root/icd.json" <<JSON
{
  "ICD": {
    "api_version": "1.4.348",
    "library_arch": "64",
    "library_path": "$lib_dir/libvulkan_lvp.so"
  },
  "file_format_version": "1.0.1"
}
JSON

library_path="$lib_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
VK_DRIVER_FILES="$root/icd.json" LD_LIBRARY_PATH="$library_path" vulkaninfo --summary

if [[ -n "${GITHUB_ENV:-}" ]]; then
    echo "VK_DRIVER_FILES=$root/icd.json" >> "$GITHUB_ENV"
    echo "LD_LIBRARY_PATH=$library_path" >> "$GITHUB_ENV"
else
    echo "export VK_DRIVER_FILES='$root/icd.json'"
    echo "export LD_LIBRARY_PATH='$library_path'"
fi
