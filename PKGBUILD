# Maintainer: Mark Baldridge <marktbaldridge@gmail.com>
pkgname=paintpot
pkgver=0.1.1
pkgrel=1
pkgdesc="A fast, friendly, pixel-perfect paint program in the spirit of classic MS Paint"
arch=('x86_64')
url="https://github.com/marktbaldridge/paintpot"
license=('MIT' 'Apache-2.0')
depends=('gcc-libs' 'glibc' 'libxkbcommon' 'libglvnd')
optdepends=(
    'wayland: Wayland display backend'
    'libx11: X11 display backend'
    'libxcursor: X11 cursor theme support'
    'libxrandr: X11 multi-monitor support'
    'libxi: X11 input support'
    'mesa: OpenGL/EGL driver (or an equivalent GPU driver)'
    'xdg-desktop-portal: native file open/save dialogs'
)
makedepends=('cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('5634ee1f8b5be68a975bd027ebc6ec5df39f5c592304b618b00751f1be9946bf')

prepare() {
    cd "$pkgname-$pkgver"
    cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    cargo build --frozen --release
}

check() {
    cd "$pkgname-$pkgver"
    cargo test --frozen --release
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
    install -Dm644 "$pkgname.desktop" "$pkgdir/usr/share/applications/$pkgname.desktop"
}
