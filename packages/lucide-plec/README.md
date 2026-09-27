# `@plec/lucide`

> **Unofficial and experimental.** This package is not affiliated with, endorsed by, maintained by, or supported by the Lucide project or its maintainers.

This workspace generates PLEC-compatible icon components from the installed Lucide package. It keeps Lucide-specific knowledge out of PLEC and application code.

The adapter's own code is MIT-licensed. Lucide icon definitions retain Lucide's ISC license and its separate MIT notice for Feather-derived icons. Applications that distribute Lucide-derived output must include Lucide's license notice; the fullstack app copies it to `dist/public/licenses/Lucide-LICENSE.txt` during build. The fullstack app also copies the Outfit and Raleway SIL Open Font License notices to that directory.

It is intentionally private and experimental. Do not depend on it for production use, and do not treat it as an official Lucide package or support channel.

The generated components return ordinary SVG graph nodes. A future compiler component-manifest integration can consume the same generated catalog without making PLEC aware of Lucide icons.

For Lucide itself, use the official project and packages: [lucide.dev](https://lucide.dev).
