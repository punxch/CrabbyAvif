# Crabby Avif 🦀

Avif parser/decoder/encoder implementation in Rust.

Feel free to file an issue for any question, suggestion or bug report.
Contributions are also welcome, see [CONTRIBUTING](CONTRIBUTING.md).

## Features

 * Supports aom as the underlying AV1 encoder.
 * Uses the pure Rust rav1d AV1 decoder by default, with external assembly disabled.
 * Supports native dav1d or Android MediaCodec as optional AV1 decoders.
 * C API compatible with [libavif](https://github.com/aomediacodec/libavif)
 * ..and more

## Build

The default configuration is AVIF decoding with **rav1d 1.1.0**, including 8-,
10-, and 12-bit images, alpha, animated sequences, grids, and progressive images.
AVIF parsing, frame timing, metadata, and color conversion remain in CrabbyAvif.
The build enables only rav1d's `bitdepth_8` and `bitdepth_16` features.

With Rust and the platform linker installed (MSVC and the Windows SDK on Windows,
Xcode command-line tools on macOS, or a system linker on Linux), run:

```sh
cargo build --profile release --locked
# The supplied Windows profile inherits release and keeps panic unwinding.
cargo build --profile release-windows --locked
```

`Cargo.lock` is checked in so a fresh checkout can use `--locked`. These are native
builds on each OS; a Cargo profile does not select an OS or install a cross linker.
No dav1d/libyuv installation, NASM, Meson, Ninja, pkg-config, or libclang is needed
by the default build. rav1d still has Rust build dependencies named `cc` and
`nasm-rs`, but does not invoke their native tools when `asm` is disabled.

The portable CI workflow builds and tests on Windows, Linux, and macOS without
running the native dependency setup. Additional architectures need verification;
in particular rav1d 1.1.0 uses unstable Rust feature attributes on ARM32 and RISC-V.
Disabling assembly trades throughput for simpler builds. Optimized builds are
recommended for decoding and tests. Without libyuv, progressive-image scaling uses
a Rust bilinear filter, so scaled pixels need not be bit-identical to libyuv's box
filter. Packed Android pixel formats still require libyuv for scaling.

### Use from another Rust project

```toml
# Replace the path with a pinned Git revision when publishing the consuming app.
crabby-avif = { path = "../CrabbyAvif", default-features = false, features = ["rav1d"] }
```

Select `CodecChoice::Auto` or `CodecChoice::Rav1d`. Frame iteration, seeking,
timing, and RGBA conversion use the existing `decoder::Decoder` API. Looping is
controlled by the caller, which can seek back to frame zero.

The consuming project owns its profile and lockfile. Update its dependency
configuration and lockfile once, then its normal
`cargo build --profile <profile_name> --locked` command needs no extra flags.
A dependency's lockfile and profile definitions do not override those of the app.
Do not enable rav1d's `asm` feature elsewhere in the dependency graph: Cargo
unifies features. `python scripts/check-portable-deps.py` checks this repository's
default dependency graph in CI.

### Optional native configuration

The `native` feature restores the previous default bundle: `dav1d`, `libyuv`,
`png`, `jpeg`, `gif`, and `sharpyuv`. Install or build those libraries as before,
then use:

```sh
cargo build --no-default-features --features native --locked
# Native dav1d decoder only:
cargo build --no-default-features --features dav1d --locked
```

`rav1d` and `dav1d` are mutually exclusive because both export `dav1d_*` symbols.
Combining them, including through `--all-features`, is intentionally rejected.
Use separate builds when comparing the backends. Other optional native features
(such as `libyuv`, `png`, and `aom`) still require their original native toolchains.

For encoder support, install/build aom (`sys/aom-sys/aom.cmd`) and enable `aom`.
The default build is a decoder; it does not include PNG/JPEG/GIF file I/O or the
CLI. Those remain opt-in features.

## Binary

The library and package is named "crabby_avif". The name of the binary tool is
"crabbyavif".

```sh
cargo run --no-default-features --features native,aom,cli -- --help

# Example encode
cargo run --no-default-features --features native,aom,cli -- input.png -q 60 output.avif

# Example decode
cargo run --features cli,png -- input.avif output.png

# Example inspection
cargo run --no-default-features --features cli -- --info input.avif
```

## Tests

```sh
cargo test --profile release-windows --locked -- --skip test_conformance
```

### Conformance Tests

```sh
git clone https://github.com/AOMediaCodec/av1-avif.git external/av1-avif
git clone https://github.com/AOMediaCodec/libavif.git external/libavif
cd external/libavif/ext
./dav1d.cmd
cd ..
cmake -S . -B build -DAVIF_CODEC_DAV1D=LOCAL -DAVIF_LIBYUV=OFF -DAVIF_BUILD_APPS=ON
cmake --build build --parallel -t avifdec
cd ../..
cargo test --profile release-windows --locked -- test_conformance
```

If you already have the `av1-avif` repository checked out and the `avifdec`
binary available, you can point to those by setting the following environment
variables:

``sh
CRABBYAVIF_CONFORMANCE_TEST_DATA_DIR=<path> CRABBYAVIF_CONFORMANCE_TEST_AVIFDEC=<avifdec_binary> cargo test --profile release-windows --locked -- test_conformance
``

### C API Tests

```sh
# Build google test
cd external
./googletest.cmd
cd ..
# Build the library with the C API enabled and a codec
cargo build --no-default-features --features native,aom,capi --release
# Build the C/C++ Tests
cmake -S c_api_tests -B c_build
cmake --build c_build --parallel
# Run the C/C++ Tests (conformance_tests exclusive)
ctest --test-dir c_build --output-on-failure -E conformance_tests
```

### Android Tests

The decoder tests can be run on Android using [dinghy](https://crates.io/crates/cargo-dinghy).

```sh
# One time set up
cargo install cargo-dinghy
# Set path to NDK
export ANDROID_NDK_HOME=<path_to_ndk>
# Install rust toolchain for target
rustup target add aarch64-linux-android
# End of One time set up
# Make sure the device/emulator is available via adb.
cargo dinghy -d android test --no-default-features --features android_mediacodec,libyuv --target aarch64-linux-android --test decoder_tests
```

## License

See the [Apache v2.0 license](LICENSE) file.
