# FlatBuffers generated contract types are regenerated outside R8's reach and
# must remain stable if future release builds reference them reflectively.
-keep class ferrex.** { *; }
-keepclassmembers class ferrex.** { *; }
