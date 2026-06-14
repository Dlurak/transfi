{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      forEachSystem =
        function:
        nixpkgs.lib.genAttrs [ "aarch64-linux" "x86_64-linux" ] (
          system: function nixpkgs.legacyPackages.${system}
        );
    in
    {
      packages = forEachSystem (pkgs: rec {
        transfi = pkgs.callPackage ./nix/build.nix { };
        default = transfi;
      });

      devShell = forEachSystem (
        pkgs:
        pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rustfmt
            rust-analyzer
            rustPackages.clippy
            bacon
          ];
          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
        }
      );
    };
}
