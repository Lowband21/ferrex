{ config, lib, pkgs, ... }:
let
  cfg = config.services.ferrex;
in
{
  options.services.ferrex = {
    enable = lib.mkEnableOption "ferrex media server";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ferrex-server;
      description = "Package providing the ferrex-server binary. Use the flake overlay or set this explicitly.";
    };

    mediaRoot = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Root path for media files.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 3000;
      description = "Port ferrex-server listens on.";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "0.0.0.0";
      description = "Host/interface ferrex-server binds to.";
    };

    database.url = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Primary PostgreSQL connection string for DATABASE_URL.";
    };

    database.adminUrl = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional admin PostgreSQL connection string for DATABASE_URL_ADMIN.";
    };

    redis.url = lib.mkOption {
      type = lib.types.str;
      default = "redis://127.0.0.1:6379";
      description = "Redis connection string for REDIS_URL.";
    };

    tmdbApiKey = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional TMDB API key for TMDB_API_KEY.";
    };

    cacheDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/cache/ferrex";
      description = "Cache directory path for CACHE_DIR.";
    };

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Additional environment variable overrides for ferrex-server.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "ferrex";
      description = "System user account running ferrex-server.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "ferrex";
      description = "System group for ferrex-server.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the configured ferrex-server TCP port in the firewall.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.mediaRoot != null;
        message = "services.ferrex.mediaRoot must be set when services.ferrex.enable = true";
      }
      {
        assertion = cfg.database.url != null;
        message = "services.ferrex.database.url must be set when services.ferrex.enable = true";
      }
    ];

    users.groups.${cfg.group} = { };
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
    };

    systemd.tmpfiles.rules = [
      "d ${toString cfg.cacheDir} 0750 ${cfg.user} ${cfg.group} -"
    ];

    systemd.services.ferrex-server = {
      description = "Ferrex Media Server";
      after = [ "network.target" "postgresql.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/ferrex-server";
        User = cfg.user;
        Group = cfg.group;
      };
      environment = {
        DATABASE_URL = cfg.database.url;
        REDIS_URL = cfg.redis.url;
        CACHE_DIR = toString cfg.cacheDir;
        SERVER_HOST = cfg.host;
        SERVER_PORT = toString cfg.port;
        MEDIA_ROOT = toString cfg.mediaRoot;
      }
      // lib.optionalAttrs (cfg.database.adminUrl != null) {
        DATABASE_URL_ADMIN = cfg.database.adminUrl;
      }
      // lib.optionalAttrs (cfg.tmdbApiKey != null) {
        TMDB_API_KEY = cfg.tmdbApiKey;
      }
      // cfg.environment;
    };

    networking.firewall.allowedTCPPorts = lib.optional cfg.openFirewall cfg.port;
  };
}
