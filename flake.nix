{
  description = "stalk-api dev shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      devShells = forAll (pkgs: {
        # Only extra tools. Rust (rustup, via rust-toolchain.toml), the C compiler
        # and openssl stay the system ones, so release binaries run on dhole and
        # don't depend on /nix/store. That's also why this is mkShellNoCC.
        default = pkgs.mkShellNoCC {
          packages = [
            pkgs.sqlx-cli
            # Its own test suite fails in the nix sandbox; the tool itself works.
            (pkgs.cargo-llvm-cov.overrideAttrs { doCheck = false; })
            pkgs.sqlite
          ];
        };
      });
    };
}
