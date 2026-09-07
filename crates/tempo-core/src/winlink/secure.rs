//! The Winlink secure-login challenge-response (`;PQ:` → `;PR:`).
//!
//! The CMS answers the SID exchange with a `;PQ:` challenge; the client must reply `;PR:` with a
//! response derived from the challenge, the account password, and a fixed 64-byte protocol salt.
//! The salt is a **public protocol constant**, not a key — it is published in every open Winlink
//! client — and MD5 here is the wire format's choice, never a security decision Nexus makes.
//!
//! The exact digest derivation is not formally specified anywhere; the reference implementations
//! agree on the shape and not on every detail (byte order, masking, decimal formatting). Every
//! such choice is isolated inside one function so a live CMS connect can settle it in one place.
//!
//! Not yet implemented — the module exists so the rest of `winlink` can name it.
