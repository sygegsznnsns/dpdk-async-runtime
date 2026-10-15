#!/usr/bin/env bash
set -euo pipefail

DPDK_VERSION="22.11.4"
DPDK_SRC_DIR="${HOME}/src/dpdk-stable-${DPDK_VERSION}"
DPDK_INSTALL_PREFIX="/usr/local"
HUGEPAGES="1024"
DPDK_PCI_BDF="${DPDK_PCI_BDF:-0000:28:00.0}"

echo "=== [1/6] Installing system dependencies ==="
if command -v dnf >/dev/null 2>&1; then
    sudo dnf install -y gcc make meson ninja-build numactl-devel \
        kernel-headers kernel-devel pciutils git wget tar xz
elif command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install -y gcc make meson ninja-build libnuma-dev \
        linux-headers-$(uname -r) pciutils git wget tar xz-utils
fi

echo "=== [2/6] Installing Rust toolchain ==="
if ! command -v rustc >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "${HOME}/.cargo/env"
fi
rustup default stable
echo "rustc: $(rustc --version)"

echo "=== [3/6] Building DPDK ${DPDK_VERSION} ==="
if [ ! -d "${DPDK_SRC_DIR}" ]; then
    mkdir -p "$(dirname "${DPDK_SRC_DIR}")"
    cd "$(dirname "${DPDK_SRC_DIR}")"
    wget -q "https://fast.dpdk.org/rel/dpdk-${DPDK_VERSION}.tar.xz"
    tar xf "dpdk-${DPDK_VERSION}.tar.xz"
fi
cd "${DPDK_SRC_DIR}"
if [ ! -d build ]; then
    meson setup build \
        -Dprefix="${DPDK_INSTALL_PREFIX}" \
        -Dlibdir=lib64 \
        -Dexamples= \
        -Dtests=disabled \
        -Ddriverdir="${DPDK_INSTALL_PREFIX}/lib64/dpdk/drivers" \
        -Dmax_lcores=16 \
        -Denable_docs=false
fi
ninja -C build
sudo ninja -C build install
sudo ldconfig
export PKG_CONFIG_PATH="${DPDK_INSTALL_PREFIX}/lib64/pkgconfig"
echo "DPDK: $(pkg-config --modversion libdpdk)"

echo "=== [4/6] Configuring hugepages (${HUGEPAGES} x 2MB) ==="
echo "${HUGEPAGES}" | sudo tee /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages >/dev/null
if ! mount | grep -q hugetlbfs; then
    sudo mkdir -p /mnt/huge
    sudo mount -t hugetlbfs nodev /mnt/huge
fi
echo "Hugepages: $(cat /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages)"

echo "=== [5/6] Loading vfio-pci and binding ${DPDK_PCI_BDF} ==="
sudo modprobe vfio-pci
if [ -f /sys/module/vfio/parameters/enable_unsafe_noiommu_mode ]; then
    echo "1" | sudo tee /sys/module/vfio/parameters/enable_unsafe_noiommu_mode >/dev/null
fi
cd "${DPDK_SRC_DIR}"
sudo ./build/app/dpdk-devbind.py --bind=vfio-pci "${DPDK_PCI_BDF}" || true
echo "DPDK NIC bound to vfio-pci:"
sudo ./build/app/dpdk-devbind.py --status | grep -A2 "${DPDK_PCI_BDF}" || true

echo "=== [6/6] Building project ==="
cd "$(dirname "$0")/.."
export PKG_CONFIG_PATH="${DPDK_INSTALL_PREFIX}/lib64/pkgconfig"
cargo build --release -p dpdk-runtime -p async-ping -p raw-ping

echo ""
echo "=== Setup complete ==="
echo "DPDK PCI: ${DPDK_PCI_BDF}"
echo "Run examples:"
echo "  sudo env LD_LIBRARY_PATH=${DPDK_INSTALL_PREFIX}/lib64 ./target/release/async-ping --delay-us 500 --duration-sec 60 --sessions 64 --payload 64"
echo "  sudo env LD_LIBRARY_PATH=${DPDK_INSTALL_PREFIX}/lib64 ./target/release/raw-ping --delay-us 500 --duration-sec 60 --sessions 64 --payload 64"
