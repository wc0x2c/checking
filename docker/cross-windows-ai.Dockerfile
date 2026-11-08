# Custom `cross` image for the Windows build.
#
# The stock ghcr.io/cross-rs/x86_64-pc-windows-gnu image has mingw-w64 from
# 2018 (GCC 7.3). This image uses the Arch mingw-w64-gcc 16.x package, which
# has Windows 10 headers. The `windows` just recipe uses this image, because
# the host build scripts cached in ./target need its glibc version.
#
# This image only builds the Windows exe and does not run it, so it does not
# need wine.
FROM archlinux:latest

RUN pacman -Sy --noconfirm \
        mingw-w64-gcc \
        mingw-w64-headers \
        mingw-w64-crt \
        mingw-w64-winpthreads \
        cmake \
        make \
        clang \
        which \
    && pacman -Scc --noconfirm

# cross gets the target linker from these variables. They point at the mingw
# compiler driver.
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
    CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++ \
    AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
