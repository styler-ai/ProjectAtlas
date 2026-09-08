# Fixed PDF guest dependency

This directory retains the published source and attribution of `pdf-extract`
0.12.0 by Jeff Muizelaar and contributors. Its published Cargo manifest declares
the MIT license. The registry archive contains no separate license file.

Upstream: https://github.com/jrmuizel/pdf-extract

Published source commit: `b95bf9f6268772d5088f09b0034e488e64294835`.

ProjectAtlas carries these local changes for its fixed, contained PDF guest:

- Normalize trailing whitespace retained in the published source.
- Use the same pinned `lopdf` 0.44.0 as the guest's bounded input owner.
- Decode complete content streams strictly instead of accepting a parsed prefix.
- Key cached fonts by the resolved dictionary's identity within the immutable
  document, preserving independent page and Form resource scopes.
- Refuse incomplete or unmapped character codes instead of silently ending text.
- Refuse ActualText replacement semantics in direct or resource-named marked-content properties before publishing underlying glyphs.
- Interpret quote text-showing operators with their line movement and spacing semantics.
- Apply CID range widths to both inclusive endpoints using the declared width.
- Apply ExtGState font dictionaries and finite sizes through the existing scoped font cache.
- Measure plain-text horizontal advances and font heights from their separate transformed vectors, preserving rotated and anisotropically scaled text spacing.
- Refuse vertical Type 0 fonts selected by Identity-V or an encoding CMap with WMode 1 through the shared font cache owner.
- Apply inherited integer page rotation in the initial graphics transform; reject non-quarter-turn or non-integer rotation before publication.
- Interpret Form XObjects with inherited graphics state and composed caller/Form
  transforms, skip Image pixels without OCR, and refuse unknown XObject subtypes
  or malformed Form matrices.

The guest uses exact-page execution and bounded output collection. It does not
use the upstream convenience function that stops after the first page error.
The original generated glyph-table attribution headers remain intact. They cite:

- https://github.com/michal-h21/htfgen/commits/master/glyphlist-extended.txt
- https://github.com/kohler/lcdf-typetools/blob/master/texglyphlist.txt
- https://github.com/apache/pdfbox/blob/trunk/pdfbox/src/main/resources/org/apache/pdfbox/resources/glyphlist/additional.txt

These changes should be removed when an audited upstream release provides the
same behavior and passes the guest's Form, image, font-scope, malformed-page,
resource, and cancellation regressions.
