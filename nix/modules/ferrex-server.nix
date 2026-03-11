{ config, lib, pkgs, ... }:
let
  cfg = config.services.ferrex;

  # Build the environment file from all configured options.
  # This replaces what `ferrexctl init --non-interactive` would produce, but
  # in a fully declarative way.  Secrets come from EnvironmentFile (sops-nix,
  # agenix, or a manually-managed file) so they never hit the Nix store.
  envFile = pkgs.writeText "ferrex-server.env" (lib.concatStringsSep "\n" (
    lib.mapAttrsToList (k: v: "${k}=${v}") (baseEnvironment // cfg.environment)
  ));

  baseEnvironment = {
    # Server
    SERVER_HOST = cfg.host;
    SERVER_PORT = toString cfg.port;
    FERREX_SERVER_URL = cfg.serverUrl;

    # Database
    DATABASE_URL = cfg.database.url;
    SQLX_OFFLINE = "true";

    # Redis
    REDIS_URL = cfg.redis.url;

    # Media
    MEDIA_ROOT = toString cfg.mediaRoot;

    # Cache
    CACHE_DIR = toString cfg.cacheDir;
    TRANSCODE_CACHE_DIR = "${toString cfg.cacheDir}/transcode";
    THUMBNAIL_CACHE_DIR = "${toString cfg.cacheDir}/thumbnails";

    # CORS
    CORS_ALLOWED_ORIGINS = lib.concatStringsSep "," cfg.cors.allowedOrigins;
    CORS_ALLOW_CREDENTIALS = lib.boolToString cfg.cors.allowCredentials;

    # HSTS
    HSTS_MAX_AGE = toString cfg.hsts.maxAge;
    HSTS_INCLUDE_SUBDOMAINS = lib.boolToString cfg.hsts.includeSubdomains;
    HSTS_PRELOAD = lib.boolToString cfg.hsts.preload;

    # Security
    ENFORCE_HTTPS = lib.boolToString cfg.security.enforceHttps;
    TRUST_PROXY_HEADERS = lib.boolToString cfg.security.trustProxyHeaders;

    # TMDB
    TMDB_LANG = cfg.tmdb.lang;
    TMDB_REGION = cfg.tmdb.region;
  }
  // lib.optionalAttrs (cfg.database.adminUrl != null) {
    DATABASE_URL_ADMIN = cfg.database.adminUrl;
  }
  // lib.optionalAttrs (cfg.tmdb.apiKey != null) {
    TMDB_API_KEY = cfg.tmdb.apiKey;
  }
  // lib.optionalAttrs (cfg.ffmpeg.ffmpegPath != null) {
    FFMPEG_PATH = cfg.ffmpeg.ffmpegPath;
  }
  // lib.optionalAttrs (cfg.ffmpeg.ffprobePath != null) {
    FFPROBE_PATH = cfg.ffmpeg.ffprobePath;
  };

