# Nexus documentation

Two things live here, and this page says which is which so you land on the right one.

## The operator manual

**[docs/guide/](guide/index.md)** is the manual — one illustrated page per section of the app,
from the first-run wizard to awards. It is what
[hamradiotools.io/manual](https://hamradiotools.io/manual/) serves, and what the EPUB and PDF are
built from. Start there.

The first hour, in order:

- **[Quick start](quick-start.md)** — install, the first-run wizard a step at a time, first contact
- **[Install](install.md)** — downloads, SmartScreen, SHA-256, where your data lives
- **[Troubleshooting](troubleshooting.md)** — blank window, no decodes, CAT and audio failures
- **[FAQ](faq.md)** — the questions that come up most

## Topic pages

**[docs/manual/](manual/README.md)** is the topic set: pages that answer one question rather than
describing one section of the app. Several have no equivalent in the manual —
[Rig and Audio Setup](manual/Rig-and-Audio-Setup.md),
[Building from Source](manual/Building-from-Source.md),
[Architecture and Protocol](manual/Architecture-and-Protocol.md),
[Frequency Plan](manual/Frequency-Plan.md),
[Field Day](manual/Field-Day.md),
[Integrations](manual/Integrations.md),
[Privacy and Coordinated QSY](manual/Privacy-and-Coordinated-QSY.md) and
[Roadmap](manual/Roadmap.md).

## Specifications and internals

- **[Comprehensive overview](OVERVIEW.md)** — every surface, in depth
- **[Architecture](ARCHITECTURE.md)** and **[data flow](architecture-data-flow.md)**
- **[Tempo protocol](Tempo-Protocol.md)** — the native waveforms, for implementers
- **[Frequency plan](FREQUENCIES.md)** — where the TempoFast and TempoDeep tiers sit on the bands
- **[Interop](interop.md)** — the WSJT-X UDP protocol, companion apps, the CAT broker, cluster feeds
- **[Translation](i18n.md)** — how the catalogs work and how to add a language

`RELEASE_NOTES-*.md` are point-in-time records of what shipped in one version; the running history
is the [changelog](../CHANGELOG.md). `launch/wiki/` is the source for the GitHub and SourceForge
wikis — those pages are pasted there by hand, so edit them here.
