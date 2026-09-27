# xmip-core-path-json-pointer

The `json-pointer` path technology, a technology of
[xmip-core-path](https://github.com/IlleNilsson/xmip-core-path). It carries
`JsonPointerLanguage`, the `PathLanguage` for the language `json-pointer`: a
pointer compiles once, and the JSON a Message carries is parsed once for every
JSON path reading it. Promote reads through it, demote writes through it to
produce a new Stream (ADR-0013), route and process read.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
