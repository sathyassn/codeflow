# Utility fonts

Locally bundled Latin variable normal faces from Fontsource Variable packages
`@fontsource-variable/archivo`, `@fontsource-variable/inter`, and
`@fontsource-variable/ibm-plex-sans`, each version **5.3.0**. Original SIL Open
Font License texts are included beside the unmodified WOFF2 files.

Sources: https://fontsource.org/fonts/archivo,
https://fontsource.org/fonts/inter, https://fontsource.org/fonts/ibm-plex-sans.
The presentation runtime embeds these files; the portal bundles them as local
assets. Neither requires a font CDN. Other scripts and unavailable glyphs use
the declared system fallback stack. Product design is not bound to these fonts.

The presentation renderer also embeds all three complete license texts and
exposes them in a collapsed Font licenses footer outside the document and
annotation roots. Offline exports keep that footer and the notices. Portal
builds copy `public/fonts-LICENSE.txt`, linked from Display; retain the exact
notices when updating or redistributing the fonts.
