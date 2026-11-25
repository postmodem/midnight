{
  description = "Midnight: a compiled, Ruby-flavored language built around entities, components and systems";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      version = (lib.importTOML ./Cargo.toml).package.version;

      # Everything the compiler and its tests read.
      src = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./Cargo.toml
          ./Cargo.lock
          ./src
          ./runtime
          ./docs/language.md
          (lib.fileset.maybeMissing ./tests)
          (lib.fileset.maybeMissing ./examples)
          (lib.fileset.maybeMissing ./bench)
        ];
      };

      midnightFor =
        pkgs:
        pkgs.rustPlatform.buildRustPackage {
          pname = "midnight";
          inherit version src;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.makeWrapper ];
          nativeCheckInputs = [ pkgs.clang ];
          # The nixpkgs compiler wrapper adds hardening flags of its own;
          # generated programs are built with the flags midnight passes and
          # no others.
          preCheck = ''
            export MIDNIGHT_CC=${lib.getExe' pkgs.clang "clang"}
            export NIX_HARDENING_ENABLE=
          '';

          postInstall = ''
            wrapProgram $out/bin/midnight \
              --set-default MIDNIGHT_CC ${lib.getExe' pkgs.clang "clang"} \
              --set NIX_HARDENING_ENABLE ""
          '';

          meta = {
            description = "Compiler for the Midnight language";
            license = lib.licenses.mit;
            mainProgram = "midnight";
          };
        };
    in
    {
      packages = forAllSystems (pkgs: {
        default = midnightFor pkgs;
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = lib.getExe (midnightFor pkgs);
          meta.description = "The Midnight compiler";
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
            pkgs.clang
            pkgs.nixfmt
          ];
          MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
          hardeningDisable = [ "all" ];
        };
      });

      checks = forAllSystems (
        pkgs:
        let
          midnight = midnightFor pkgs;
        in
        {
          package = midnight;

          clippy = midnight.overrideAttrs (old: {
            pname = "midnight-clippy";
            nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.clippy ];
            buildPhase = ''
              cargo clippy --offline --all-targets -- -D warnings
            '';
            doCheck = false;
            installPhase = ''
              touch $out
            '';
            postInstall = "";
          });

          rustfmt =
            pkgs.runCommand "check-rustfmt"
              {
                nativeBuildInputs = [
                  pkgs.cargo
                  pkgs.rustfmt
                ];
              }
              ''
                cd ${src}
                cargo fmt --check
                touch $out
              '';

          nixfmt =
            pkgs.runCommand "check-nixfmt"
              {
                nativeBuildInputs = [ pkgs.nixfmt ];
              }
              ''
                find ${self} -name '*.nix' -exec nixfmt --check {} +
                touch $out
              '';
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
