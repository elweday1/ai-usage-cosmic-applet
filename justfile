appid := 'co.osterberg.AiUsage'

# ---- system install (any COSMIC distro) -------------------------------------
# Staging root (for packagers) and install prefix. Override on the command line:
#   sudo just install                 # → /usr/bin, /usr/share/applications
#   sudo just prefix=/usr/local install
#   just rootdir=$pkgdir prefix=/usr install   # packaging into a staging dir
rootdir := ''
prefix := '/usr'
base := rootdir / prefix
bin-dst := base / 'bin/ai-usage'
desktop-dst := base / 'share/applications' / appid + '.desktop'

# Build the optimized binary.
build:
    cargo build --release

test:
    cargo test
    bash contrib/test-usage-writer.sh
    bash contrib/test-install-statusline.sh

# Install the usage-writer statusLine into Claude Code (~/.claude) so the applet
# gets fed. Leaves an existing statusLine untouched (prints how to chain it).
install-statusline:
    bash contrib/install-statusline.sh

# The desktop Exec stays a bare command name, resolved via PATH — correct for an
# FHS prefix whose bin/ is on PATH. (The Nix package rewrites it to an absolute
# /nix/store path instead; see nix/package.nix.)

# Install binary + .desktop under {{prefix}} (sudo for /usr; Exec resolved via PATH).
install:
    install -Dm0755 target/release/ai-usage {{bin-dst}}
    install -Dm0644 data/{{appid}}.desktop {{desktop-dst}}

uninstall:
    rm -f {{bin-dst}} {{desktop-dst}}

# ---- dev install (no sudo, no /usr) -----------------------------------------
bindir := env_var('HOME') / '.local/bin'
libexecdir := env_var('HOME') / '.local/libexec'
bin-real := libexecdir / 'ai-usage-bin'
bin-wrapper := bindir / 'ai-usage'
desktop-dev := env_var('HOME') / '.local/share/applications' / appid + '.desktop'

# Dev install for iterating before the Nix package is wired in. winit dlopens
# libwayland/libGL/vulkan at runtime, and the panel launches the binary WITHOUT
# the devShell env — so bake the devShell's LD_LIBRARY_PATH (set while this runs
# under `nix develop`) into a wrapper. NOTE: those /nix/store paths are not
# GC-roots, so this breaks on `nix-collect-garbage`; use the Nix package for a
# durable install.

# Dev install to ~/.local (no sudo; LD_LIBRARY_PATH-wrapped). Run under nix develop.
install-dev: build
    install -Dm0755 target/release/ai-usage {{bin-real}}
    mkdir -p {{bindir}}
    printf '#!/bin/sh\nexport LD_LIBRARY_PATH=%s\nexec %s "$@"\n' "$LD_LIBRARY_PATH" "{{bin-real}}" > {{bin-wrapper}}
    chmod +x {{bin-wrapper}}
    mkdir -p $(dirname {{desktop-dev}})
    sed 's|^Exec=.*|Exec={{bin-wrapper}}|' data/{{appid}}.desktop > {{desktop-dev}}
