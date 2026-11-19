# The scanner

<doc-section name="parse_config">

## What the scanner refuses

`parse_config` is given exactly one directory <doc-claim name="takes-one-directory"/> and never searches
for another. It refuses a directory that holds no `config.json` <doc-claim name="rejects-missing-config"/>.

<doc-section name="project-scan">

## What a refusal names

The message of every refusal names the directory it refused <doc-claim name="names-what-it-refused"/>, and
`evidence/refusals.txt` is the artefact that collects them.

The claims this section states, and the evidence each one rests on:

<doc-each source="claims">
- `<doc-value field="predicate"/>`: <doc-value field="text"/> (channel: <doc-value field="channel"/>)
