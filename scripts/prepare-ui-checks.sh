# Source this file from UI check scripts that compile the application views.
bash scripts/build-app.sh
pebble_swiftmath_products="$PWD/build/xcode/Build/Products/Debug"
pebble_swiftmath_flags=(-I "$pebble_swiftmath_products" "$pebble_swiftmath_products/SwiftMath.o")
ditto "$pebble_swiftmath_products/SwiftMath_SwiftMath.bundle" build/SwiftMath_SwiftMath.bundle
