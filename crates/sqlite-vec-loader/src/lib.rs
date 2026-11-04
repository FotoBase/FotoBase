//! Registers the statically linked sqlite-vec module as a SQLite
//! auto-extension (TASK-05-008, PLAN §5 `embeddings` sqlite-vec virtual
//! table): every SQLite connection opened after [`register`] gets the
//! `vec0` virtual-table module.
//!
//! This is a deliberate one-function crate. The single
//! `sqlite3_auto_extension` FFI call is `unsafe`, and the workspace denies
//! `unsafe_code` from the command line, which source-level `allow`
//! attributes cannot override. This crate therefore does not inherit the
//! workspace lints; the unsafe block is confined here and nothing else
//! belongs in this file.

/// Register the sqlite-vec extension. Idempotent and process-wide
/// (`sqlite3_auto_extension` ignores duplicate registrations of the same
/// entry point); must run before the first connection that needs `vec0`.
pub fn register() {
    // `sqlite3_auto_extension` takes an `unsafe extern "C" fn` pointer; the
    // sqlite-vec entry point has exactly that shape but a different Rust
    // signature, hence the transmute (the crate's documented rusqlite
    // integration pattern).
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::os::raw::c_int,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    }
}
