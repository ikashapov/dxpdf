# dxpdf (Go bindings)

Go bindings for [dxpdf](https://github.com/nerdy-pro/dxpdf), a fast
DOCX-to-PDF converter powered by Skia. This package is a thin cgo layer over
the same Rust engine the CLI and Python package use — see
[`src/capi.rs`](../src/capi.rs) for the C ABI it binds to.

## Requirements

- `CGO_ENABLED=1` and a C compiler (cgo requirement).
- One of linux/amd64, linux/arm64, darwin/amd64, darwin/arm64, windows/amd64.
  Windows is a different shape from the other four — see "Install" below
  for what that means for deployment.
- No tagged releases of this module yet, so `go get .../go@vX.Y.Z` won't
  resolve. Plain `go get github.com/nerdy-pro/dxpdf/go` (or `@main`, or a
  commit SHA to pin) works fine — Go falls back to a pseudo-version off the
  default branch tip.

## Install

The prebuilt library for every supported platform is committed under
`internal/capi/lib/` (see that directory's own note on why, and
`tests/capi_lib_freshness.rs` in the main repo for how staleness against
the Rust source is caught), so there's no separate fetch step:

```sh
go get github.com/nerdy-pro/dxpdf/go
```

Or, working from a clone of the main repo:

```sh
cd go
go build ./...
```

**Windows only:** the other four platforms link a static archive that's
fully absorbed into your binary — `go get`/`go build` is the whole story.
Windows links `dxpdf.dll` dynamically instead (`internal/capi/lib/
windows_amd64/`'s own note says why), so it's *loaded*, not absorbed: your
built binary needs `dxpdf.dll` next to it, or somewhere on `PATH`, at
runtime — copy it from
`$(go env GOMODCACHE)/github.com/nerdy-pro/dxpdf/go@<version>/internal/capi/lib/windows_amd64/dxpdf.dll`
(or from a clone of the main repo, if you're building against one) into
your build/release output alongside the executable.

## Usage

```go
package main

import (
	"log"

	"github.com/nerdy-pro/dxpdf/go"
)

func main() {
	pdfBytes, err := dxpdf.Convert(docxBytes)
	if err != nil {
		log.Fatal(err)
	}
	_ = pdfBytes

	// Or work with files directly, optionally overriding the embedded-image DPI:
	if err := dxpdf.ConvertFileWithOptions("input.docx", "output.pdf", 300); err != nil {
		log.Fatal(err)
	}
}
```

See [`dxpdf.go`](dxpdf.go) for the full API
(`Convert`/`ConvertWithOptions`/`ConvertFile`/`ConvertFileWithOptions`),
which mirrors the Python package's `convert`/`convert_file` one for one.
