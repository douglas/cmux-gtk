fn main() {
    // $ORIGIN covers dev runs from target/release (the build copies the
    // vendored .so files there); ../lib/jmux covers the installed layout
    // ($PREFIX/bin/jmux-app + $PREFIX/lib/jmux/*.so, shipped by install.sh).
    // Pinning the search path matters: without it the loader fell back to a
    // stale /usr/lib/libghostty.so from an old cmux-gtk package, silently
    // reintroducing fixed bugs (the free_text arity leak behind the 47 GB
    // OOM).
    println!("cargo:rustc-link-arg-bin=jmux-app=-Wl,-rpath,$ORIGIN");
    println!("cargo:rustc-link-arg-bin=jmux-app=-Wl,-rpath,$ORIGIN/../lib/jmux");
}
