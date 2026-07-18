# Third-Party Notices

`slowrx.rs` is a Rust port of [slowrx](https://github.com/windytan/slowrx),
the SSTV decoder by Oona Räisänen (OH2EIQ). Significant portions of this
crate's algorithms — VIS detection, mode-specification tables,
frequency-to-pixel mappings, sync correlation — are translated from
slowrx's C source code. Per-file headers in `src/` identify which
modules are direct translations and credit the corresponding slowrx file.

## slowrx — ISC License

```text
Copyright (c) 2007-2013, Oona Räisänen (OH2EIQ [at] sral.fi)

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

The ISC license is functionally equivalent to the 2-clause BSD and MIT
licenses; redistributing the resulting Rust port under MIT preserves
slowrx's permission terms while adding the standard MIT warranty
disclaimer. The attribution and ISC permission notice above are
preserved here in compliance with slowrx's distribution terms.

## Project links

- slowrx repository: https://github.com/windytan/slowrx
- slowrx project page: https://windytan.github.io/slowrx/
- Author: Oona Räisänen (OH2EIQ), https://windytan.github.io/
