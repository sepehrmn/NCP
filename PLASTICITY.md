# Single neuron, populations, custom parameters, and plasticity over NCP

NCP records and stimulates at the granularity of **a single neuron, a synapse, or
a whole population**, with **custom neuron parameters**, and supports closing a
**learning loop** — short- and long-term synaptic plasticity driven by feedback
from the plant (e.g. a UAV). The wire vocabulary for all of this already exists;
this note maps it and records what the NEST backend now implements.

## Single neuron vs population

- **Single neuron:** `NetworkRef.population_sizes = {"n0": 1}` builds one neuron.
- **Population:** `{"pop": 64}` builds 64. Same contract, any size.
- **Pick which neuron(s) to record:** `RecordTarget.ids` selects indices; for an
  analog `V_m` multimeter the backend records `ids[0]` (or neuron 0) so a single
  neuron *or* a representative of a population gives an unambiguous trace
  (a multimeter on N>1 returns interleaved rows). Spikes/rate record the whole
  target population.

## Custom-parameter neurons + the multimeter

- **`kind=builtin`:** `NetworkRef.params` (e.g. `{"V_th": -50.0, "tau_m": 15.0,
  "E_L": -65.0}`) are applied at `nest.Create` — so the single neuron or the whole
  population is built with custom parameters. Read their `V_m` over time with an
  `Observable::V_m` record target (the multimeter), at `cadence_ms`.
- **`kind=handle`:** parameters come from the NESTML-generated model (Engram's
  generate→compile path); `params` is not used for neuron creation. This is the
  path for fully custom neuron/synapse dynamics.
- The `multimeter` records `V_m`; the same mechanism extends to other recordables
  a model exposes via `RecordTarget.recordables[]` (`g_ex`/`g_in`/`w`/`rate`, …),
  resolved as the multimeter's `record_from` list (landed in #10).

## Plasticity — long-term, short-term, and reward-modulated (UAV feedback)

**The vocabulary is already in the protocol** — no new wire field is needed:
- `Observable::Weight` — **read synaptic weights** (now implemented: the backend
  reads `GetConnections(source=pop).get("weight")` each step).
- `StimulusKind::WeightSet` — set/reset weights directly.
- the ordinary `rate_hz` / `current_pA` stimulus — **the reward/modulation
  channel** (drives a neuromodulatory source; see R-STDP below).

| Plasticity | NEST mechanism | NCP mapping |
|---|---|---|
| **Short-term (STP)** — facilitation/depression | `tsodyks2_synapse` (intrinsic, activity-driven) | choose the synapse in the network (handle/NESTML); read with `Observable::Weight`; modulated indirectly by the input drive |
| **Long-term (LTP/LTD)** — Hebbian STDP | `stdp_synapse` (spike-timing-driven) | plastic synapses in the network; read weight trajectories with `Observable::Weight` |
| **Reward-modulated (R-STDP)** — *the UAV-feedback loop* | `stdp_dopamine_synapse` + `volume_transmitter` fed by a dopaminergic source | the UAV's outcome (reward/error scalar) → set the source population's `rate_hz`/`current_pA` via the **existing stimulus** → neuromodulation gates the weight update → read with `Observable::Weight` |

### The closed learning loop (feedback from the UAV)
```
sensor → SNN(controller) → command → UAV acts → outcome (reward/error)
   ▲                                                      │
   └──────  Observable::Weight (read learned weights)     │
                                                          ▼
      NCP reward stimulus (rate_hz/current_pA) → volume_transmitter
            → modulates stdp_dopamine_synapse weights  ─────────────┘
```
So a UAV-derived reward signal closes a **reinforcement-learning-style plasticity
loop** entirely within NCP's existing messages: perception in, command out, reward
back in as a stimulus, weights observable out — `calibrated_posterior=false`
throughout (this is a control/learning artifact, never a validated claim). This is
the substrate an SNN-RL controller trains against.

## What the backend implements vs. what the network must declare

- **Implemented in `NestBackend`:** custom neuron params at `Create` (`builtin`),
  `ids`-selected `V_m` recording, and `Observable::Weight` readback.
- **Declared by the network (`kind=handle` / NESTML / PyNEST):** the *plastic
  synapses themselves* (STP/STDP/R-STDP) and the dopaminergic source +
  `volume_transmitter`. The single-population quick path has no synapses, so
  `Observable::Weight` returns empty there — plasticity needs a network with plastic
  connections, which is exactly what the handle/generate path builds.

Honest scope: NCP provides the *interface* to plastic networks (read/observe
weights, inject reward, set weights) and the closed loop around them; the plastic
network is built via Engram's generate→compile machinery, not auto-synthesized by
the quick built-in path.
