Plain chat answer, as an assistant would give it in an ordinary conversation. The control for
the `area-drilldown` page family. Not a design.

---

The fixture has 6 areas and 6 shared contracts.

Ownership is thin: only 3 of the 6 contracts have an owning area — locale bundle format is owned by web; auth token schema is owned by identity service; product catalog API is owned by catalog service. The remaining 3 (event envelope, telemetry field names, design tokens)
have no owner at all, which means several areas depend on them and nobody is the authority for
them.

The apps (web, iOS, Android) are mostly consumers. iOS and Android are both mid-migration on the
product catalog API. The services own what they serve: identity owns the auth token schema,
catalog owns the product catalog API. The data platform owns nothing and consumes two contracts.

If you want to enter an area by what it owns rather than by where its folder sits: web owns the
locale bundle format, identity owns auth, catalog owns the catalog API, and iOS, Android and the
data platform own nothing.
