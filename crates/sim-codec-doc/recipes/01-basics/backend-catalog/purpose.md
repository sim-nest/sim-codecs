# Markup backend catalog (descriptor)

The document codec exposes a catalog of implemented and tracked markup backends. Markdown,
Typst, AsciiDoc, and LaTeX are installed as strict read/write codecs; tracked entries such
as Djot, Org, WikiText, and Texinfo are cataloged without silently registering decoders.
Asking the runtime to decode with a tracked `codec:markup/<id>` returns an unknown-codec
error instead of treating the input as Markdown.
