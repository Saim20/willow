# Maintainer: Saim <saim20 at github dot com>
pkgname=willow
pkgver=3.0.0
pkgrel=4
pkgdesc="Simple offline configurable voice assistant for gnome"
arch=('x86_64')
url="https://github.com/Saim20/willow"
license=('MIT')
depends=(
    'gnome-shell>=45'
    'libpulse'
    'ydotool'
    'curl'
)
makedepends=(
    'rust'
    'cargo'
    'glib2'
)
optdepends=(
    'speech-dispatcher: TTS feedback via spd-say'
    'espeak: TTS fallback when speech-dispatcher is unavailable'
    'python-sentencepiece: encode custom KWS keywords at install/runtime'
)
options=('!debug')
install=willow.install
source=("willow::git+https://github.com/Saim20/willow.git#tag=v3.0.0")
sha256sums=('SKIP')

build() {
    cd "$srcdir/$pkgname/service-rs"
    cargo build --release
}

check() {
    cd "$srcdir/$pkgname/service-rs"

    if [ ! -x "target/release/willow-service" ]; then
        printf "ERROR: willow-service binary not found or not executable!\n"
        return 1
    fi

    printf "✓ Binary checks passed\n"
}

package() {
    cd "$srcdir/$pkgname"

    install -Dm755 service-rs/target/release/willow-service \
        "$pkgdir/usr/bin/willow-service"

    dbus_service="$(mktemp)"
    sed 's|@CMAKE_INSTALL_PREFIX@|/usr|g' dbus/com.github.saim.Willow.service.in >"$dbus_service"
    install -Dm644 "$dbus_service" \
        "$pkgdir/usr/share/dbus-1/services/com.github.saim.Willow.service"
    rm -f "$dbus_service"

    install -Dm644 dbus/com.github.saim.Willow.xml \
        "$pkgdir/usr/share/dbus-1/interfaces/com.github.saim.Willow.xml"

    install -Dm644 systemd/willow.service \
        "$pkgdir/usr/lib/systemd/user/willow.service"

    local ext_dir="$pkgdir/usr/share/gnome-shell/extensions/willow@saim"
    mkdir -p "$ext_dir"
    cp -r gnome-extension/willow@saim/* "$ext_dir/"

    glib-compile-schemas "$ext_dir/schemas/"

    if [[ ! -f "$ext_dir/schemas/gschemas.compiled" ]]; then
        printf "ERROR: gschemas.compiled was not generated\n"
        return 1
    fi

    install -Dm644 config.json \
        "$pkgdir/usr/share/willow/config.json"

    install -Dm644 context.json \
        "$pkgdir/usr/share/willow/context.json"

    install -Dm644 LICENSE \
        "$pkgdir/usr/share/licenses/$pkgname/LICENSE"

    install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
    install -Dm644 SMART_WORKFLOWS.md "$pkgdir/usr/share/doc/$pkgname/SMART_WORKFLOWS.md"
    install -Dm644 SMART_WORKFLOWS_QUICKSTART.md "$pkgdir/usr/share/doc/$pkgname/SMART_WORKFLOWS_QUICKSTART.md"

    install -Dm755 download-model.sh "$pkgdir/usr/bin/willow-download-model"
    install -Dm755 scripts/generate-keywords.py "$pkgdir/usr/share/willow/scripts/generate-keywords.py"
    install -Dm644 data/kws-default-keywords.txt "$pkgdir/usr/share/willow/kws-default-keywords.txt"
}
