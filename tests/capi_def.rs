//! Guards `go/internal/capi/dxpdf.def` against drifting from `src/capi.rs`.
//!
//! Windows is the one platform where the Go bindings link against a DLL
//! rather than a static archive (see `go/cgo_windows_amd64.go`'s module
//! doc for why), and a MinGW-compatible import library has to be generated
//! from a `.def` file naming every exported symbol — there is no build
//! step that derives that list automatically, so a function added to or
//! removed from `src/capi.rs`'s C ABI has to be mirrored here by hand. This
//! test is what notices when that mirroring is forgotten.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `#[no_mangle] pub (unsafe )?extern "C" fn NAME` in `src/capi.rs` —
/// the C ABI surface the `.def` file's `EXPORTS` list must match exactly.
fn exported_capi_functions(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() != "#[no_mangle]" {
            continue;
        }
        for candidate in lines.by_ref() {
            let candidate = candidate.trim();
            if candidate.is_empty() || candidate.starts_with("///") || candidate.starts_with("//") {
                continue;
            }
            let rest = candidate
                .strip_prefix("pub unsafe extern \"C\" fn ")
                .or_else(|| candidate.strip_prefix("pub extern \"C\" fn "));
            if let Some(rest) = rest {
                let name = rest.split(['(', '<']).next().unwrap_or("").trim();
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
            break;
        }
    }
    names
}

/// The `.def` file's `EXPORTS` list, one symbol per line.
fn def_file_exports(def: &str) -> BTreeSet<String> {
    def.lines()
        .skip_while(|line| line.trim() != "EXPORTS")
        .skip(1)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn def_file_exports_match_capi_rs() {
    let root = repo_root();

    let capi_source =
        std::fs::read_to_string(root.join("src/capi.rs")).expect("reading src/capi.rs");
    let def_source = std::fs::read_to_string(root.join("go/internal/capi/dxpdf.def"))
        .expect("reading go/internal/capi/dxpdf.def");

    let from_rust = exported_capi_functions(&capi_source);
    let from_def = def_file_exports(&def_source);

    assert_eq!(
        from_rust, from_def,
        "go/internal/capi/dxpdf.def's EXPORTS list has drifted from src/capi.rs's \
         #[no_mangle] extern \"C\" functions — update the .def file to match \
         (see tests/capi_def.rs's module doc)"
    );
}
