//go:build windows && amd64

package dxpdf

// The one platform where this package links a DLL, not a static archive.
// The first attempt tried to statically link `dxpdf.lib` built for
// `x86_64-pc-windows-msvc`, and failed exactly where the comment that used
// to live here predicted: cgo on Windows links via MinGW-w64's `gcc`/`ld`
// by default, and GNU ld cannot resolve the MSVC-mangled C++ runtime
// symbols (operator new/delete, std::vector, ...) that Skia's object code
// pulls into a static archive — undefined references like `??2@YAPEAX_K@Z`
// at `go test` link time, not at build time.
//
// The two ways around that were both dead ends, checked rather than
// assumed:
//
//   - Rebuilding with `x86_64-pc-windows-gnu` (MinGW ABI) instead of
//     `-msvc`, so the archive's object code would match GNU ld's
//     expectations: `skia-bindings` has no prebuilt binaries for that
//     target, and Skia's own Windows build only targets MSVC/clang-cl —
//     https://github.com/rust-skia/rust-skia/issues/345 and /769.
//   - Pointing cgo at MSVC's own `link.exe` (`CC=cl` plus an MSVC dev
//     environment) so the linker matches the archive's ABI: Go's linker
//     has no MSVC object-file support at all, on either side of the link —
//     https://github.com/golang/go/issues/20982 is still open, and a Go
//     team member states plainly in that thread that there is currently
//     none.
//
// Dynamic linking sidesteps both: only the plain `extern "C"` export
// boundary has to resolve at link time, and the Windows x64 calling
// convention is one ABI shared by MSVC and MinGW-w64 alike — the system
// DLLs below already prove this, since they're MSVC-built and have linked
// clean via MinGW since the first attempt. It's also precedented in this
// repo: the Windows Python wheel already links dynamically for the same
// underlying reason (a `cdylib`, not the `staticlib` the other three
// platforms use).
//
// What that costs: `dxpdf.dll` has to be *loaded*, not just linked against,
// so it has to be next to the consuming binary or on `PATH` at runtime —
// unlike every other platform, where `libdxpdf.a` is fully absorbed into
// the output binary and there is nothing left to ship separately. See
// `go/internal/capi/lib/README.md` for where the DLL comes from and how a
// consumer is expected to deploy it.
//
// `go/internal/capi/dxpdf.def` names every exported symbol; `dlltool`
// turns it into `libdxpdf.a`, a MinGW-native import library, without ever
// touching the MSVC-produced `dxpdf.dll.lib` import stub — so this doesn't
// depend on GNU ld being able to read an MSVC import library either.
// That's what makes `-ldxpdf` below the ordinary GNU convention rather
// than another MSVC-naming special case.

/*
#cgo LDFLAGS: -L${SRCDIR}/internal/capi/lib/windows_amd64 -ldxpdf -ldwrite -lwindowscodecs -lole32 -loleaut32 -lgdi32 -luser32 -ladvapi32 -lshell32
*/
import "C"
