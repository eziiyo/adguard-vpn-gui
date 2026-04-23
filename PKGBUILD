# Maintainer: Angelo Pennisi <angelo.pennisi@gmail.com>
pkgname=adguard-vpn-gui
pkgver=0.1.0
pkgrel=1
pkgdesc="Graphical frontend for AdGuard VPN CLI"
arch=('x86_64')
url="https://github.com/eziiyo/adguard-vpn-gui"
license=('MIT')

depends=(
  'adguardvpn-cli'
  'webkit2gtk-4.1'
  'gtk3'
  'libayatana-appindicator'
  'glib2'
  'pango'
  'cairo'
  'gdk-pixbuf2'
  'hicolor-icon-theme'
)

makedepends=(
  'rust'
  'cargo'
  'npm'
  'nodejs'
  'pkg-config'
  'base-devel'
)

source=("$pkgname-$pkgver.tar.gz::https://github.com/eziiyo/adguard-vpn-gui/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$pkgname-$pkgver"
  npm ci
  npm run tauri build -- --bundles none
}

package() {
  cd "$pkgname-$pkgver"

  install -Dm755 "src-tauri/target/release/$pkgname" \
    "$pkgdir/usr/bin/$pkgname"

  install -Dm644 "src-tauri/icons/128x128.png" \
    "$pkgdir/usr/share/icons/hicolor/128x128/apps/$pkgname.png"

  install -Dm644 "src-tauri/icons/32x32.png" \
    "$pkgdir/usr/share/icons/hicolor/32x32/apps/$pkgname.png"

  install -Dm644 /dev/stdin "$pkgdir/usr/share/applications/$pkgname.desktop" <<EOF
[Desktop Entry]
Name=AdGuard VPN
Comment=Graphical frontend for AdGuard VPN CLI
Exec=$pkgname
Icon=$pkgname
Type=Application
Categories=Network;
Keywords=vpn;adguard;privacy;
EOF
}
