{ config, lib, pkgs, ... }:
let
  cfg = config.programs.ferrex-player;
  packageSrc = cfg.package.src or (pkgs.ferrex-player.src or null);
in
{
  options.programs.ferrex-player = {
    enable = lib.mkEnableOption "ferrex media player";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ferrex-player;
      description = "Package providing the ferrex-player binary.";
    };

    serverUrl = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional URL of the ferrex server.";
    };

    settings = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Additional environment variable overrides for ferrex-player.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.dataFile = {
      "applications/io.github.lowband21.FerrexPlayer.desktop".text = ''
        [Desktop Entry]
        Name=Ferrex Player
        Comment=Native media player for Ferrex server with zero-copy HDR on Wayland
        GenericName=Media Player
        Exec=ferrex-player
        Icon=io.github.lowband21.FerrexPlayer
        Terminal=false
        Type=Application
        Categories=AudioVideo;Video;Player;
        Keywords=media;video;streaming;hdr;wayland;player;
        StartupNotify=true
        StartupWMClass=ferrex-player
      '';
    }
    // lib.optionalAttrs (packageSrc != null) {
      "icons/hicolor/128x128/apps/io.github.lowband21.FerrexPlayer.png".source = "${packageSrc}/flatpak/icons/128x128/apps/io.github.lowband21.FerrexPlayer.png";
      "icons/hicolor/192x192/apps/io.github.lowband21.FerrexPlayer.png".source = "${packageSrc}/flatpak/icons/192x192/apps/io.github.lowband21.FerrexPlayer.png";
      "icons/hicolor/512x512/apps/io.github.lowband21.FerrexPlayer.png".source = "${packageSrc}/flatpak/icons/512x512/apps/io.github.lowband21.FerrexPlayer.png";
    };

    home.sessionVariables =
      lib.optionalAttrs (cfg.serverUrl != null) {
        FERREX_SERVER_URL = cfg.serverUrl;
      }
      // cfg.settings;
  };
}
