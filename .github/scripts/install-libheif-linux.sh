#!/usr/bin/env bash
# Build libheif from upstream: Ubuntu LTS apt only ships 1.17.x, but
# libheif-sys (via void-whatsapp) requires >= 1.21 for the v1_21 API feature.
set -euo pipefail

VERSION="${LIBHEIF_VERSION:-1.23.1}"
PREFIX="${LIBHEIF_PREFIX:-/usr/local}"

sudo apt-get update
sudo apt-get install -y \
  cmake \
  ninja-build \
  pkg-config \
  libde265-dev \
  libaom-dev \
  libjpeg-turbo8-dev

curl -fsSL \
  "https://github.com/strukturag/libheif/releases/download/v${VERSION}/libheif-${VERSION}.tar.gz" \
  -o /tmp/libheif.tar.gz
tar -xzf /tmp/libheif.tar.gz -C /tmp

cmake -S "/tmp/libheif-${VERSION}" -B /tmp/libheif-build -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="${PREFIX}" \
  -DBUILD_SHARED_LIBS=ON \
  -DBUILD_TESTING=OFF \
  -DWITH_EXAMPLES=OFF \
  -DWITH_GDK_PIXBUF=OFF \
  -DENABLE_PLUGIN_LOADING=OFF
cmake --build /tmp/libheif-build
sudo cmake --install /tmp/libheif-build
sudo ldconfig

# Ensure cargo's pkg-config probe finds the freshly installed .pc
{
  echo "PKG_CONFIG_PATH=${PREFIX}/lib/pkgconfig${PKG_CONFIG_PATH:+:${PKG_CONFIG_PATH}}"
  echo "LD_LIBRARY_PATH=${PREFIX}/lib${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
} >> "${GITHUB_ENV}"

pkg-config --modversion libheif
pkg-config --exists --print-errors "libheif >= 1.21"
