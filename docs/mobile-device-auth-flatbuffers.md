# Mobile device-auth FlatBuffers contract

`mobile/shared/schemas/auth.fbs` defines the mobile DTOs for the device password-login, PIN challenge/login, device status, device-management summary, and known-device profile-selection flows. Rust bindings are generated into `ferrex-flatbuffers`; Kotlin bindings are generated on demand for Android/TV by `mobile/shared/codegen/generate-kotlin.sh`.

As of this contract slice, Ferrex server device-auth endpoints (`/api/v1/auth/device/login`, `/pin`, `/pin/challenge`, and `/known-users`) still use JSON extractors and do not participate in request/response content negotiation. The FlatBuffers schema is therefore a shared client contract for follow-up mobile work; server-side FlatBuffers handler tests should be added when those endpoints accept `application/x-flatbuffers`.
