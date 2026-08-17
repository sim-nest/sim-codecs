# Inspect a retained classfile without a JVM

Decode the retained classfile as inert data, then browse its bounded `constants`,
`attributes`, and `instructions` directories. Every instruction exposes both a
method-local code offset and an absolute byte offset into the retained `bytes`.
