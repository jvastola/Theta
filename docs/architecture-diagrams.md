# Theta Engine System Architecture Diagrams

**Doc Guide:**
- For full system overview, see [Architecture](architecture.md).
- For protocol and schema details, see [Network Protocol Schema Plan](network_protocol_schema_plan.md).
- For roadmap and telemetry context, see [INDEX](INDEX.md) and [Phase 5 Parallel Plan](phase5_parallel_plan.md).

This document captures high-level UML-style views of the Theta Engine. Diagrams are authored in [Mermaid](https://mermaid.js.org/) so they can be rendered directly in supporting tooling.

## Module Overview

```mermaid
classDiagram
    class Engine {
        +run()
        +add_system()
        +add_parallel_system()
        +world()
    }
    class Scheduler {
        +tick(delta)
        +add_system(stage, name, system)
        +add_parallel_system(stage, name, system)
        -world: World
        -buckets: StageBucket[]
        -last_profile: FrameProfile
    }
    class Renderer {
        +render(delta)
        +config()
        -backend: GpuBackend
        -vr: VrBridge
    }
    class VrBridge {
        <<interface>>
        +label()
        +acquire_views()
        +present()
        +present_gpu()
    }
    class GpuBackend {
        <<interface>>
        +label()
        +render_frame(inputs, views)
    }
    class World {
        +spawn()
        +insert(entity, component)
        +get(entity)
    }
    class EditorModule
    class NetworkModule

    Engine --> Scheduler
    Engine --> Renderer
    Engine --> World
    Renderer --> GpuBackend
    Renderer --> VrBridge
    Scheduler --> World
    Engine --> EditorModule
    Engine --> NetworkModule
```

## Frame Execution Flow

```mermaid
sequenceDiagram
    autonumber
    participant Engine
    participant Scheduler
    participant Renderer
    participant VrBridge
    participant GpuBackend

    Engine->>Scheduler: tick(delta)
    Scheduler->>World: run Startup/Simulation systems
    Scheduler->>World: run Render/Editor systems (parallel where possible)
    Scheduler-->>Engine: FrameProfile
    Engine->>Renderer: render(delta)
    Renderer->>VrBridge: acquire_views()
    Renderer->>GpuBackend: render_frame(inputs, views)
    GpuBackend-->>Renderer: RenderSubmission (swapchain handles + GPU surfaces)
    alt GPU submission available
        Renderer->>VrBridge: present_gpu(submission)
    else
        Renderer->>VrBridge: present(vr_submission)
    end
    VrBridge-->>Renderer: acknowledge
    Renderer-->>Engine: success
    Engine->>World: update diagnostics/components
```

## Data Flow: VR Input Sampling

```mermaid
graph TD
    subgraph Providers
        Simulated[SimulatedInputProvider]
        OpenXR[OpenXrInputProvider]
    end
    Providers --> MutexInput[Shared VrInputProvider]
    MutexInput -->|sample(delta)| Engine
    Engine --> Scheduler
    Scheduler --> World
    World -->|update| Head[TrackedPose]
    World --> Left[ControllerState (L)]
    World --> Right[ControllerState (R)]
    World --> Stats[FrameStats]
    Stats --> Renderer
    Stats --> Editor
```

## Networking Integration Roadmap

```mermaid
stateDiagram-v2
    [*] --> LocalSim
    LocalSim --> TelemetrySync: expose profiler data & input state
    TelemetrySync --> ECSReplication: diff FrameStats + VR components
    ECSReplication --> MultiplayerAlpha: integrate QUIC/WebRTC transport
    MultiplayerAlpha --> [*]
```

These diagrams will evolve as the engine matures. Update them alongside major architectural shifts to keep the visualization aligned with the implementation.
