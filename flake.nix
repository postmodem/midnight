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

      # MIDNIGHT_GPU_FLAGS: what builds and links runtime/midnight_gpu.c.
      # wgpu-native opens the Vulkan loader by name, so on Linux the
      # program itself is made to need it.
      gpuFlagsFor =
        pkgs:
        lib.concatStringsSep " " (
          [
            "-I${lib.getDev pkgs.wgpu-native}/include"
            "-L${lib.getLib pkgs.wgpu-native}/lib"
            "-lwgpu_native"
            "-Wl,-rpath,${lib.getLib pkgs.wgpu-native}/lib"
          ]
          ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            "-L${lib.getLib pkgs.vulkan-loader}/lib"
            "-Wl,--no-as-needed"
            "-lvulkan"
            "-Wl,--as-needed"
            "-Wl,-rpath,${lib.getLib pkgs.vulkan-loader}/lib"
          ]
        );

      # MIDNIGHT_WINDOW_FLAGS: what builds and links runtime/midnight_window.c
      # beside midnight_gpu.c.
      windowFlagsFor =
        pkgs:
        lib.concatStringsSep " " [
          "-I${lib.getDev pkgs.sdl3}/include"
          "-L${lib.getLib pkgs.sdl3}/lib"
          "-lSDL3"
          "-Wl,-rpath,${lib.getLib pkgs.sdl3}/lib"
        ];

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
            export MIDNIGHT_GPU_FLAGS="${gpuFlagsFor pkgs}"
            export MIDNIGHT_WINDOW_FLAGS="${windowFlagsFor pkgs}"
            export NIX_HARDENING_ENABLE=
          '';

          postInstall = ''
            wrapProgram $out/bin/midnight \
              --set-default MIDNIGHT_CC ${lib.getExe' pkgs.clang "clang"} \
              --set-default MIDNIGHT_GPU_FLAGS "${gpuFlagsFor pkgs}" \
              --set-default MIDNIGHT_WINDOW_FLAGS "${windowFlagsFor pkgs}" \
              --set NIX_HARDENING_ENABLE ""
          '';

          meta = {
            description = "Compiler for the Midnight language";
            license = lib.licenses.mit;
            mainProgram = "midnight";
          };
        };

      # The Vulkan driver file of Mesa's llvmpipe. With VK_DRIVER_FILES set
      # to it, llvmpipe is the only device a program finds.
      softwareIcdFor =
        pkgs: "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.parsed.cpu.name}.json";

      # tests/suite.rs as a program. It tests the compiler MIDNIGHT_BIN
      # names on the programs under the working directory.
      suiteFor =
        pkgs:
        (midnightFor pkgs).overrideAttrs {
          pname = "midnight-suite";
          buildPhase = ''
            cargo test --offline --release --no-run --test suite
          '';
          doCheck = false;
          installPhase = ''
            install -D "$(find target/release/deps -name 'suite-*' -type f -executable)" \
              $out/bin/midnight-suite
          '';
          postInstall = "";
          meta.mainProgram = "midnight-suite";
        };

      # Runs tests of the suite by name, and fails unless every name matches
      # a test.
      suiteTestsFor =
        pkgs: names:
        let
          suite = lib.getExe (suiteFor pkgs);
          list = lib.concatStringsSep " " names;
          count = toString (builtins.length names);
        in
        ''
          found=$(${suite} --exact --list ${list} | grep -c ': test$' || true)
          if [ "$found" != ${count} ]; then
            echo "midnight-suite: ${count} tests named, $found found" >&2
            exit 1
          fi
          ${suite} --exact ${list}
        '';

      # The suite's GPU tests.
      gpuTestsFor =
        pkgs:
        suiteTestsFor pkgs [
          "gpu_programs"
          "gpu_stats_match_plan"
          "gpu_stats_match_expected"
          "gpu_driver_finds_its_parts"
          "gpu_strict_div_sqrt"
        ];

      # The suite's ThreadSanitizer tests.
      tsanTestsFor =
        pkgs:
        suiteTestsFor pkgs [
          "tsan_pool_stress"
          "tsan_programs"
        ];

      # The suite's GPU mode on the machine's hardware device.
      gpuTestFor =
        pkgs:
        pkgs.writeShellApplication {
          name = "midnight-gpu-test";
          runtimeEnv = {
            MIDNIGHT_BIN = lib.getExe (midnightFor pkgs);
            MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
            MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
            MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
            NIX_HARDENING_ENABLE = "";
            MIDNIGHT_TEST_GPU = "hardware";
          };
          text = ''
            cd ${src}
            ${gpuTestsFor pkgs}
          '';
        };

      # What the window tests need besides the compiler: a compositor that
      # shows nothing on the desktop, and a way to take its picture.
      windowToolsFor = pkgs: [
        pkgs.sway-unwrapped
        pkgs.grim
        pkgs.clang
        pkgs.coreutils
        pkgs.findutils
        pkgs.gnugrep
        pkgs.gnused
      ];

      # tests/window/test.sh on this machine's GPU, with sway drawing as it
      # chooses.
      windowTestFor =
        pkgs:
        pkgs.writeShellApplication {
          name = "midnight-window-test";
          runtimeInputs = windowToolsFor pkgs;
          runtimeEnv = {
            MIDNIGHT_BIN = lib.getExe (midnightFor pkgs);
            MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
            MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
            MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
            NIX_HARDENING_ENABLE = "";
          };
          text = ''
            out=$(mktemp -d)
            cd ${src}
            bash tests/window/test.sh "$out"
            rm -r "$out"
          '';
        };

      benchFor =
        pkgs:
        pkgs.writeShellApplication {
          name = "midnight-bench";
          runtimeInputs = [
            (midnightFor pkgs)
            pkgs.clang
            pkgs.hyperfine
            pkgs.jq
            pkgs.gawk
            pkgs.coreutils
          ];
          runtimeEnv = {
            BENCH_SRC = "${src}";
            MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
            MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
            MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
            NIX_HARDENING_ENABLE = "";
          };
          text = builtins.readFile ./bench/bench.sh;
        };

      benchGpuFor =
        pkgs:
        pkgs.writeShellApplication {
          name = "midnight-bench-gpu";
          runtimeInputs = [
            (midnightFor pkgs)
            pkgs.gawk
            pkgs.gnugrep
            pkgs.gnused
            pkgs.coreutils
          ];
          runtimeEnv = {
            BENCH_SRC = "${src}";
            MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
            MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
            MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
          };
          text = builtins.readFile ./bench/bench-gpu.sh;
        };

      benchCoresFor =
        pkgs:
        pkgs.writeShellApplication {
          name = "midnight-bench-cores";
          runtimeInputs = [
            (midnightFor pkgs)
            pkgs.gawk
            pkgs.gnused
            pkgs.coreutils
          ];
          runtimeEnv = {
            BENCH_SRC = "${src}";
            MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
            MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
            MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
          };
          text = builtins.readFile ./bench/cores.sh;
        };
    in
    {
      packages = forAllSystems (pkgs: {
        default = midnightFor pkgs;
      });

      apps = forAllSystems (
        pkgs:
        {
          default = {
            type = "app";
            program = lib.getExe (midnightFor pkgs);
            meta.description = "The Midnight compiler";
          };
          bench = {
            type = "app";
            program = lib.getExe (benchFor pkgs);
            meta.description = "Time bench.12am against a hand-written C baseline, then churn.12am";
          };
          gpu-test = {
            type = "app";
            program = lib.getExe (gpuTestFor pkgs);
            meta.description = "Run the programs with GPU systems on this machine's GPU";
          };
          bench-gpu = {
            type = "app";
            program = lib.getExe (benchGpuFor pkgs);
            meta.description = "Time GPU systems on the GPU and on the CPU: gravity on 16,384 bodies, also strict, then a move of one million entities";
          };
          bench-cores = {
            type = "app";
            program = lib.getExe (benchCoresFor pkgs);
            meta.description = "Time loops on one thread and on every core, with no GPU: GPU blocks and independent each loops";
          };
        }
        // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          window-test = {
            type = "app";
            program = lib.getExe (windowTestFor pkgs);
            meta.description = "Run the window tests on this machine's GPU, inside a compositor that shows nothing on the desktop";
          };
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell (
          {
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
            MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
            MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
            hardeningDisable = [ "all" ];
          }
          // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            MIDNIGHT_TEST_SOFTWARE_ICD = softwareIcdFor pkgs;
            # For tests/window: a compositor of their own, and its picture.
            nativeBuildInputs = [
              pkgs.sway-unwrapped
              pkgs.grim
            ];
          }
        );
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
        // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          # The suite's GPU mode on llvmpipe, release and debug builds.
          gpu =
            pkgs.runCommand "midnight-gpu-tests"
              {
                MIDNIGHT_BIN = lib.getExe midnight;
                MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
                MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
                MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
                MIDNIGHT_TEST_GPU = "software";
                MIDNIGHT_TEST_SOFTWARE_ICD = softwareIcdFor pkgs;
              }
              ''
                export HOME=$TMPDIR XDG_RUNTIME_DIR=$TMPDIR
                cd ${src}
                ${gpuTestsFor pkgs}
                touch $out
              '';

          # tests/window/test.sh in the build sandbox: sway drawing with
          # pixman, programs on llvmpipe.
          window =
            pkgs.runCommand "midnight-window-tests"
              {
                nativeBuildInputs = windowToolsFor pkgs;
                MIDNIGHT_BIN = lib.getExe midnight;
                MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
                MIDNIGHT_GPU_FLAGS = gpuFlagsFor pkgs;
                MIDNIGHT_WINDOW_FLAGS = windowFlagsFor pkgs;
                NIX_HARDENING_ENABLE = "";
                RENDERER = "pixman";
                VK_DRIVER_FILES = softwareIcdFor pkgs;
              }
              ''
                export HOME=$TMPDIR
                mkdir -p $out
                cd ${src}
                set -o pipefail
                bash tests/window/test.sh $TMPDIR/window | tee $out/log
              '';

          # The suite's ThreadSanitizer tests.
          tsan =
            pkgs.runCommand "midnight-tsan-tests"
              {
                MIDNIGHT_BIN = lib.getExe midnight;
                MIDNIGHT_CC = lib.getExe' pkgs.clang "clang";
                MIDNIGHT_TEST_TSAN = "1";
              }
              ''
                export HOME=$TMPDIR
                cd ${src}
                ${tsanTestsFor pkgs}
                touch $out
              '';
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
