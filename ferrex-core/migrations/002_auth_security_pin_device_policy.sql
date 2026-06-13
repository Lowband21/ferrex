-- Persist authentication policy for official-client PIN validation and device trust.
--
-- PIN proof routes let clients validate raw PIN length and patterns before
-- deriving proof material; those routes store only proof hashes and enforce
-- non-empty proofs, device possession, trusted_until, and lockout.

ALTER TABLE public.auth_security_settings
    ADD COLUMN pin_policy jsonb NOT NULL DEFAULT '{
        "min_length": 4,
        "max_length": 8,
        "require_numeric": true,
        "reject_repeated_digits": true,
        "max_consecutive_identical": 2,
        "reject_sequential_digits": true
    }'::jsonb,
    ADD COLUMN device_trust_policy jsonb NOT NULL DEFAULT '{
        "remember_device_default": false,
        "trust_duration_days": 30,
        "pin_max_attempts": 3,
        "pin_lockout_minutes": 5,
        "admin_pin_unlock_enabled": false
    }'::jsonb;

ALTER TABLE public.auth_security_settings
    ALTER COLUMN pin_policy DROP DEFAULT,
    ALTER COLUMN device_trust_policy DROP DEFAULT;

COMMENT ON TABLE public.auth_security_settings IS 'Authentication policy settings for passwords, official-client PIN validation, device trust, remember-device, lockout, and admin PIN-unlock behavior.';

COMMENT ON COLUMN public.auth_security_settings.pin_policy IS 'JSON payload describing raw PIN validation rules enforced by official clients before deriving proof-route PIN material.';

COMMENT ON COLUMN public.auth_security_settings.device_trust_policy IS 'JSON payload describing remember-device defaults, trusted_until duration, PIN lockout, and admin PIN-unlock behavior.';
