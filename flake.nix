{
  description = "Ferrex development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.crane.url = "github:ipetkov/crane";

  outputs =
    { self, nixpkgs, rust-overlay, crane }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);

      # GStreamer pin for Linux player builds.
      #
      # We keep this as an overlay so both devShells and packages can share it.
      # 1.28.4 is the current stable release (bug-fix on top of 1.28.x).
      gstOverlay_1_28_4 =
        final: prev:
        let
          version = "1.28.4";

          gstSet = prev.gst_all_1.overrideScope (
            gstFinal: gstPrev: {
              gstreamer = gstPrev.gstreamer.overrideAttrs (_old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gstreamer/gstreamer-${version}.tar.xz";
                  hash = "sha256-9a3H6PRIwQJgs7JaoQHJ1UBnTI2aVMK3eobQTys7UN0=";
                };
              });

              gst-plugins-base = gstPrev.gst-plugins-base.overrideAttrs (_old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-plugins-base/gst-plugins-base-${version}.tar.xz";
                  hash = "sha256-qJiv1XZhcrAEnmeBVY4GiQmL+HudgrhGxlLlccAdYNg=";
                };
              });

              gst-plugins-good = gstPrev.gst-plugins-good.overrideAttrs (_old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-plugins-good/gst-plugins-good-${version}.tar.xz";
                  hash = "sha256-yCXqc3xZzqDkoMQdojiARf9d0y0WIiCsk6eoLuSgTmE=";
                };
              });

              gst-plugins-bad = gstPrev.gst-plugins-bad.overrideAttrs (old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-${version}.tar.xz";
                  hash = "sha256-MytzIPMMYPLVlBRG0DudBeN4HywlYb776IcYvXd/Dkc=";
                };
                buildInputs = (old.buildInputs or [ ]) ++ [
                  prev.libdrm
                  prev.systemdMinimal  # for libudev
                ];
                # Start from the nixpkgs base flags and layer our overrides.
                # Meson last-wins, so our values take precedence.
                #
                # The previous overlay used -Dauto_features=disabled which
                # turned off ALL optional plugins (including AV1, Vulkan,
                # DRM, closedcaption, …).  This caused decodebin3 to report
                # "Missing element: AV1 decoder" and tear down the audio
                # chain.  The flags below match the old manual meson build.
                mesonFlags =
                  (old.mesonFlags or [ ])
                  ++ [
                    # --- features from the old working meson build ---
                    "-Dgpl=enabled"
                    "-Dwayland=enabled"
                    "-Dva=enabled"
                    # Vulkan plugin has GIR introspection issues in the
                    # Nix sandbox (exit code 126 running test binaries).
                    # Disable for now — not needed for A/V decode.
                    "-Dvulkan=disabled"
                    "-Dvulkan-video=disabled"
                    "-Ddrm=enabled"
                    "-Dudev=enabled"
                    "-Dkms=enabled"
                    "-Dclosedcaption=enabled"
                    # --- deps not (yet) packaged in nixpkgs ---
                    "-Dmpeghdec=disabled"
                    "-Dtflite=disabled"
                    "-Dwpe2=disabled"
                    "-Dwebrtc=disabled"
                    "-Dwebrtcdsp=disabled"
                    "-Dlcevcdecoder=disabled"
                    # Skip docs to reduce build time.
                    "-Ddoc=disabled"
                  ];
              });

              gst-plugins-ugly = gstPrev.gst-plugins-ugly.overrideAttrs (old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-plugins-ugly/gst-plugins-ugly-${version}.tar.xz";
                  hash = "sha256-VIbNFFxa9DJZ/TfKylnQSOKmfdsHCC6o9Q7w8CqF+KU=";
                };
                mesonFlags =
                  (old.mesonFlags or [ ])
                  ++ [
                    "-Dgpl=enabled"
                  ];
              });

              gst-libav = gstPrev.gst-libav.overrideAttrs (_old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-libav/gst-libav-${version}.tar.xz";
                  hash = "sha256-vRel3yh0p6WLy697lAIjN5rZYTYk246teD2wPnS7kEs=";
                };
              });
            }
          );
        in
        {
          gst_1_28_4 = gstSet;
        };

      workspaceToml = fromTOML (builtins.readFile ./Cargo.toml);
      workspaceVersion = workspaceToml.workspace.package.version or "0.0.0";

      playerMediaInputs =
        { pkgs, gst }:
        [
          pkgs.pipewire

          # Include full outputs so setup hooks set `GST_PLUGIN_SYSTEM_PATH_1_0`.
          gst.gstreamer
          gst.gst-plugins-base
          gst.gst-plugins-good
          gst.gst-plugins-bad
          gst.gst-plugins-ugly
          gst.gst-libav

          # Headers/pkg-config for builds.
          gst.gstreamer.dev
          gst.gst-plugins-base.dev
          gst.gst-plugins-good.dev

          # VA-API / dmabuf runtime dependencies (helps keep crash reports actionable).
          pkgs.libva
          pkgs.libdrm
          pkgs.mesa

          # wgpu backends (Vulkan/OpenGL).
          pkgs.vulkan-loader
        ]
        ++ nixpkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          # winit loads Wayland/X11 libs via dlopen; ensure they're in the shell
          # environment so `cargo run` binaries can find them on NixOS.
          pkgs.wayland
          pkgs.libxkbcommon
          pkgs.libx11
          pkgs.libxcursor
          pkgs.libxi
          pkgs.libxrandr
        ];

      makePlayerRuntimeEnv =
        { pkgs, gst }:
        let
          gstPluginPath = nixpkgs.lib.concatStringsSep ":" [
            "${gst.gstreamer.out}/lib/gstreamer-1.0"
            "${gst.gst-plugins-base.out}/lib/gstreamer-1.0"
            "${gst.gst-plugins-good.out}/lib/gstreamer-1.0"
            "${gst.gst-plugins-bad.out}/lib/gstreamer-1.0"
            "${gst.gst-plugins-ugly.out}/lib/gstreamer-1.0"
            "${gst.gst-libav.out}/lib/gstreamer-1.0"
            "${pkgs.pipewire}/lib/gstreamer-1.0"
          ];

          gpuLibraryPath = nixpkgs.lib.concatStringsSep ":" [
            "${pkgs.wayland}/lib"
            "${pkgs.libxkbcommon}/lib"
            "${pkgs.libx11}/lib"
            "${pkgs.libxcursor}/lib"
            "${pkgs.libxi}/lib"
            "${pkgs.libxrandr}/lib"
            "${pkgs.vulkan-loader}/lib"
          ];
        in
        {
          inherit gstPluginPath gpuLibraryPath;

          gpuEnvironment = ''
            # Prefer system GPU drivers on NixOS for Vulkan/GL discovery.
            if [ -d /run/opengl-driver ]; then
              export LD_LIBRARY_PATH="/run/opengl-driver/lib''${LD_LIBRARY_PATH:+:}$LD_LIBRARY_PATH"
              export LIBGL_DRIVERS_PATH="/run/opengl-driver/lib/dri"
              export LIBVA_DRIVERS_PATH="/run/opengl-driver/lib/dri"
              export __EGL_VENDOR_LIBRARY_DIRS="/run/opengl-driver/share/glvnd/egl_vendor.d''${__EGL_VENDOR_LIBRARY_DIRS:+:}$__EGL_VENDOR_LIBRARY_DIRS"

              # Best-effort default for VA-API on Wayland; override if needed.
              export GST_VA_DISPLAY="''${GST_VA_DISPLAY:-wayland}"

              if [ -z "''${LIBVA_DRIVER_NAME:-}" ]; then
                if [ -f /run/opengl-driver/lib/dri/radeonsi_drv_video.so ]; then
                  export LIBVA_DRIVER_NAME=radeonsi
                fi
              fi

              if [ -d /run/opengl-driver/share/vulkan/icd.d ]; then
                shopt -s nullglob
                icds=(/run/opengl-driver/share/vulkan/icd.d/*.json)
                shopt -u nullglob
                if [ "''${#icds[@]}" -gt 0 ]; then
                  export VK_ICD_FILENAMES="$(IFS=:; echo "''${icds[*]}")"
                fi
              fi
            else
              # Non-NixOS fallback: use the Mesa packages in this environment.
              export LD_LIBRARY_PATH="${pkgs.mesa}/lib''${LD_LIBRARY_PATH:+:}$LD_LIBRARY_PATH"
              export LIBGL_DRIVERS_PATH="${pkgs.mesa}/lib/dri"
              export LIBVA_DRIVERS_PATH="${pkgs.mesa}/lib/dri"
              export __EGL_VENDOR_LIBRARY_DIRS="${pkgs.mesa}/share/glvnd/egl_vendor.d''${__EGL_VENDOR_LIBRARY_DIRS:+:}$__EGL_VENDOR_LIBRARY_DIRS"

              export GST_VA_DISPLAY="''${GST_VA_DISPLAY:-wayland}"
            fi
          '';
        };

    in
    {
      overlays.gst_1_28_4 = gstOverlay_1_28_4;

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
            config.allowUnfree = true;
          };

          pkgsPlayer = import nixpkgs {
            inherit system;
            overlays = [
              self.overlays.gst_1_28_4
              rust-overlay.overlays.default
            ];
            config.allowUnfree = true;
          };
          gst = pkgsPlayer.gst_1_28_4;
          ffmpegPkg = if pkgs ? ffmpeg-full then pkgs.ffmpeg-full else pkgs.ffmpeg;
          ffmpegPkgPlayer =
            if pkgsPlayer ? ffmpeg-full then pkgsPlayer.ffmpeg-full else pkgsPlayer.ffmpeg;
          libclang = pkgs.llvmPackages.libclang;
          libclangPlayer = pkgsPlayer.llvmPackages.libclang;

          rustToolchain = pkgs.rust-bin.stable."1.92.0".default;
          rustToolchainPlayer = pkgsPlayer.rust-bin.stable."1.92.0".default;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          craneLibPlayer = (crane.mkLib pkgsPlayer).overrideToolchain rustToolchainPlayer;
          playerRuntime = makePlayerRuntimeEnv { pkgs = pkgsPlayer; inherit gst; };
          playerMediaBuildInputs = playerMediaInputs { pkgs = pkgsPlayer; inherit gst; };

          src =
            let
              sqlxFilter = path: _type: (builtins.match ".*\.sqlx/.*" path) != null;
              migrationsFilter = path: _type: (builtins.match ".*/migrations/.*\.sql$" path) != null;
              wgslFilter = path: _type: (builtins.match ".*\.wgsl$" path) != null;
              ttfFilter = path: _type: (builtins.match ".*\.ttf$" path) != null;
            in
            nixpkgs.lib.cleanSourceWith {
              src = ./.;
              filter =
                path: type:
                (sqlxFilter path type)
                || (migrationsFilter path type)
                || (wgslFilter path type)
                || (ttfFilter path type)
                || (craneLib.filterCargoSources path type);
            };

          mkCommonArgs =
            { pkgs, libclang, ffmpegPkg }:
            {
              inherit src;
              strictDeps = true;
              pname = "ferrex-workspace";
              version = workspaceVersion;

              nativeBuildInputs = with pkgs; [
                pkg-config
                llvmPackages.clang
              ];

              buildInputs = [
                libclang
                pkgs.openssl
                ffmpegPkg.dev
              ];

              SQLX_OFFLINE = "true";
              LIBCLANG_PATH = "${libclang.lib}/lib";
            };

          commonArgs = mkCommonArgs { inherit pkgs libclang ffmpegPkg; };
          playerCommonArgs = mkCommonArgs {
            pkgs = pkgsPlayer;
            libclang = libclangPlayer;
            ffmpegPkg = ffmpegPkgPlayer;
          };

          serverCargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "ferrex-server-deps";
            cargoExtraArgs = "-p ferrex-server";
          });

          ctlCargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "ferrexctl-deps";
            cargoExtraArgs = "-p ferrexctl";
          });

          playerCargoArtifacts = craneLibPlayer.buildDepsOnly (playerCommonArgs // {
            pname = "ferrex-player-deps";
            cargoExtraArgs = "-p ferrex-player";
            buildInputs = playerCommonArgs.buildInputs ++ playerMediaBuildInputs;
          });

          ferrexPlayerBin = craneLibPlayer.buildPackage (playerCommonArgs // {
            cargoArtifacts = playerCargoArtifacts;
            pname = "ferrex-player";
            cargoExtraArgs = "-p ferrex-player";
            doCheck = false;

            nativeBuildInputs = playerCommonArgs.nativeBuildInputs ++ (with pkgsPlayer; [
              makeWrapper
            ]);

            buildInputs = playerCommonArgs.buildInputs ++ playerMediaBuildInputs;
          });

          ferrexServerBin = craneLib.buildPackage (commonArgs // {
            cargoArtifacts = serverCargoArtifacts;
            pname = "ferrex-server";
            cargoExtraArgs = "-p ferrex-server";
            doCheck = false;
          });

          ferrexCtlBin = craneLib.buildPackage (commonArgs // {
            cargoArtifacts = ctlCargoArtifacts;
            pname = "ferrexctl";
            cargoExtraArgs = "-p ferrexctl";
            doCheck = false;
          });
        in
        {
          gstreamer_1_28_4 = gst.gstreamer;
          gst_plugins_base_1_28_4 = gst.gst-plugins-base;
          gst_plugins_good_1_28_4 = gst.gst-plugins-good;
          gst_plugins_bad_1_28_4 = gst.gst-plugins-bad;
          gst_plugins_ugly_1_28_4 = gst.gst-plugins-ugly;
          gst_libav_1_28_4 = gst.gst-libav;

          ferrex-player-bin = ferrexPlayerBin;

          # Nix-friendly wrapper:
          # - forces plugin discovery to the pinned GStreamer 1.28.4 set
          # - sets LD_LIBRARY_PATH for dlopen-loaded Wayland/X11/Vulkan libs
          ferrex-player = pkgsPlayer.runCommand "ferrex-player-${workspaceVersion}" {
            nativeBuildInputs = [ pkgsPlayer.makeWrapper ];
          } ''
            mkdir -p "$out/bin"
            makeWrapper "${ferrexPlayerBin}/bin/ferrex-player" "$out/bin/ferrex-player" \
              --run ${nixpkgs.lib.escapeShellArg playerRuntime.gpuEnvironment} \
              --set GST_PLUGIN_SYSTEM_PATH_1_0 "${playerRuntime.gstPluginPath}" \
              --set GST_PLUGIN_PATH_1_0 "${playerRuntime.gstPluginPath}" \
              --prefix LD_LIBRARY_PATH : "${playerRuntime.gpuLibraryPath}"
          '';

          ferrex-server = ferrexServerBin;
          ferrexctl = ferrexCtlBin;
        }
      );

      apps = forAllSystems (
        system:
        let
          pkgs = self.packages.${system};
        in
        {
          ferrex-player = {
            type = "app";
            program = "${pkgs.ferrex-player}/bin/ferrex-player";
          };
          ferrex-server = {
            type = "app";
            program = "${pkgs.ferrex-server}/bin/ferrex-server";
          };
          ferrexctl = {
            type = "app";
            program = "${pkgs.ferrexctl}/bin/ferrexctl";
          };
          default = self.apps.${system}.ferrex-player;
        }
      );

      nixosModules.ferrex-server = import ./nix/modules/ferrex-server.nix;

      overlays.default = final: prev: {
        ferrex-player = self.packages.${final.stdenv.hostPlatform.system}.ferrex-player;
        ferrex-server = self.packages.${final.stdenv.hostPlatform.system}.ferrex-server;
        ferrexctl = self.packages.${final.stdenv.hostPlatform.system}.ferrexctl;
      };

      homeManagerModules.ferrex-player = import ./nix/modules/ferrex-player-hm.nix;

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
            config.allowUnfree = true;
          };

          pkgsPlayer = import nixpkgs {
            inherit system;
            overlays = [
              self.overlays.gst_1_28_4
              rust-overlay.overlays.default
            ];
            config.allowUnfree = true;
          };
          gst = pkgsPlayer.gst_1_28_4;

          rustToolchain = pkgs.rust-bin.stable."1.92.0".default;
          rustToolchainPlayer = pkgsPlayer.rust-bin.stable."1.92.0".default;

          ffmpegPkg = if pkgs ? ffmpeg-full then pkgs.ffmpeg-full else pkgs.ffmpeg;
          ffmpegPkgPlayer =
            if pkgsPlayer ? ffmpeg-full then pkgsPlayer.ffmpeg-full else pkgsPlayer.ffmpeg;
          libclang = pkgs.llvmPackages.libclang;
          libclangPlayer = pkgsPlayer.llvmPackages.libclang;
          postgresqlWithPgUuidv7 = pkgs.postgresql.withPackages (ps: [ ps.pg_uuidv7 ]);
          postgresqlWithPgUuidv7Player = pkgsPlayer.postgresql.withPackages (ps: [ ps.pg_uuidv7 ]);
          playerRuntime = makePlayerRuntimeEnv { pkgs = pkgsPlayer; inherit gst; };
          playerShellBuildInputs = [
            libclangPlayer
            ffmpegPkgPlayer.dev
          ] ++ playerMediaInputs { pkgs = pkgsPlayer; inherit gst; };

          serverShell = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              rustToolchain
              pkg-config
              llvmPackages.clang
              just
              jq
              python3
              gh
              curl
              git
              wl-clipboard
              postgresqlWithPgUuidv7
              flatpak
              flatpak-builder
              appstream
              prek
              uv
              shfmt
              shellcheck
              hadolint
            ];

            buildInputs = [
              libclang
              ffmpegPkg.dev
            ];

            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/target-nix"
              export LIBCLANG_PATH="${libclang.lib}/lib"

              # Helps crates like ffmpeg-sys-next when building outside Nix's build sandbox.
              export PKG_CONFIG_PATH="${ffmpegPkg.dev}/lib/pkgconfig:${ffmpegPkg.dev}/share/pkgconfig:''${PKG_CONFIG_PATH:-}"
            '';
          };

          playerShell = pkgsPlayer.mkShell {
            nativeBuildInputs = with pkgsPlayer; [
              rustToolchainPlayer
              pkg-config
              llvmPackages.clang
              just
              jq
              python3
              gh
              curl
              git
              wl-clipboard
              postgresqlWithPgUuidv7Player
              flatpak
              flatpak-builder
              appstream
              gst.gstreamer.bin
              libva-utils
              vulkan-tools
              mesa-demos
              prek
              uv
              shfmt
              shellcheck
              hadolint
            ];

            buildInputs = playerShellBuildInputs;

            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/target-nix"
              export LIBCLANG_PATH="${libclangPlayer.lib}/lib"

              # Helps crates like ffmpeg-sys-next when building outside Nix's build sandbox.
              export PKG_CONFIG_PATH="${ffmpegPkgPlayer.dev}/lib/pkgconfig:${ffmpegPkgPlayer.dev}/share/pkgconfig:''${PKG_CONFIG_PATH:-}"

              # Keep GStreamer plugin discovery consistent (avoid mixing system plugins
              # from other GStreamer versions via $NIX_PROFILES).
              #
              # NOTE: `multiqueue` (required by playbin3/decodebin3) lives in
              # `libgstcoreelements.so` from the `gstreamer` package, so include
              # the `gstreamer` output explicitly; the package can otherwise
              # resolve to a non-plugin output in some contexts.
              export GST_PLUGIN_SYSTEM_PATH_1_0="${playerRuntime.gstPluginPath}"
              export GST_PLUGIN_PATH_1_0="$GST_PLUGIN_SYSTEM_PATH_1_0"

              export LD_LIBRARY_PATH="${playerRuntime.gpuLibraryPath}:''${LD_LIBRARY_PATH:-}"

              ${playerRuntime.gpuEnvironment}

              echo "GStreamer: $(pkg-config --modversion gstreamer-1.0 2>/dev/null || true)"
              echo "Tip: confirm VA with: vainfo && gst-inspect-1.0 vapostproc"
            '';
          };
        in
        {
          default = playerShell;
          ferrex-player = playerShell;
          server = serverShell;
        }
      );
    };
}
