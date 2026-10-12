{
  description = "COSMIC applet showing AI usage";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        package = pkgs.callPackage ./nix/package.nix { };
        # Native deps libcosmic/iced/winit need to build and run on Wayland.
        runtimeLibs = with pkgs; [
          wayland libxkbcommon vulkan-loader libGL
          fontconfig freetype expat
        ];
        nativeDeps = with pkgs; [ pkg-config makeWrapper ];
      in {
        # `nix build`, `nix run`, `nix profile install github:…#default`.
        packages.default = package;
        packages.ai-usage = package;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = nativeDeps ++ (with pkgs; [ rustc cargo rustfmt clippy just ]);
          buildInputs = runtimeLibs;
          # winit/iced dlopen wayland & vulkan at runtime; expose them.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
          RUST_BACKTRACE = "1";
        };
      })
    // {
      # System-independent: drop into another config's `nixpkgs.overlays`.
      overlays.default = import ./nix/overlay.nix;
    };
}
