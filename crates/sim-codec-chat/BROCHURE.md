# sim-codec-chat

In one line: It reads and writes model conversations -- prompts, replies, and events -- in one neutral, provider-independent form.

## What it gives you

Conversations with a model come in many shapes: a request you send, the answer that comes back, events along the way, and the record cards that summarize them. This gives all of those a single, tidy text form that does not belong to any one provider. It also understands native provider payloads for OpenAI, Anthropic, Ollama, LM Studio, Lemonade, and OpenAI-compatible local servers, while keeping the saved conversation in the same neutral record. You can capture a whole exchange, store it, move it, or read it later, and it means the same thing regardless of which service produced it. Because the form is consistent, transcripts from different sources line up the same way, so they can be compared, replayed, or archived without special handling for each origin. It keeps the parts of a conversation clearly separated -- who asked, what answered, what happened -- so the record stays legible.

## Why you will be glad

- Transcripts from any provider share one common shape.
- Provider-specific payloads can enter and leave that shape without leaking transport details into the rest of the runtime.
- Whole exchanges can be saved and reopened without losing their structure.
- Requests, replies, and events stay clearly distinguished in the record.

## Where it fits

This is the conversation-transcript member of the SIM codec family. It gives model interactions a neutral written form, so the rest of the runtime can store, move, and inspect them the same way it handles any other value.
