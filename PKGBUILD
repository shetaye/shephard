# Maintainer: shetaye

pkgname=shephard
pkgver=0.1.0
pkgrel=1
pkgdesc='TUI tool to sync many Git repositories with pull/commit/push workflows'
arch=('x86_64' 'aarch64')
url='https://github.com/shetaye/shephard'
license=('custom:unknown')
depends=('git')
makedepends=('cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
  cd "$srcdir/$pkgname-$pkgver"
  cargo fetch --locked --target "$(rustc -vV | sed -n 's|host: ||p')"
}

build() {
  cd "$srcdir/$pkgname-$pkgver"
  export CARGO_TARGET_DIR=target
  export RUSTUP_TOOLCHAIN=stable
  cargo build --frozen --release --locked
}

check() {
  cd "$srcdir/$pkgname-$pkgver"
  export CARGO_TARGET_DIR=target
  export RUSTUP_TOOLCHAIN=stable
  cargo test --frozen --locked
}

package() {
  cd "$srcdir/$pkgname-$pkgver"
  install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
}
