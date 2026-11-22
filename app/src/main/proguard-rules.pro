# UniFFI/JNA use reflection and direct native registration; R8 must not strip
# the generated bindings or JNA's callback/Structure machinery.
-keepattributes *Annotation*,Signature,InnerClasses,EnclosingMethod

-keep class uniffi.snap_ffi.** { *; }
-keepclassmembers class uniffi.snap_ffi.** { *; }
-keep interface uniffi.snap_ffi.** { *; }

-keep class com.sun.jna.** { *; }
-keepclassmembers class com.sun.jna.** { *; }
-keep interface com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.Structure { <fields>; }
-keepclassmembers class * implements com.sun.jna.Callback { *; }
-keepclassmembers interface * extends com.sun.jna.Callback { *; }
-keepclassmembers class * implements com.sun.jna.Library { *; }

-keepclasseswithmembernames class * { native <methods>; }
-dontwarn java.awt.**
-dontwarn com.sun.jna.**