in
{
  options.services.ferrex = {
    enable = lib.mkEnableOption "ferrex media server";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ferrex-server;
      description = "Package providing the ferrex-server binary.";
    };

    ctlPackage = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ferrexctl;
      description = "Package providing ferrexctl (used for DB migrations).";
    };

    # ── Core server ──────────────────────────────────────────────────

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

    serverUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://localhost:${toString cfg.port}";
      defaultText = lib.literalExpression ''"http://localhost:''${toString cfg.port}"'';
      description = "Public URL clients use to reach this server (FERREX_SERVER_URL).";
    };

    # ── Database ─────────────────────────────────────────────────────

    database.url = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Primary PostgreSQL connection string (DATABASE_URL).";
    };

    database.adminUrl = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Admin PostgreSQL connection string for migrations (DATABASE_URL_ADMIN).";
    };

    database.runMigrations = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Run database migrations (ferrex-server db migrate) before starting.
        This is idempotent and safe to leave enabled.
      '';
    };

    # ── Redis ────────────────────────────────────────────────────────

    redis.url = lib.mkOption {
      type = lib.types.str;
      default = "redis://127.0.0.1:6379";
      description = "Redis connection string (REDIS_URL).";
    };

    redis.createLocally = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable a local Redis instance for ferrex.";
    };

    # ── TMDB ─────────────────────────────────────────────────────────

    tmdb.apiKey = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "TMDB API key. Prefer secretsFile for this.";
    };

    tmdb.lang = lib.mkOption {
      type = lib.types.str;
      default = "en-US";
      description = "TMDB language code.";
    };

    tmdb.region = lib.mkOption {
      type = lib.types.str;
      default = "US";
      description = "TMDB region code.";
    };

    # ── Cache ────────────────────────────────────────────────────────

    cacheDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/cache/ferrex";
      description = "Cache directory (CACHE_DIR). Transcode/thumbnail subdirs are created automatically.";
    };

    # ── CORS ─────────────────────────────────────────────────────────

    cors.allowedOrigins = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "http://localhost:5173"
        "http://localhost:${toString cfg.port}"
      ];
      defaultText = lib.literalExpression ''[ "http://localhost:5173" "http://localhost:''${toString cfg.port}" ]'';
      description = "CORS allowed origins.";
    };

    cors.allowCredentials = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "CORS allow credentials.";
    };

    # ── HSTS ─────────────────────────────────────────────────────────

    hsts.maxAge = lib.mkOption {
      type = lib.types.int;
      default = 0;
      description = "HSTS max-age in seconds (0 = disabled).";
    };

    hsts.includeSubdomains = lib.mkOption {
      type = lib.types.bool;
      default = false;
    };

    hsts.preload = lib.mkOption {
      type = lib.types.bool;
      default = false;
    };

    # ── Security / TLS ───────────────────────────────────────────────

    security.enforceHttps = lib.mkOption {
      type = lib.types.bool;
      default = false;
    };

    security.trustProxyHeaders = lib.mkOption {
      type = lib.types.bool;
      default = false;
    };

    # ── FFmpeg ───────────────────────────────────────────────────────

    ffmpeg.ffmpegPath = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Override path to ffmpeg binary (FFMPEG_PATH).";
    };

    ffmpeg.ffprobePath = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Override path to ffprobe binary (FFPROBE_PATH).";
    };

    # ── Secrets ──────────────────────────────────────────────────────

    secretsFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        Path to an EnvironmentFile containing secrets that must NOT go in the
        Nix store.  Expected keys (any subset):

          AUTH_PASSWORD_PEPPER=<random-64-char>
          AUTH_TOKEN_KEY=<random-64-char>
          FERREX_SETUP_TOKEN=<random-48-char>
          TMDB_API_KEY=<your-key>

        Generate initial secrets with:
          ferrexctl init --non-interactive --print-only 2>/dev/null | grep -E '^(AUTH_|FERREX_SETUP_TOKEN)'

        Or use sops-nix / agenix to manage the file declaratively.
      '';
    };

    autoGenerateSecrets = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Automatically generate auth secrets on first boot if secretsFile is
        not set.  Secrets are persisted to /var/lib/ferrex/secrets.env and
        reused on subsequent boots.

        Disable this if you manage secrets externally (sops-nix, agenix, vault).
      '';
    };

    # ── Escape hatch ─────────────────────────────────────────────────

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = ''
        Additional/override environment variables for ferrex-server.
        These take precedence over all structured options above.
      '';
    };

    # ── System ───────────────────────────────────────────────────────

    user = lib.mkOption {
      type = lib.types.str;
      default = "ferrex";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "ferrex";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
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
      {
        assertion = cfg.secretsFile != null || cfg.autoGenerateSecrets;
        message = ''
          services.ferrex: either set secretsFile to a file containing
          AUTH_PASSWORD_PEPPER, AUTH_TOKEN_KEY, and FERREX_SETUP_TOKEN,
          or leave autoGenerateSecrets = true (default) to have them
          generated on first boot.
        '';
      }
    ];

    # ── Users & directories ──────────────────────────────────────────

    users.groups.${cfg.group} = { };
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
    };

    systemd.tmpfiles.rules = [
      "d ${toString cfg.cacheDir} 0750 ${cfg.user} ${cfg.group} -"
      "d ${toString cfg.cacheDir}/transcode 0750 ${cfg.user} ${cfg.group} -"
      "d ${toString cfg.cacheDir}/thumbnails 0750 ${cfg.user} ${cfg.group} -"
    ];

    # ── Redis (optional local instance) ──────────────────────────────

    services.redis.servers.ferrex = lib.mkIf cfg.redis.createLocally {
      enable = true;
      port = 6379;
    };

    # ── Secret generation service ────────────────────────────────────

    systemd.services.ferrex-secrets-init = lib.mkIf (cfg.autoGenerateSecrets && cfg.secretsFile == null) {
      description = "Generate ferrex auth secrets on first boot";
      wantedBy = [ "ferrex-server.service" ];
      before = [ "ferrex-server.service" ];
      unitConfig.ConditionPathExists = "!/var/lib/ferrex/secrets.env";
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        StateDirectory = "ferrex";
        StateDirectoryMode = "0750";
        User = "root";
        Group = cfg.group;
      };
      # Use ferrexctl to generate secrets, or fall back to openssl if not available
      script = ''
        gen() { ${pkgs.openssl}/bin/openssl rand -base64 "$1" | tr -d '\n=+/' | head -c "$1"; }
        DIR=/var/lib/ferrex
        FILE="$DIR/secrets.env"

        install -d -m 0750 -o root -g ${cfg.group} "$DIR"

        {
          echo "AUTH_PASSWORD_PEPPER=$(gen 64)"
          echo "AUTH_TOKEN_KEY=$(gen 64)"
          echo "FERREX_SETUP_TOKEN=$(gen 48)"
        } > "$FILE"

        chmod 0640 "$FILE"
        chown root:${cfg.group} "$FILE"
      '';
    };

    # ── DB migration service ─────────────────────────────────────────

    systemd.services.ferrex-db-migrate = lib.mkIf cfg.database.runMigrations {
      description = "Ferrex database migrations";
      after = [ "postgresql.service" "network.target" ]
        ++ lib.optional (cfg.autoGenerateSecrets && cfg.secretsFile == null) "ferrex-secrets-init.service";
      before = [ "ferrex-server.service" ];
      wantedBy = [ "ferrex-server.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        User = cfg.user;
        Group = cfg.group;
        EnvironmentFile = [
          envFile
        ] ++ (if cfg.secretsFile != null
              then [ cfg.secretsFile ]
              else lib.optional cfg.autoGenerateSecrets "/var/lib/ferrex/secrets.env");
      };
      script = ''
        ${cfg.package}/bin/ferrex-server db migrate
      '';
    };

    # ── Main server service ──────────────────────────────────────────

    systemd.services.ferrex-server = {
      description = "Ferrex Media Server";
      after = [ "network.target" "postgresql.service" ]
        ++ lib.optional cfg.redis.createLocally "redis-ferrex.service"
        ++ lib.optional cfg.database.runMigrations "ferrex-db-migrate.service"
        ++ lib.optional (cfg.autoGenerateSecrets && cfg.secretsFile == null) "ferrex-secrets-init.service";
      requires =
        lib.optional cfg.database.runMigrations "ferrex-db-migrate.service"
        ++ lib.optional (cfg.autoGenerateSecrets && cfg.secretsFile == null) "ferrex-secrets-init.service";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/ferrex-server";
        User = cfg.user;
        Group = cfg.group;
        Restart = "on-failure";
        RestartSec = 5;
        StateDirectory = "ferrex";

        # Load all config from a single env file (non-secret values)
        # plus the secrets file (never in the Nix store)
        EnvironmentFile = [
          envFile
        ] ++ (if cfg.secretsFile != null
              then [ cfg.secretsFile ]
              else lib.optional cfg.autoGenerateSecrets "/var/lib/ferrex/secrets.env");

        # Hardening
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [
          (toString cfg.cacheDir)
          "/var/lib/ferrex"
        ] ++ lib.optional (cfg.mediaRoot != null) (toString cfg.mediaRoot);
        PrivateTmp = true;
      };
    };

    networking.firewall.allowedTCPPorts = lib.optional cfg.openFirewall cfg.port;
  };
}
