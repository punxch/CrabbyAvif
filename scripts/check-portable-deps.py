"""Fail CI if the default build pulls in native AVIF tools or rav1d assembly."""

import subprocess

output = subprocess.check_output(
    ["cargo", "tree", "--locked", "--package", "crabby-avif", "-e", "normal,build",
     "--prefix", "none", "--format", "{p} {f}"],
    text=True,
)
forbidden = {"dav1d-sys", "libyuv-sys", "libpng-sys", "libsharpyuv-sys",
             "aom-sys", "avm-sys", "libjxl-sys", "bindgen", "cbindgen", "clang-sys"}
seen_rav1d = False
for line in output.splitlines():
    parts = line.split()
    if not parts:
        continue
    if parts[0] in forbidden:
        raise SystemExit(f"Unexpected default dependency: {line}")
    if parts[0] == "rav1d":
        seen_rav1d = True
        features = set(parts[2].split(",")) if len(parts) > 2 else set()
        if features != {"bitdepth_8", "bitdepth_16"}:
            raise SystemExit(f"Unexpected rav1d features: {line}")
if not seen_rav1d:
    raise SystemExit("The default graph does not contain rav1d")
print("Default graph: rav1d with both bit depths, no assembly or native AVIF dependencies.")
