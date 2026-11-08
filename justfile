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
