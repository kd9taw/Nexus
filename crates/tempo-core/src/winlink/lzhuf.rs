//! LZHUF — the adaptive-Huffman-over-LZSS codec Winlink uses for compressed B2F bodies.
//!
//! LZSS (Haruhiko Okumura) with adaptive Huffman coding (Haruyasu Yoshizaki) over it, in the exact
//! form Winlink forwarding requires: the compressed image is an interoperability fact, so this is a
//! port of a specific implementation rather than an independent codec, and it carries that source's
//! licence in its own header once the port lands.
//!
//! One wire question is undecidable from the documents — whether the CRC16 covers the compressed or
//! the uncompressed image — so it lives behind a single named constant. A round-trip through this
//! module proves self-consistency and nothing about the wire; only a real peer settles it.
//!
//! Not yet implemented — the module exists so the rest of `winlink` can name it.
