# nixpkgs overlay packaging this applet, for wiring into another flake/config.
#
#   1. add this repo as a flake input, or `import` this file;
#   2. add `inputs.ai-usage.overlays.default` (or
#      `(import ./path/to/nix/overlay.nix)`) to `nixpkgs.overlays`;
#   3. add `ai-usage` to your `home.packages` or
#      `environment.systemPackages`;
#   4. run `nixos-rebuild switch` (or `home-manager switch`).
#
# The derivation itself lives in ./package.nix so the flake's `packages.default`
# and this overlay stay identical.

final: _prev: {
  ai-usage = final.callPackage ./package.nix { };
}
