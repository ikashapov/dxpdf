//go:build windows && amd64

package dxpdf

// UNVERIFIED FIRST ATTEMPT — see AGENTS.md's Go-bindings note: static-linking
// a Skia-containing archive through cgo on MSVC is a combination this repo
// has not exercised anywhere else (the existing Windows Python wheel links
// dynamically — a cdylib, not this staticlib path). Nothing here has been
// linked or run on a real Windows host yet; it exists to have something
// concrete to iterate against in CI, not because the flag list below is
// known-good the way every other platform's is.
//
// Two things are open, not just this flag list:
//
//   - Toolchain match: the committed windows_amd64 library is built with the
//     `x86_64-pc-windows-msvc` target (matching the only proven Windows Rust
//     build in this repo — `build-wheels`' wheel leg), which produces an
//     MSVC-format `dxpdf.lib`. cgo on Windows defaults to a MinGW-w64/gcc C
//     toolchain, not MSVC's cl.exe/link.exe — whether GNU ld can resolve an
//     MSVC-toolchain-produced archive containing MSVC-ABI C++ object code
//     (Skia itself) is unverified. If linking fails on toolchain/ABI grounds
//     rather than a missing-library grounds, the likely fix is rebuilding
//     with `x86_64-pc-windows-gnu` instead, not a flag change here.
//   - This system-library list is a best-effort guess at Skia's Windows
//     dependencies for a CPU/PDF-only backend (DirectWrite-backed font
//     matching, WIC for image codecs, the COM libraries both need) — not
//     derived from an actual build log the way every other platform's list
//     was (see cgo_linux_amd64.go's comment for what that looked like).
//     Expect this to need correcting against real linker errors.
//
// `-l:dxpdf.lib`, not `-ldxpdf`: the MSVC toolchain names the archive
// without the Unix `lib`-prefix convention `-l` otherwise assumes; `-l:name`
// is GNU ld's syntax for linking a file by its exact name.

/*
#cgo LDFLAGS: -L${SRCDIR}/internal/capi/lib/windows_amd64 -l:dxpdf.lib -ldwrite -lwindowscodecs -lole32 -loleaut32 -lgdi32 -luser32 -ladvapi32 -lshell32
*/
import "C"
