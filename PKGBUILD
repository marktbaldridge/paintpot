# Maintainer: Mark Baldridge <marktbaldridge@gmail.com>
pkgname=paintpot
pkgver=0.1.2
pkgrel=1
pkgdesc="A fast, friendly, pixel-perfect paint program in the spirit of classic MS Paint"
arch=('x86_64')
url="https://github.com/marktbaldridge/paintpot"
license=('MIT' 'Apache-2.0')
depends=('gcc-libs' 'glibc' 'libxkbcommon' 'libglvnd' 'hicolor-icon-theme')
optdepends=(
    'wayland: Wayland display backend'
    'libx11: X11 display backend'
    'libxcursor: X11 cursor theme support'
    'libxrandr: X11 multi-monitor support'
    'libxi: X11 input support'
    'mesa: OpenGL/EGL driver (or an equivalent GPU driver)'
    'xdg-desktop-portal: native file open/save dialogs'
)
makedepends=('cargo' 'librsvg')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('2f64dec045ab4f6a52cd277ea544c4a4c51848f890a4d9aae93fb830ae23a06a')

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

    install -Dm644 "assets/$pkgname.svg" \
        "$pkgdir/usr/share/icons/hicolor/scalable/apps/$pkgname.svg"
    for size in 16 22 24 32 48 64 128 256; do
        install -d "$pkgdir/usr/share/icons/hicolor/${size}x${size}/apps"
        rsvg-convert -w "$size" -h "$size" "assets/$pkgname.svg" \
            -o "$pkgdir/usr/share/icons/hicolor/${size}x${size}/apps/$pkgname.png"
    done
}
