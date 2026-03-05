# Mission: Bio DynamX Clonable Voice Agent Factory

## Objective

Build a production-grade, multi-tenant voice agent infrastructure on Google Cloud
that can dynamically spawn, configure, and manage AI-powered voice agents using
Vertex AI Gemini Live native audio streaming.

## Core Principles

### The Bi-Lateral Approach

We build biologically compatible tools that minimize "brain tax." All agent personas
operate on the Neuro-Framework:

1. **Neurobiology (Miller's Law):** Keep logic flows and spoken sentences concise —
   7±2 chunks maximum per interaction turn.
2. **Neurosales (Old Brain Targeting):** Hit the amygdala first with high-contrast
   pain points and tangibility to drive urgency.
3. **Neuromarketing (Emotional Resonance):** Optimize for attention and emotional
   resonance to bypass the skeptical neocortex before presenting logical ROI.

## Architecture

```
┌─────────────────────────────────────────────┐
│              IRONCLAW ENGINE                │
│                                             │
│  ┌─────────┐  ┌──────────┐  ┌───────────┐  │
│  │ Session  │  │  Agent   │  │   Hot     │  │
│  │ Manager  │  │ Registry │  │  Reload   │  │
│  │ (Tokio)  │  │(RwLock)  │  │ (notify)  │  │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  │
│       │             │              │         │
│       └─────────────┼──────────────┘         │
│                     │                        │
│  ┌──────────────────┴───────────────────┐    │
│  │         Vertex AI WebSocket          │    │
│  │    (Gemini Live Native Audio)        │    │
│  └──────────────────────────────────────┘    │
│                                             │
└─────────────────┬───────────────────────────┘
                  │
    ┌─────────────┴──────────────┐
    │     profiles/ (YAML)       │
    │  ┌──────┐ ┌──────┐ ┌───┐  │
    │  │Jenny │ │Mark  │ │...│  │
    │  └──────┘ └──────┘ └───┘  │
    └────────────────────────────┘
```

## Success Criteria

- [ ] Rust workspace compiles cleanly
- [ ] Vertex AI WebSocket integration connects and streams audio
- [ ] Hot-reloading watcher detects new/modified YAML profiles
- [ ] Session manager handles concurrent voice sessions
- [ ] Docker container builds and runs on Cloud Run
