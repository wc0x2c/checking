windows: cross-image
    # cargo clean
    # This recipe uses the custom image and not the stock image that Cross.toml
    # names. Cargo caches host build scripts in ./target. A build script that
    # the Arch image compiles links against a newer glibc, so it cannot run in
    # the stock image. With the stock image and these cached build scripts,
    # `ring` fails with "GLIBC_2.28 not found".
    CROSS_TARGET_X86_64_PC_WINDOWS_GNU_IMAGE=checking-cross-win-ai:latest \
        cross build --release --target x86_64-pc-windows-gnu --bins
deploy: windows
    cp target/x86_64-pc-windows-gnu/release/checking.exe /mnt/spacefolder_void/

# Build the custom cross image (Arch mingw with Windows 10 headers) one time.
cross-image:
    docker build -f docker/cross-windows-ai.Dockerfile -t checking-cross-win-ai:latest docker/

build:
    cargo build --release

test *args: build
    cargo nextest run --release {{args}}

# The same as `test`. The workspace has no slow tests yet.
quick-test *args: build
    cargo nextest run --release {{args}}

# The iRacing sender compiles only for Windows, so lint that target too.
lint:
    cargo fmt
    cargo clippy --all-targets -- -D warnings
    cargo clippy --target {{windows_target}} --all-targets -- -D warnings

# Cargo publishes each crate after the crates it needs. Give `--dry-run` to
# check without upload.
# Publish every crate of the workspace to crates.io.
publish *args:
    cargo publish --workspace {{args}}

windows_target := "x86_64-pc-windows-gnu"

# This recipe uses the image that Cross.toml names. If the stock image fails
# with "GLIBC_2.28 not found", set CROSS_TARGET_X86_64_PC_WINDOWS_GNU_IMAGE as
# the `windows` recipe does.
# Build the Windows binary that a release attaches, in target/x86_64-pc-windows-gnu/dist/.
dist:
    cross build --locked --profile dist --target {{windows_target}} --bin checking

# The zip holds the Windows binary, the README and the licenses.
# Put the assets of a release in target/release-assets/. Give the tag, as `v0.1.0`.
release-assets tag: dist
    #!/usr/bin/env bash
    set -euo pipefail
    out=target/release-assets
    rm -rf "$out"
    mkdir -p "$out"
    zip --junk-paths "$out/checking-{{tag}}-{{windows_target}}.zip" \
        "target/{{windows_target}}/dist/checking.exe" \
        README.md LICENSE-APACHE LICENSE-MIT
    ls "$out"
