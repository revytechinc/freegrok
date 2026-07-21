fn main() {
    let mut build = cc::Build::new();
    build.file("sqlite-vec.c").define("SQLITE_CORE", None);

    // sqlite-vec.c redefines uint8_t/uint16_t/uint64_t from BSD-style
    // u_int*_t without including <sys/types.h>. On FreeBSD (and any OS where
    // <stdint.h> already provides uint*_t), that block is wrong:
    //   - without sys/types.h: unknown type name 'u_int8_t'
    //   - with sys/types.h: redefinition of typedef 'uint8_t'
    // Skip the block by defining one of its exclude guards. __wasi__ is the
    // least invasive of the existing guards (no other behavior depends on it
    // in this C file for our SQLITE_CORE build).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("freebsd") {
        build.define("__wasi__", None);
    }

    build.compile("sqlite_vec0");
}
