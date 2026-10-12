# The applet derivation, factored out so both the flake (`packages.default`) and
# the nixpkgs overlay build the exact same thing.
#
# Mirrors nixpkgs' own `cosmic-applets` recipe: one `cargoHash` (fetchCargoVendor,
# which handles libcosmic's git workspace), plus `libcosmicAppHook`, which wraps
# the binary with the Wayland/Vulkan/xkb runtime libraries libcosmic dlopens — so
# no manual LD_LIBRARY_PATH wrapper is needed.
{
  lib,
  rustPlatform,
  pkg-config,
  libcosmicAppHook,
}:

rustPlatform.buildRustPackage {
  pname = "ai-usage";
  version = "0.1.2";

  # Only the files the build, tests, and install actually use. Keeping README,
  # screenshots, CI config, flake, etc. OUT of the build source means doc-only
  # changes don't change the derivation — so `nix build` is a cache hit instead
  # of recompiling libcosmic from scratch.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../data
      ../tests
    ];
  };

  # fetchCargoVendor; recapture with `cargoHash = ""` if Cargo.lock changes.
  # NOTE: a version bump changes Cargo.lock, which changes this hash — re-pin it.
  cargoHash = "sha256-J5ybHWZUOHwKqTwvRn0DSqY6qVw1lIp4IwhmjkTiaGk=";

  __structuredAttrs = true;

  nativeBuildInputs = [
    pkg-config
    libcosmicAppHook # wraps the binary with wayland/vulkan/xkb at runtime
    rustPlatform.bindgenHook
  ];

  postInstall = ''
    install -Dm0644 data/co.osterberg.AiUsage.desktop \
      $out/share/applications/co.osterberg.AiUsage.desktop
    substituteInPlace $out/share/applications/co.osterberg.AiUsage.desktop \
      --replace-warn 'Exec=ai-usage' "Exec=$out/bin/ai-usage"
  '';

  meta = {
    description = "Minimal COSMIC panel applet showing Claude Code usage";
    homepage = "https://github.com/osterbergsimon/cosmic-applet-claude-usage";
    license = lib.licenses.gpl3Only;
    mainProgram = "ai-usage";
    platforms = lib.platforms.linux;
  };
}
