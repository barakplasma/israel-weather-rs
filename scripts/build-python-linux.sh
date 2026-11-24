#!/usr/bin/env bash
# Run on a disposable BoxD Linux VM (or any Linux Docker host).
set -euo pipefail
arch=${1:?x86_64 or aarch64}
case "$arch" in
  x86_64) platform=linux/amd64 ;;
  aarch64) platform=linux/arm64 ;;
  *) exit 2 ;;
esac
mkdir -p dist
if [[ "$(uname -m)" != "$arch" ]]; then
  docker run --privileged --rm tonistiigi/binfmt --install arm64,amd64
fi
docker run --rm --platform "$platform" -v "$PWD:/io" -w /io \
  "quay.io/pypa/manylinux_2_28_${arch}" bash -euxc '
    dnf install -y perl-IPC-Cmd perl-FindBin perl-File-Compare perl-File-Copy
    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.98 --profile minimal
    export PATH="$HOME/.cargo/bin:$PATH"
    py=/opt/python/cp310-cp310/bin/python
    "$py" -m pip install "maturin>=1.9,<2" twine pytest
    "$py" -m maturin build --release --manifest-path crates/weather-python/Cargo.toml --manylinux 2_28 --interpreter "$py" --out dist
    "$py" -m twine check dist/*.whl
    "$py" -m pip install dist/*.whl
    "$py" -m pip check
    "$py" -m pytest crates/weather-python/tests
    py=/opt/python/cp314-cp314/bin/python
    "$py" -m pip install pytest dist/*.whl
    "$py" -m pip check
    "$py" -m pytest crates/weather-python/tests
  '
