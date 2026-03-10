{
  description = "Ferrex development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";

  outputs =
    { self, nixpkgs, rust-overlay }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);

      # GStreamer pin for Linux player builds.
      #
      # We keep this as an overlay so both devShells and packages can share it.
      gstOverlay_1_27_2 =
        final: prev:
        let
          version = "1.27.2";

          gstSet = prev.gst_all_1.overrideScope (
            gstFinal: gstPrev: {
              gstreamer = gstPrev.gstreamer.overrideAttrs (_old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gstreamer/gstreamer-${version}.tar.xz";
                  hash = "sha256-zhKcfqktzjBjsCkXHRMk0gOScTL8Pgz5K3hN3QVcJB0=";
                };
              });

              gst-plugins-base = gstPrev.gst-plugins-base.overrideAttrs (_old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-plugins-base/gst-plugins-base-${version}.tar.xz";
                  hash = "sha256-b1MKDqxP46jlSHw+6nsfrDB+KiHwVtm0F0eBioi+S/Y=";
                };
              });

              gst-plugins-good = gstPrev.gst-plugins-good.overrideAttrs (_old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-plugins-good/gst-plugins-good-${version}.tar.xz";
                  hash = "sha256-TwR0FtcbECmY20zFE5JcsxfYejQJ0P+uO6IYdlsPHOU=";
                };
              });

              gst-plugins-bad = gstPrev.gst-plugins-bad.overrideAttrs (old: {
                inherit version;
                src = prev.fetchurl {
                  url = "https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-${version}.tar.xz";
                  hash = "sha256-9O9K+8D3F2K6vti7p8LcTc2Q1aQB4Keb0F87lZdJdtM=";
                };
                mesonFlags =
                  (old.mesonFlags or [ ])
                  ++ [
                    # Avoid enabling every new "auto" plugin in the 1.27.x dev series,
                    # since some optional deps aren't packaged in nixpkgs yet.
                    "-Dauto_features=disabled"
                    # This is a dev-shell dependency; skip docs to reduce build time and
                    # avoid doc/introspection coupling issues.
                    "-Ddoc=disabled"
                    "-Dwayland=enabled"
                    "-Dva=enabled"
                    # Optional TensorFlow Lite plugin (dependency not packaged in nixpkgs today).
                    "-Dtflite=disabled"
                  ];
              });
            }
          );
        in
        {
          gst_1_27_2 = gstSet;
        };

      workspaceToml = fromTOML (builtins.readFile ./Cargo.toml);
      workspaceVersion = workspaceToml.workspace.package.version or "0.0.0";

      # Common outputHashes for all git-sourced crates
      commonOutputHashes = {
        # gtk-rs-core (glib, gio, etc.) — git+https://github.com/gtk-rs/gtk-rs-core@999d7194
        "gio-sys-0.23.0-alpha" = "sha256-1jWwY1kpp3W6V9zV9Fp70cl4oXc70Q2ieje6fAQHhi8=";
        "glib-0.23.0-alpha" = "sha256-1jWwY1kpp3W6V9zV9Fp70cl4oXc70Q2ieje6fAQHhi8=";
        "glib-macros-0.23.0-alpha" = "sha256-1jWwY1kpp3W6V9zV9Fp70cl4oXc70Q2ieje6fAQHhi8=";
        "glib-sys-0.23.0-alpha" = "sha256-1jWwY1kpp3W6V9zV9Fp70cl4oXc70Q2ieje6fAQHhi8=";
        "gobject-sys-0.23.0-alpha" = "sha256-1jWwY1kpp3W6V9zV9Fp70cl4oXc70Q2ieje6fAQHhi8=";
        # gstreamer-rs — git+https://gitlab.freedesktop.org/gstreamer/gstreamer-rs@05d28e33
        "gstreamer-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        "gstreamer-app-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        "gstreamer-app-sys-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        "gstreamer-base-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        "gstreamer-base-sys-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        "gstreamer-sys-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        "gstreamer-video-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        "gstreamer-video-sys-0.26.0-alpha" = "sha256-VfCWnBpt5hR2JGVBrbKXt/oS1HzrMIHfC3UW1BWZnBE=";
        # iced-ferrex
        "iced-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_aw-0.13.0-dev" = "sha256-Z9+uQmaAJrHV6kG2MiSaA+ksWj7FNw3Fr9yeDv8gY5g=";
        "iced_beacon-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_core-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_debug-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_devtools-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_fonts-0.3.0-dev" = "sha256-hX+45thUQ0cX/Xo36jCS8dllZJxm1c3CSktHSrlu1dw=";
        "iced_fonts_macros-0.3.0-dev" = "sha256-hX+45thUQ0cX/Xo36jCS8dllZJxm1c3CSktHSrlu1dw=";
        "iced_futures-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_graphics-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_program-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_renderer-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_runtime-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_selector-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_test-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_tester-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_tiny_skia-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_wgpu-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_widget-0.14.2" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        "iced_winit-0.14.0" = "sha256-tVqKPy2i9fWEoh2CpItSzwsRytD0w9CNuIbtk9cTJyE=";
        # lucide-icons-iced
        "lucide-icons-0.525.0" = "sha256-RxSdYtQqeu6Uhv1eN+QGL/e7ioi+7JQP6jm64HAkrHc=";
        # subwave
        "subwave_appsink-0.1.0" = "sha256-fFQx1QuN+3fXjm8tGxWTBjapUbbwFlGogBYTW5I5jrQ=";
        "subwave_core-0.1.0" = "sha256-fFQx1QuN+3fXjm8tGxWTBjapUbbwFlGogBYTW5I5jrQ=";
        "subwave_unified-0.1.0" = "sha256-fFQx1QuN+3fXjm8tGxWTBjapUbbwFlGogBYTW5I5jrQ=";
        "subwave_wayland-0.1.0" = "sha256-fFQx1QuN+3fXjm8tGxWTBjapUbbwFlGogBYTW5I5jrQ=";
      };
    in
    {
      overlays.gst_1_27_2 = gstOverlay_1_27_2;

      packages = forAllSystems (
        system:
        let
          pkgsPlayer = import nixpkgs {
            inherit system;
            overlays = [
              self.overlays.gst_1_27_2
              rust-overlay.overlays.default
            ];
            config.allowUnfree = true;
          };
          gst = pkgsPlayer.gst_1_27_2;
          ffmpegPkgPlayer =
            if pkgsPlayer ? ffmpeg-full then pkgsPlayer.ffmpeg-full else pkgsPlayer.ffmpeg;
          libclang = pkgsPlayer.llvmPackages.libclang;

          rustToolchain = pkgsPlayer.rust-bin.stable."1.92.0".default;
          rustPlatform = pkgsPlayer.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };

          ferrexPlayerBin = rustPlatform.buildRustPackage {
            pname = "ferrex-player";
            version = workspaceVersion;
            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = commonOutputHashes;
            };

            nativeBuildInputs = with pkgsPlayer; [
              pkg-config
              llvmPackages.clang
              makeWrapper
            ];

            buildInputs =
              [
                libclang
                ffmpegPkgPlayer.dev

                gst.gstreamer
                gst.gst-plugins-base
                gst.gst-plugins-good
                gst.gst-plugins-bad

                gst.gstreamer.dev
                gst.gst-plugins-base.dev
                gst.gst-plugins-good.dev

                pkgsPlayer.pipewire
                pkgsPlayer.libva
                pkgsPlayer.libdrm
                pkgsPlayer.mesa
                pkgsPlayer.vulkan-loader
                pkgsPlayer.wayland
                pkgsPlayer.libxkbcommon
                pkgsPlayer.xorg.libX11
                pkgsPlayer.xorg.libXcursor
                pkgsPlayer.xorg.libXi
                pkgsPlayer.xorg.libXrandr
              ];

            # Build only the player crate.
            cargoBuildFlags = [
              "-p"
              "ferrex-player"
            ];

            env = {
              SQLX_OFFLINE = "true";
              LIBCLANG_PATH = "${libclang.lib}/lib";
            };

            doCheck = false;
          };

          ferrexServerBin = rustPlatform.buildRustPackage {
            pname = "ferrex-server";
            version = workspaceVersion;
            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = commonOutputHashes;
            };

            nativeBuildInputs = with pkgsPlayer; [ pkg-config ];

            buildInputs = with pkgsPlayer; [ ffmpegPkgPlayer.dev openssl ];

            cargoBuildFlags = [ "-p" "ferrex-server" ];

            env = {
              SQLX_OFFLINE = "true";
            };

            doCheck = false;
          };

          ferrexCtlBin = rustPlatform.buildRustPackage {
            pname = "ferrexctl";
            version = workspaceVersion;
            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = commonOutputHashes;
            };

            nativeBuildInputs = with pkgsPlayer; [ pkg-config ];

            buildInputs = with pkgsPlayer; [ ffmpegPkgPlayer.dev openssl ];

            cargoBuildFlags = [ "-p" "ferrexctl" ];

            env = {
              SQLX_OFFLINE = "true";
            };

            doCheck = false;
          };
        in
        {
          gstreamer_1_27_2 = gst.gstreamer;
          gst_plugins_base_1_27_2 = gst.gst-plugins-base;
          gst_plugins_good_1_27_2 = gst.gst-plugins-good;
          gst_plugins_bad_1_27_2 = gst.gst-plugins-bad;

          ferrex-player-bin = ferrexPlayerBin;

          # Nix-friendly wrapper:
          # - forces plugin discovery to the pinned GStreamer 1.27.2 set
          # - sets LD_LIBRARY_PATH for dlopen-loaded Wayland/X11/Vulkan libs
          ferrex-player = pkgsPlayer.runCommand "ferrex-player-${workspaceVersion}" {
            nativeBuildInputs = [ pkgsPlayer.makeWrapper ];
          } ''
            mkdir -p "$out/bin"
            makeWrapper "${ferrexPlayerBin}/bin/ferrex-player" "$out/bin/ferrex-player" \
              --run 'if [ -d /run/opengl-driver ]; then
                export LD_LIBRARY_PATH="/run/opengl-driver/lib''${LD_LIBRARY_PATH:+:}$LD_LIBRARY_PATH"
                export LIBGL_DRIVERS_PATH="/run/opengl-driver/lib/dri"
                export LIBVA_DRIVERS_PATH="/run/opengl-driver/lib/dri"
                export __EGL_VENDOR_LIBRARY_DIRS="/run/opengl-driver/share/glvnd/egl_vendor.d''${__EGL_VENDOR_LIBRARY_DIRS:+:}$__EGL_VENDOR_LIBRARY_DIRS"

                if [ -d /run/opengl-driver/share/vulkan/icd.d ]; then
                  shopt -s nullglob
                  icds=(/run/opengl-driver/share/vulkan/icd.d/*.json)
                  shopt -u nullglob
                  if [ "''${#icds[@]}" -gt 0 ]; then
                    export VK_ICD_FILENAMES="$(IFS=:; echo "''${icds[*]}")"
                  fi
                fi
              fi' \
              --set GST_PLUGIN_SYSTEM_PATH_1_0 "${gst.gstreamer.out}/lib/gstreamer-1.0:${gst.gst-plugins-base.out}/lib/gstreamer-1.0:${gst.gst-plugins-good.out}/lib/gstreamer-1.0:${gst.gst-plugins-bad.out}/lib/gstreamer-1.0" \
              --set GST_PLUGIN_PATH_1_0 "${gst.gstreamer.out}/lib/gstreamer-1.0:${gst.gst-plugins-base.out}/lib/gstreamer-1.0:${gst.gst-plugins-good.out}/lib/gstreamer-1.0:${gst.gst-plugins-bad.out}/lib/gstreamer-1.0" \
              --prefix LD_LIBRARY_PATH : "${pkgsPlayer.wayland}/lib:${pkgsPlayer.libxkbcommon}/lib:${pkgsPlayer.xorg.libX11}/lib:${pkgsPlayer.xorg.libXcursor}/lib:${pkgsPlayer.xorg.libXi}/lib:${pkgsPlayer.xorg.libXrandr}/lib:${pkgsPlayer.vulkan-loader}/lib"
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
        ferrex-player = self.packages.${final.system}.ferrex-player;
        ferrex-server = self.packages.${final.system}.ferrex-server;
        ferrexctl = self.packages.${final.system}.ferrexctl;
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
              self.overlays.gst_1_27_2
              rust-overlay.overlays.default
            ];
            config.allowUnfree = true;
          };
          gst = pkgsPlayer.gst_1_27_2;

          rustToolchain = pkgsPlayer.rust-bin.stable."1.92.0".default;

          ffmpegPkg = if pkgs ? ffmpeg-full then pkgs.ffmpeg-full else pkgs.ffmpeg;
          ffmpegPkgPlayer =
            if pkgsPlayer ? ffmpeg-full then pkgsPlayer.ffmpeg-full else pkgsPlayer.ffmpeg;
          libclang = pkgs.llvmPackages.libclang;
        in
        {
          default = pkgs.mkShell {
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
              postgresql
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

          ferrex-player = pkgsPlayer.mkShell {
            nativeBuildInputs = with pkgsPlayer; [
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
              postgresql
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

            buildInputs =
              [
                pkgsPlayer.pipewire
                pkgsPlayer.llvmPackages.libclang
                ffmpegPkgPlayer.dev

                # Include full outputs so setup hooks set `GST_PLUGIN_SYSTEM_PATH_1_0`.
                gst.gstreamer
                gst.gst-plugins-base
                gst.gst-plugins-good
                gst.gst-plugins-bad

                # Headers/pkg-config for builds.
                gst.gstreamer.dev
                gst.gst-plugins-base.dev
                gst.gst-plugins-good.dev

                # VA-API / dmabuf runtime dependencies (helps keep crash reports actionable).
                pkgsPlayer.libva
                pkgsPlayer.libdrm
                pkgsPlayer.mesa

                # wgpu backends (Vulkan/OpenGL).
                pkgsPlayer.vulkan-loader
              ]
              ++ pkgsPlayer.lib.optionals pkgsPlayer.stdenv.hostPlatform.isLinux [
                # winit loads Wayland/X11 libs via dlopen; ensure they're in the shell
                # environment so `cargo run` binaries can find them on NixOS.
                pkgsPlayer.wayland
                pkgsPlayer.libxkbcommon
                pkgsPlayer.xorg.libX11
                pkgsPlayer.xorg.libXcursor
                pkgsPlayer.xorg.libXi
                pkgsPlayer.xorg.libXrandr
              ];

            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/target-nix"
              export LIBCLANG_PATH="${pkgsPlayer.llvmPackages.libclang.lib}/lib"

              # Helps crates like ffmpeg-sys-next when building outside Nix's build sandbox.
              export PKG_CONFIG_PATH="${ffmpegPkgPlayer.dev}/lib/pkgconfig:${ffmpegPkgPlayer.dev}/share/pkgconfig:''${PKG_CONFIG_PATH:-}"

              # Keep GStreamer plugin discovery consistent (avoid mixing system plugins
              # from other GStreamer versions via $NIX_PROFILES).
              #
              # NOTE: `multiqueue` (required by playbin3/decodebin3) lives in
              # `libgstcoreelements.so` from the `gstreamer` package, so include
              # `${gst.gstreamer}/lib/gstreamer-1.0`.
              #
              # In nixpkgs, `gstreamer` is multi-output; `gst.gstreamer` can resolve
              # to the `bin` output in some contexts, which does *not* contain
              # `lib/gstreamer-1.0`. Use `.out` explicitly so core elements are
              # discoverable.
              export GST_PLUGIN_SYSTEM_PATH_1_0="${gst.gstreamer.out}/lib/gstreamer-1.0:${gst.gst-plugins-base.out}/lib/gstreamer-1.0:${gst.gst-plugins-good.out}/lib/gstreamer-1.0:${gst.gst-plugins-bad.out}/lib/gstreamer-1.0"
              export GST_PLUGIN_PATH_1_0="$GST_PLUGIN_SYSTEM_PATH_1_0"

              export LD_LIBRARY_PATH="${pkgsPlayer.wayland}/lib:${pkgsPlayer.libxkbcommon}/lib:${pkgsPlayer.xorg.libX11}/lib:${pkgsPlayer.xorg.libXcursor}/lib:${pkgsPlayer.xorg.libXi}/lib:${pkgsPlayer.xorg.libXrandr}/lib:${pkgsPlayer.vulkan-loader}/lib:''${LD_LIBRARY_PATH:-}"

              # Prefer system GPU drivers on NixOS for Vulkan/GL discovery.
              if [ -d /run/opengl-driver ]; then
                export LD_LIBRARY_PATH="/run/opengl-driver/lib''${LD_LIBRARY_PATH:+:}$LD_LIBRARY_PATH"
                export LIBGL_DRIVERS_PATH="/run/opengl-driver/lib/dri"
                export __EGL_VENDOR_LIBRARY_DIRS="/run/opengl-driver/share/glvnd/egl_vendor.d"
                export LIBVA_DRIVERS_PATH="/run/opengl-driver/lib/dri"

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
                # Non-NixOS fallback: use the Mesa packages in this shell.
                export LD_LIBRARY_PATH="${pkgsPlayer.mesa}/lib''${LD_LIBRARY_PATH:+:}$LD_LIBRARY_PATH"
                export LIBGL_DRIVERS_PATH="${pkgsPlayer.mesa}/lib/dri"
                export LIBVA_DRIVERS_PATH="${pkgsPlayer.mesa}/lib/dri"
                export __EGL_VENDOR_LIBRARY_DIRS="${pkgsPlayer.mesa}/share/glvnd/egl_vendor.d:''${__EGL_VENDOR_LIBRARY_DIRS:-}"

                export GST_VA_DISPLAY="''${GST_VA_DISPLAY:-wayland}"
              fi

              echo "GStreamer: $(pkg-config --modversion gstreamer-1.0 2>/dev/null || true)"
              echo "Tip: confirm VA with: vainfo && gst-inspect-1.0 vapostproc"
            '';
          };
        }
      );
    };
}
